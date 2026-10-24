// Podoxide
// Copyright (C) 2026  Stefan Stark
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program.  If not, see <https://www.gnu.org/licenses/>.

//! CLI argument scanner, flag validator, and mode/action resolver.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::core::config::{DEFAULT_CONFIG_NAME, DEFAULT_CONFIG_PATH};
use crate::core::context::{
    CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, Flags, ImageAction,
    InitAction, LogsAction, Mode, ModeAction, RecipeAction,
};
use crate::error::{CliError, CliErrorKind};
use crate::utilities::path::extract_config_stem;
use crate::utilities::string::{contains_control_or_whitespace, is_empty_value_literal};

const INITIAL_POSITIONALS_CAPACITY: usize = 8;
/// Maximum number of command-line arguments accepted by the parser.
pub const MAX_CLI_ARGS: usize = 256;

/// Parses an iterator of command-line arguments (excluding `argv[0]`) into [`Context`].
///
/// # Errors
///
/// Returns [`CliError`] if argument limits are exceeded, an argument is not valid
/// UTF-8, an unknown or unexpected flag is provided, a required flag value or target
/// is missing, or a mode/action combination is invalid.
pub fn parse_args<I, T>(context: &mut Context, arguments: I) -> Result<(), CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    context.flags = Flags::new();
    let positionals = scan_raw_arguments(&mut context.flags, arguments)?;
    let (mode, action, parameters) = parse_mode(&positionals, context.flags.version)?;
    context.mode = mode;
    context.action = action;
    context.parameters = parameters;
    validate_mode_flags(context)?;
    context.config.name = resolve_config_name(context.flags.config.as_deref());
    context.config.path = resolve_config_path(context.flags.config.as_deref());
    if context.flags.simulate {
        context.config.simulate = true;
    }
    Ok(())
}

/// Resolves a configuration file argument into its configuration name (file stem).
#[must_use]
pub fn resolve_config_name(config: Option<&Path>) -> String {
    config
        .and_then(Path::to_str)
        .and_then(extract_config_stem)
        .unwrap_or(DEFAULT_CONFIG_NAME)
        .to_owned()
}

/// Resolves a configuration file argument into a normalized `.conf` path.
#[must_use]
pub fn resolve_config_path(config: Option<&Path>) -> PathBuf {
    let Some(path) = config else {
        return PathBuf::from(DEFAULT_CONFIG_PATH);
    };
    let Some(raw) = path.to_str() else {
        return PathBuf::from(DEFAULT_CONFIG_PATH);
    };
    if extract_config_stem(raw).is_none() {
        return PathBuf::from(DEFAULT_CONFIG_PATH);
    }
    let has_explicit_prefix =
        raw.starts_with('/') || raw.starts_with("./") || raw.starts_with("../");
    let has_conf_extension = raw.ends_with(".conf");

    let mut resolved = String::with_capacity(raw.len() + 7);
    if !has_explicit_prefix {
        resolved.push_str("./");
    }
    resolved.push_str(raw);
    if !has_conf_extension {
        resolved.push_str(".conf");
    }
    PathBuf::from(resolved)
}

/// Validates and stores a value-taking CLI flag (`--config`, `-c`, `--since`, `--tail`, `--until`) into `flags`.
fn apply_flag_value(flags: &mut Flags, flag_name: &str, raw_value: &str) -> Result<(), CliError> {
    if is_empty_value_literal(raw_value) {
        return Err(CliError::with_argument(
            CliErrorKind::EmptyFlagValue,
            flag_name,
        ));
    }
    match flag_name {
        "--config" | "-c" => {
            flags.config = Some(parse_config_path(flag_name, raw_value)?);
            Ok(())
        }
        "--since" => {
            flags.since = Some(parse_log_flag_value(raw_value)?);
            Ok(())
        }
        "--tail" => {
            flags.tail = Some(parse_log_flag_value(raw_value)?);
            Ok(())
        }
        "--until" => {
            flags.until = Some(parse_log_flag_value(raw_value)?);
            Ok(())
        }
        _ => Err(CliError::with_argument(CliErrorKind::UnknownFlag, flag_name)),
    }
}

/// Consumes and validates the next CLI argument as the value for a space-separated value flag.
fn consume_next_value<I, T>(
    iterator: &mut I,
    argument_count: &mut usize,
    flag_name: &str,
) -> Result<String, CliError>
where
    I: Iterator<Item = T>,
    T: Into<OsString>,
{
    let Some(next_raw) = iterator.next() else {
        return Err(CliError::with_argument(
            CliErrorKind::MissingFlagValue,
            flag_name,
        ));
    };
    *argument_count += 1;
    if *argument_count > MAX_CLI_ARGS {
        return Err(CliError::new(CliErrorKind::LimitExceeded));
    }
    let next_os: OsString = next_raw.into();
    let value_str = os_str_to_utf8(&next_os)?;
    if is_empty_value_literal(value_str) {
        return Err(CliError::with_argument(
            CliErrorKind::EmptyFlagValue,
            flag_name,
        ));
    }
    if value_str.starts_with('-') {
        return Err(CliError::with_argument(
            CliErrorKind::MissingFlagValue,
            flag_name,
        ));
    }
    Ok(value_str.to_owned())
}

/// Verifies that `positionals` contains at most `max_count` arguments.
fn ensure_no_extra_arguments(
    positionals: &[String],
    max_count: usize,
) -> Result<(), CliError> {
    if let Some(extra) = positionals.get(max_count) {
        return Err(CliError::with_argument(
            CliErrorKind::TooManyArguments,
            extra,
        ));
    }
    Ok(())
}

/// Evaluates whether `segment` is a safe, non-empty target component without path separators, leading dashes, or control characters.
fn is_valid_target_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.starts_with('-')
        && !segment.starts_with('@')
        && !segment.contains('/')
        && !contains_control_or_whitespace(segment)
}

/// Converts an `OsStr` command-line argument to a UTF-8 `&str`.
fn os_str_to_utf8(argument: &OsStr) -> Result<&str, CliError> {
    argument
        .to_str()
        .ok_or_else(|| CliError::new(CliErrorKind::InvalidUtf8))
}

/// Parses positional arguments for `pods command exec <recipe> <command|index>`.
fn parse_command_exec(
    positionals: &[String],
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    ensure_no_extra_arguments(positionals, 3)?;
    let Some(recipe) = positionals.get(1) else {
        if bypass_targets {
            return Ok((ModeAction::Command(CommandAction::Exec), Vec::new()));
        }
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_recipe_only_target(recipe)?;
    let Some(command_name) = positionals.get(2) else {
        if bypass_targets {
            return Ok((
                ModeAction::Command(CommandAction::Exec),
                vec![recipe.clone()],
            ));
        }
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_recipe_only_target(command_name)?;
    Ok((
        ModeAction::Command(CommandAction::Exec),
        vec![recipe.clone(), command_name.clone()],
    ))
}

/// Parses implicit `pods command <recipe> [<command|index>]` arguments into `list` or `exec`.
fn parse_command_implicit(
    recipe: &str,
    positionals: &[String],
) -> Result<(ModeAction, Vec<String>), CliError> {
    validate_recipe_only_target(recipe)?;
    let Some(command_name) = positionals.get(1) else {
        return Ok((
            ModeAction::Command(CommandAction::List),
            vec![recipe.to_owned()],
        ));
    };
    ensure_no_extra_arguments(positionals, 2)?;
    validate_recipe_only_target(command_name)?;
    Ok((
        ModeAction::Command(CommandAction::Exec),
        vec![recipe.to_owned(), command_name.clone()],
    ))
}

/// Parses positional arguments for `pods command list <recipe>`.
fn parse_command_list(
    positionals: &[String],
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    ensure_no_extra_arguments(positionals, 2)?;
    let Some(recipe) = positionals.get(1) else {
        if bypass_targets {
            return Ok((ModeAction::Command(CommandAction::List), Vec::new()));
        }
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_recipe_only_target(recipe)?;
    Ok((
        ModeAction::Command(CommandAction::List),
        vec![recipe.clone()],
    ))
}

/// Parses positional arguments for `command` (`cmd`) mode.
fn parse_command_positionals(
    positionals: &[String],
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Command(CommandAction::Help), Vec::new()));
    };
    match first.as_str() {
        "exec" => parse_command_exec(positionals, bypass_targets),
        "help" => {
            ensure_no_extra_arguments(positionals, 1)?;
            Ok((ModeAction::Command(CommandAction::Help), Vec::new()))
        }
        "list" => parse_command_list(positionals, bypass_targets),
        recipe => parse_command_implicit(recipe, positionals),
    }
}

/// Validates a `--config` / `-c` file path argument and converts it to a [`PathBuf`].
fn parse_config_path(flag_name: &str, raw_value: &str) -> Result<PathBuf, CliError> {
    if is_empty_value_literal(raw_value) {
        return Err(CliError::with_argument(
            CliErrorKind::EmptyFlagValue,
            flag_name,
        ));
    }
    if extract_config_stem(raw_value).is_none() {
        return Err(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            raw_value,
        ));
    }
    Ok(PathBuf::from(raw_value))
}

/// Parses positional arguments for `config` (`cfg`) mode.
fn parse_config_positionals(positionals: &[String]) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Config(ConfigAction::Show), Vec::new()));
    };
    let action = ConfigAction::from_str(first)?;
    ensure_no_extra_arguments(positionals, 1)?;
    Ok((ModeAction::Config(action), Vec::new()))
}

/// Parses positional arguments for `connect` (`con`) mode.
fn parse_connect_positionals(
    positionals: &[String],
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Connect(ConnectAction::Help), Vec::new()));
    };
    match first.as_str() {
        "help" => {
            ensure_no_extra_arguments(positionals, 1)?;
            Ok((ModeAction::Connect(ConnectAction::Help), Vec::new()))
        }
        "shell" => parse_connect_shell(positionals, 1, bypass_targets),
        _ => parse_connect_shell(positionals, 0, bypass_targets),
    }
}

/// Extracts and validates the container target for `pods connect [shell] <target>`.
fn parse_connect_shell(
    positionals: &[String],
    target_index: usize,
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    ensure_no_extra_arguments(positionals, target_index + 1)?;
    let Some(target) = positionals.get(target_index) else {
        if bypass_targets {
            return Ok((ModeAction::Connect(ConnectAction::Shell), Vec::new()));
        }
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_container_target(target)?;
    Ok((
        ModeAction::Connect(ConnectAction::Shell),
        vec![target.clone()],
    ))
}

/// Parses positional arguments for `default` mode lifecycle and status actions.
fn parse_default_positionals(
    positionals: &[String],
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Default(DefaultAction::Help), Vec::new()));
    };
    let action = DefaultAction::from_str(first)?;
    let parameters = &positionals[1..];

    if action == DefaultAction::Help {
        ensure_no_extra_arguments(parameters, 0)?;
    }
    if action.requires_targets() && parameters.is_empty() && !bypass_targets {
        return Err(CliError::new(CliErrorKind::MissingTarget));
    }
    validate_group_or_recipe_targets(parameters)?;
    Ok((ModeAction::Default(action), parameters.to_vec()))
}

/// Parses a `--flag=value` argument token, rejecting `=value` on boolean flags.
fn parse_equals_flag(
    flags: &mut Flags,
    flag_name: &str,
    raw_value: &str,
) -> Result<(), CliError> {
    match flag_name {
        "--config" | "-c" | "--since" | "--tail" | "--until" => {
            apply_flag_value(flags, flag_name, raw_value)
        }
        "--all" | "--debug" | "--force" | "--full" | "--preview" | "--simulate"
        | "--timestamps" | "--version" | "-v" => Err(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            flag_name,
        )),
        _ => Err(CliError::with_argument(CliErrorKind::UnknownFlag, flag_name)),
    }
}

/// Dispatches a leading-dash flag token to either `--flag=value` or standalone flag parsing.
fn parse_flag_token<I, T>(
    token: &str,
    iterator: &mut I,
    argument_count: &mut usize,
    flags: &mut Flags,
) -> Result<(), CliError>
where
    I: Iterator<Item = T>,
    T: Into<OsString>,
{
    if let Some((flag_name, raw_value)) = token.split_once('=') {
        return parse_equals_flag(flags, flag_name, raw_value);
    }
    parse_global_flag(token, iterator, argument_count, flags)
}

/// Parses a standalone boolean flag or space-separated value flag into `flags`.
fn parse_global_flag<I, T>(
    flag_name: &str,
    iterator: &mut I,
    argument_count: &mut usize,
    flags: &mut Flags,
) -> Result<(), CliError>
where
    I: Iterator<Item = T>,
    T: Into<OsString>,
{
    match flag_name {
        "--all" => flags.all = true,
        "--config" | "-c" | "--since" | "--tail" | "--until" => {
            let value = consume_next_value(iterator, argument_count, flag_name)?;
            return apply_flag_value(flags, flag_name, &value);
        }
        "--debug" => flags.debug = true,
        "--force" => flags.force = true,
        "--full" => flags.full = true,
        "--preview" => flags.preview = true,
        "--simulate" => flags.simulate = true,
        "--timestamps" => flags.timestamps = true,
        "--version" | "-v" => flags.version = true,
        _ => return Err(CliError::with_argument(CliErrorKind::UnknownFlag, flag_name)),
    }
    Ok(())
}

/// Parses positional arguments for `image` (`img`) mode.
fn parse_image_positionals(
    positionals: &[String],
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Image(ImageAction::Help), Vec::new()));
    };
    let action = ImageAction::from_str(first)?;
    let parameters = &positionals[1..];

    match action {
        ImageAction::Help | ImageAction::Prune => {
            ensure_no_extra_arguments(parameters, 0)?;
            Ok((ModeAction::Image(action), Vec::new()))
        }
        ImageAction::Update => {
            if parameters.is_empty() && !bypass_targets {
                return Err(CliError::new(CliErrorKind::MissingTarget));
            }
            validate_group_or_recipe_targets(parameters)?;
            Ok((ModeAction::Image(ImageAction::Update), parameters.to_vec()))
        }
    }
}

/// Parses positional arguments for `init` mode.
fn parse_init_positionals(positionals: &[String]) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Init(InitAction::Create), Vec::new()));
    };
    let action = InitAction::from_str(first)?;
    ensure_no_extra_arguments(positionals, 1)?;
    Ok((ModeAction::Init(action), Vec::new()))
}

/// Validates a `--since`, `--tail`, or `--until` flag value, rejecting leading dashes, whitespace, and control characters.
fn parse_log_flag_value(raw_value: &str) -> Result<String, CliError> {
    if raw_value.is_empty()
        || raw_value.starts_with('-')
        || contains_control_or_whitespace(raw_value)
    {
        return Err(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            raw_value,
        ));
    }
    Ok(raw_value.to_owned())
}

/// Parses positional arguments for `logs` (`log`) mode.
fn parse_logs_positionals(
    positionals: &[String],
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Logs(LogsAction::Help), Vec::new()));
    };
    match first.as_str() {
        "follow" => parse_logs_target(LogsAction::Follow, positionals, 1, bypass_targets),
        "help" => {
            ensure_no_extra_arguments(positionals, 1)?;
            Ok((ModeAction::Logs(LogsAction::Help), Vec::new()))
        }
        "show" => parse_logs_target(LogsAction::Show, positionals, 1, bypass_targets),
        _ => parse_logs_target(LogsAction::Show, positionals, 0, bypass_targets),
    }
}

/// Extracts and validates the `<recipe>[/<container>]` target for `logs show` or `logs follow`.
fn parse_logs_target(
    action: LogsAction,
    positionals: &[String],
    target_index: usize,
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    ensure_no_extra_arguments(positionals, target_index + 1)?;
    let Some(target) = positionals.get(target_index) else {
        if bypass_targets {
            return Ok((ModeAction::Logs(action), Vec::new()));
        }
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_container_target(target)?;
    Ok((ModeAction::Logs(action), vec![target.clone()]))
}

/// Resolves the top-level [`Mode`], [`ModeAction`], and remaining target parameters from `positionals`.
fn parse_mode(
    positionals: &[String],
    bypass_targets: bool,
) -> Result<(Mode, ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((
            Mode::Default,
            ModeAction::Default(DefaultAction::Help),
            Vec::new(),
        ));
    };
    if let Ok(mode) = Mode::from_str(first) {
        let (action, parameters) = parse_mode_action(mode, &positionals[1..], bypass_targets)?;
        return Ok((mode, action, parameters));
    }
    let (action, parameters) = parse_default_positionals(positionals, bypass_targets)?;
    Ok((Mode::Default, action, parameters))
}

/// Routes positional arguments to the mode-specific action parser for `mode`.
fn parse_mode_action(
    mode: Mode,
    positionals: &[String],
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    match mode {
        Mode::Command => parse_command_positionals(positionals, bypass_targets),
        Mode::Config => parse_config_positionals(positionals),
        Mode::Connect => parse_connect_positionals(positionals, bypass_targets),
        Mode::Default => parse_default_positionals(positionals, bypass_targets),
        Mode::Image => parse_image_positionals(positionals, bypass_targets),
        Mode::Init => parse_init_positionals(positionals),
        Mode::Logs => parse_logs_positionals(positionals, bypass_targets),
        Mode::Recipe => parse_recipe_positionals(positionals, bypass_targets),
    }
}

/// Parses positional arguments for `recipe` (`rec`) mode.
fn parse_recipe_positionals(
    positionals: &[String],
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Recipe(RecipeAction::List), Vec::new()));
    };
    match first.as_str() {
        "edit" => parse_recipe_target(RecipeAction::Edit, positionals, 1, bypass_targets),
        "help" => {
            ensure_no_extra_arguments(positionals, 1)?;
            Ok((ModeAction::Recipe(RecipeAction::Help), Vec::new()))
        }
        "list" => {
            ensure_no_extra_arguments(positionals, 1)?;
            Ok((ModeAction::Recipe(RecipeAction::List), Vec::new()))
        }
        "show" => parse_recipe_target(RecipeAction::Show, positionals, 1, bypass_targets),
        _ => parse_recipe_target(RecipeAction::Show, positionals, 0, bypass_targets),
    }
}

/// Extracts and validates the `<recipe>` target for `recipe show` or `recipe edit`.
fn parse_recipe_target(
    action: RecipeAction,
    positionals: &[String],
    target_index: usize,
    bypass_targets: bool,
) -> Result<(ModeAction, Vec<String>), CliError> {
    ensure_no_extra_arguments(positionals, target_index + 1)?;
    let Some(recipe) = positionals.get(target_index) else {
        if bypass_targets {
            return Ok((ModeAction::Recipe(action), Vec::new()));
        }
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_recipe_only_target(recipe)?;
    Ok((ModeAction::Recipe(action), vec![recipe.clone()]))
}

/// Decodes a single raw `OsString` argument and routes it as either a flag or a positional argument.
fn process_single_argument<I, T>(
    raw_argument: OsString,
    iterator: &mut I,
    argument_count: &mut usize,
    flags: &mut Flags,
    positionals: &mut Vec<String>,
) -> Result<(), CliError>
where
    I: Iterator<Item = T>,
    T: Into<OsString>,
{
    let token = os_str_to_utf8(&raw_argument)?;
    if token.starts_with('-') {
        return parse_flag_token(token, iterator, argument_count, flags);
    }
    positionals.push(token.to_owned());
    Ok(())
}

/// Scans all raw CLI arguments in a single pass, populating `flags` and returning positional arguments.
fn scan_raw_arguments<I, T>(
    flags: &mut Flags,
    arguments: I,
) -> Result<Vec<String>, CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut iterator = arguments.into_iter();
    let mut positionals = Vec::with_capacity(INITIAL_POSITIONALS_CAPACITY.min(MAX_CLI_ARGS));
    let mut argument_count = 0;

    while let Some(raw_argument) = iterator.next() {
        argument_count += 1;
        if argument_count > MAX_CLI_ARGS {
            return Err(CliError::new(CliErrorKind::LimitExceeded));
        }
        process_single_argument(
            raw_argument.into(),
            &mut iterator,
            &mut argument_count,
            flags,
            &mut positionals,
        )?;
    }

    Ok(positionals)
}

/// Constructs an [`CliErrorKind::UnexpectedFlag`] error for `flag_name`.
fn unexpected_flag(flag_name: &str) -> Result<(), CliError> {
    Err(CliError::with_argument(
        CliErrorKind::UnexpectedFlag,
        flag_name,
    ))
}

/// Validates a container target (`<target>` or `<recipe>/<container>`) for `connect` and `logs` modes.
fn validate_container_target(target: &str) -> Result<(), CliError> {
    if let Some((recipe, container)) = target.split_once('/') {
        if !is_valid_target_segment(recipe) || !is_valid_target_segment(container) {
            return Err(CliError::with_argument(CliErrorKind::InvalidTarget, target));
        }
        return Ok(());
    }
    if !is_valid_target_segment(target) {
        return Err(CliError::with_argument(CliErrorKind::InvalidTarget, target));
    }
    Ok(())
}

/// Validates a single `<recipe>` or `@<group>` target argument.
fn validate_group_or_recipe_target(target: &str) -> Result<(), CliError> {
    let segment = target.strip_prefix('@').unwrap_or(target);
    if !is_valid_target_segment(segment) {
        return Err(CliError::with_argument(CliErrorKind::InvalidTarget, target));
    }
    Ok(())
}

/// Validates a slice of `<recipe>` or `@<group>` target arguments.
fn validate_group_or_recipe_targets(parameters: &[String]) -> Result<(), CliError> {
    for parameter in parameters {
        validate_group_or_recipe_target(parameter)?;
    }
    Ok(())
}

/// Verifies that all parsed mode-specific flags are permitted for `context.mode`.
fn validate_mode_flags(context: &Context) -> Result<(), CliError> {
    if context.flags.all && !matches!(context.mode, Mode::Default | Mode::Image | Mode::Recipe) {
        return unexpected_flag("--all");
    }
    if context.flags.full && context.mode != Mode::Default {
        return unexpected_flag("--full");
    }
    if context.flags.force && context.mode != Mode::Image {
        return unexpected_flag("--force");
    }
    if context.flags.preview && context.mode != Mode::Image {
        return unexpected_flag("--preview");
    }
    if context.flags.since.is_some() && context.mode != Mode::Logs {
        return unexpected_flag("--since");
    }
    if context.flags.tail.is_some() && context.mode != Mode::Logs {
        return unexpected_flag("--tail");
    }
    if context.flags.timestamps && context.mode != Mode::Logs {
        return unexpected_flag("--timestamps");
    }
    if context.flags.until.is_some() && context.mode != Mode::Logs {
        return unexpected_flag("--until");
    }
    Ok(())
}

/// Validates a single `<recipe>` or `<command|index>` target that does not allow `@group` or `/` syntax.
fn validate_recipe_only_target(target: &str) -> Result<(), CliError> {
    if !is_valid_target_segment(target) {
        return Err(CliError::with_argument(CliErrorKind::InvalidTarget, target));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::{Path, PathBuf};

    use crate::core::context::{
        CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, Flags, ImageAction,
        InitAction, LogsAction, Mode, ModeAction, RecipeAction,
    };
    use crate::error::CliErrorKind;
    use super::{
        MAX_CLI_ARGS, apply_flag_value, parse_args, resolve_config_name, resolve_config_path,
    };

    #[test]
    fn argument_count_bounds_and_invalid_utf8() {
        let mut max_arguments = Vec::with_capacity(MAX_CLI_ARGS);
        max_arguments.push(OsString::from("up"));
        for index in 1..MAX_CLI_ARGS {
            max_arguments.push(OsString::from(format!("recipe-{index}")));
        }
        let mut valid_max = Context::new();
        parse_args(&mut valid_max, max_arguments.clone()).unwrap();
        assert_eq!(valid_max.parameters.len(), MAX_CLI_ARGS - 1);

        max_arguments.push(OsString::from("overflow-recipe"));
        let mut exceeded_context = Context::new();
        let exceeded = parse_args(&mut exceeded_context, max_arguments).unwrap_err();
        assert_eq!(exceeded.kind, CliErrorKind::LimitExceeded);

        let mut flag_boundary = Vec::with_capacity(MAX_CLI_ARGS + 1);
        flag_boundary.push(OsString::from("up"));
        for index in 1..(MAX_CLI_ARGS - 1) {
            flag_boundary.push(OsString::from(format!("recipe-{index}")));
        }
        flag_boundary.push(OsString::from("--config"));
        flag_boundary.push(OsString::from("staging"));
        assert_eq!(flag_boundary.len(), MAX_CLI_ARGS + 1);
        let mut flag_exceeded_context = Context::new();
        let flag_exceeded = parse_args(&mut flag_exceeded_context, flag_boundary).unwrap_err();
        assert_eq!(flag_exceeded.kind, CliErrorKind::LimitExceeded);

        let invalid_utf8 = OsString::from_vec(vec![b'u', b'p', 0xff, 0xfe]);
        let mut utf8_context = Context::new();
        let utf8_error = parse_args(&mut utf8_context, [invalid_utf8]).unwrap_err();
        assert_eq!(utf8_error.kind, CliErrorKind::InvalidUtf8);
    }

    fn assert_invalid_config_path(invalid_config: &str) {
        let mut equals_context = Context::new();
        let equals_flag = format!("--config={invalid_config}");
        let equals_error =
            parse_args(&mut equals_context, [equals_flag.as_str(), "status"]).unwrap_err();
        assert_eq!(
            equals_error.kind,
            CliErrorKind::InvalidFlagValue,
            "expected InvalidFlagValue for {equals_flag:?}"
        );

        if !invalid_config.starts_with('-') {
            let mut separated_context = Context::new();
            let separated_error =
                parse_args(&mut separated_context, ["--config", invalid_config, "status"])
                    .unwrap_err();
            assert_eq!(
                separated_error.kind,
                CliErrorKind::InvalidFlagValue,
                "expected InvalidFlagValue for --config {invalid_config:?}"
            );
        }
    }

    #[test]
    fn config_path_edge_cases_and_target_validation() {
        assert_eq!(resolve_config_name(None), "pods");
        assert_eq!(resolve_config_path(None), PathBuf::from("./pods.conf"));
        assert_eq!(resolve_config_name(Some(Path::new("staging"))), "staging");
        assert_eq!(
            resolve_config_path(Some(Path::new("staging"))),
            PathBuf::from("./staging.conf")
        );
        assert_eq!(resolve_config_name(Some(Path::new(".hidden"))), ".hidden");
        assert_eq!(
            resolve_config_path(Some(Path::new(".hidden"))),
            PathBuf::from("./.hidden.conf")
        );
        assert_eq!(
            resolve_config_name(Some(Path::new("./.hidden.conf"))),
            ".hidden"
        );
        assert_eq!(
            resolve_config_path(Some(Path::new("./.hidden.conf"))),
            PathBuf::from("./.hidden.conf")
        );
        assert_eq!(
            resolve_config_name(Some(Path::new("../my.multi.ext"))),
            "my.multi.ext"
        );
        assert_eq!(
            resolve_config_path(Some(Path::new("../my.multi.ext"))),
            PathBuf::from("../my.multi.ext.conf")
        );
        assert_eq!(
            resolve_config_name(Some(Path::new("/etc/pods/my.multi.ext.conf"))),
            "my.multi.ext"
        );
        assert_eq!(
            resolve_config_path(Some(Path::new("/etc/pods/my.multi.ext.conf"))),
            PathBuf::from("/etc/pods/my.multi.ext.conf")
        );

        let mut hidden_context = Context::new();
        parse_args(&mut hidden_context, ["--config=.hidden", "status"]).unwrap();
        assert_eq!(hidden_context.config.name, ".hidden");
        assert_eq!(
            hidden_context.config.path,
            PathBuf::from("./.hidden.conf")
        );

        for invalid_path in [".", "..", "/", "./", "../", "configs/", "   "] {
            let mut context = Context::new();
            let error = parse_args(&mut context, ["--config", invalid_path, "status"]).unwrap_err();
            assert_eq!(error.kind, CliErrorKind::InvalidFlagValue);
        }

        let mut context = Context::new();
        let group_in_connect = parse_args(&mut context, ["connect", "@backend"]).unwrap_err();
        assert_eq!(group_in_connect.kind, CliErrorKind::InvalidTarget);

        let group_with_container = parse_args(&mut context, ["up", "@backend/db"]).unwrap_err();
        assert_eq!(group_with_container.kind, CliErrorKind::InvalidTarget);

        let slash_in_recipe = parse_args(&mut context, ["recipe", "show", "app/db"]).unwrap_err();
        assert_eq!(slash_in_recipe.kind, CliErrorKind::InvalidTarget);

        let too_many_config = parse_args(&mut context, ["config", "show", "extra"]).unwrap_err();
        assert_eq!(too_many_config.kind, CliErrorKind::TooManyArguments);
    }

    #[test]
    fn default_lifecycle_actions_and_targets() {
        let lifecycle_actions = [
            ("create", DefaultAction::Create),
            ("down", DefaultAction::Down),
            ("reload", DefaultAction::Reload),
            ("remove", DefaultAction::Remove),
            ("restart", DefaultAction::Restart),
            ("start", DefaultAction::Start),
            ("stop", DefaultAction::Stop),
            ("up", DefaultAction::Up),
            ("update", DefaultAction::Update),
        ];

        for (keyword, expected_action) in lifecycle_actions {
            let mut context = Context::new();
            parse_args(&mut context, [keyword, "example-app", "@backend"]).unwrap();
            assert_eq!(context.mode, Mode::Default);
            assert_eq!(context.action, ModeAction::Default(expected_action));
            assert_eq!(context.default_action(), Some(expected_action));
            assert_eq!(context.parameters, vec!["example-app", "@backend"]);
            assert!(context.requires_config());

            let mut missing_context = Context::new();
            let missing_target = parse_args(&mut missing_context, [keyword]).unwrap_err();
            assert_eq!(missing_target.kind, CliErrorKind::MissingTarget);
        }

        let mut status_default = Context::new();
        parse_args(&mut status_default, ["status", "--all", "--full"]).unwrap();
        assert_eq!(status_default.mode, Mode::Default);
        assert_eq!(
            status_default.action,
            ModeAction::Default(DefaultAction::Status)
        );
        assert!(status_default.flags.all);
        assert!(status_default.flags.full);
        assert!(status_default.requires_config());

        let mut empty_invocation = Context::new();
        parse_args(&mut empty_invocation, Vec::<String>::new()).unwrap();
        assert_eq!(empty_invocation, Context::new());
        assert_eq!(empty_invocation.flags, Flags::new());
        assert_eq!(empty_invocation.mode, Mode::Default);
        assert_eq!(
            empty_invocation.action,
            ModeAction::Default(DefaultAction::Help)
        );
        assert!(!empty_invocation.requires_config());
    }

    #[test]
    fn flags_equals_and_separated_and_rejections() {
        let mut logs_equals = Context::new();
        parse_args(
            &mut logs_equals,
            [
                "--config=staging",
                "--debug",
                "logs",
                "--tail=100",
                "--since=1h",
                "--until=10m",
                "--timestamps",
                "show",
                "example-app/1",
                "--simulate",
            ],
        )
        .unwrap();
        let mut logs_separated = Context::new();
        parse_args(
            &mut logs_separated,
            [
                "-c",
                "staging",
                "--debug",
                "log",
                "--tail",
                "100",
                "--since",
                "1h",
                "--until",
                "10m",
                "--timestamps",
                "show",
                "example-app/1",
                "--simulate",
            ],
        )
        .unwrap();
        assert_eq!(logs_equals, logs_separated);
        assert_eq!(logs_equals.flags.config, Some(PathBuf::from("staging")));
        assert_eq!(logs_equals.config.name, "staging");
        assert_eq!(logs_equals.config.path, PathBuf::from("./staging.conf"));
        assert!(logs_equals.flags.debug);
        assert!(logs_equals.flags.simulate);
        assert!(logs_equals.config.simulate);
        assert_eq!(logs_equals.flags.tail.as_deref(), Some("100"));
        assert_eq!(logs_equals.flags.since.as_deref(), Some("1h"));
        assert_eq!(logs_equals.flags.until.as_deref(), Some("10m"));
        assert!(logs_equals.flags.timestamps);
        assert_eq!(logs_equals.parameters, vec!["example-app/1"]);

        let mut version_short = Context::new();
        parse_args(&mut version_short, ["-v"]).unwrap();
        assert!(version_short.flags.version);
        assert!(!version_short.requires_config());

        for empty_arg in [
            "--config=",
            "--config=\"\"",
            "--config=''",
            "--tail=",
            "--tail=\"\"",
        ] {
            let mut context = Context::new();
            let error = parse_args(&mut context, ["logs", empty_arg, "example-app"]).unwrap_err();
            assert_eq!(error.kind, CliErrorKind::EmptyFlagValue);
        }

        let mut context = Context::new();
        let empty_separated =
            parse_args(&mut context, ["--config", "", "up", "example-app"]).unwrap_err();
        assert_eq!(empty_separated.kind, CliErrorKind::EmptyFlagValue);

        for missing_flag in ["--config", "-c", "--tail", "--since", "--until"] {
            let mut missing_context = Context::new();
            let error =
                parse_args(&mut missing_context, ["logs", "example-app", missing_flag]).unwrap_err();
            assert_eq!(error.kind, CliErrorKind::MissingFlagValue);
        }

        for unknown_flag in ["--unknown", "--foo=bar", "--help", "-h", "-x", "-"] {
            let mut unknown_context = Context::new();
            let error = parse_args(&mut unknown_context, [unknown_flag]).unwrap_err();
            assert_eq!(error.kind, CliErrorKind::UnknownFlag);
        }

        for (mode_arg, bad_flag) in [
            ("logs", "--all"),
            ("recipe", "--full"),
            ("status", "--force"),
            ("status", "--preview"),
            ("status", "--since=1h"),
            ("status", "--tail=10"),
            ("status", "--timestamps"),
            ("status", "--until=10m"),
        ] {
            let mut bad_context = Context::new();
            let error = parse_args(&mut bad_context, [mode_arg, bad_flag]).unwrap_err();
            assert_eq!(error.kind, CliErrorKind::UnexpectedFlag);
        }

        let mut bool_context = Context::new();
        let boolean_with_equals =
            parse_args(&mut bool_context, ["--debug=true", "status"]).unwrap_err();
        assert_eq!(boolean_with_equals.kind, CliErrorKind::InvalidFlagValue);

        let fallback_error = apply_flag_value(&mut Flags::new(), "--unknown", "value").unwrap_err();
        assert_eq!(fallback_error.kind, CliErrorKind::UnknownFlag);
    }

    #[test]
    fn modes_and_three_letter_aliases_and_reject_ini() {
        let mut cmd_exec = Context::new();
        parse_args(&mut cmd_exec, ["cmd", "exec", "example-app", "migrate"]).unwrap();
        let mut command_exec = Context::new();
        parse_args(
            &mut command_exec,
            ["command", "exec", "example-app", "migrate"],
        )
        .unwrap();
        assert_eq!(cmd_exec, command_exec);
        assert_eq!(cmd_exec.mode, Mode::Command);
        assert_eq!(cmd_exec.action, ModeAction::Command(CommandAction::Exec));
        assert_eq!(cmd_exec.default_action(), None);
        assert_eq!(cmd_exec.parameters, vec!["example-app", "migrate"]);

        let mut cmd_implicit_list = Context::new();
        parse_args(&mut cmd_implicit_list, ["cmd", "example-app"]).unwrap();
        assert_eq!(
            cmd_implicit_list.action,
            ModeAction::Command(CommandAction::List)
        );
        assert_eq!(cmd_implicit_list.parameters, vec!["example-app"]);

        let mut cmd_implicit_exec = Context::new();
        parse_args(&mut cmd_implicit_exec, ["command", "example-app", "1"]).unwrap();
        assert_eq!(
            cmd_implicit_exec.action,
            ModeAction::Command(CommandAction::Exec)
        );
        assert_eq!(cmd_implicit_exec.parameters, vec!["example-app", "1"]);

        let mut cfg_show = Context::new();
        parse_args(&mut cfg_show, ["cfg"]).unwrap();
        let mut config_show = Context::new();
        parse_args(&mut config_show, ["config", "show"]).unwrap();
        assert_eq!(cfg_show, config_show);
        assert_eq!(cfg_show.mode, Mode::Config);
        assert_eq!(cfg_show.action, ModeAction::Config(ConfigAction::Show));

        let mut cfg_edit = Context::new();
        parse_args(&mut cfg_edit, ["cfg", "edit"]).unwrap();
        assert_eq!(cfg_edit.action, ModeAction::Config(ConfigAction::Edit));

        let mut con_shell = Context::new();
        parse_args(&mut con_shell, ["con", "example-app/db"]).unwrap();
        let mut connect_shell = Context::new();
        parse_args(&mut connect_shell, ["connect", "shell", "example-app/db"]).unwrap();
        assert_eq!(con_shell, connect_shell);
        assert_eq!(con_shell.mode, Mode::Connect);
        assert_eq!(con_shell.action, ModeAction::Connect(ConnectAction::Shell));

        let mut img_prune = Context::new();
        parse_args(
            &mut img_prune,
            ["img", "prune", "--all", "--force", "--preview"],
        )
        .unwrap();
        let mut image_prune = Context::new();
        parse_args(
            &mut image_prune,
            ["image", "prune", "--all", "--force", "--preview"],
        )
        .unwrap();
        assert_eq!(img_prune, image_prune);
        assert_eq!(img_prune.mode, Mode::Image);
        assert_eq!(img_prune.action, ModeAction::Image(ImageAction::Prune));
        assert!(!img_prune.requires_config());

        let mut img_update = Context::new();
        parse_args(&mut img_update, ["img", "update", "@backend"]).unwrap();
        assert_eq!(img_update.action, ModeAction::Image(ImageAction::Update));
        assert!(img_update.requires_config());

        let mut init_create = Context::new();
        parse_args(&mut init_create, ["init"]).unwrap();
        assert_eq!(init_create.mode, Mode::Init);
        assert_eq!(init_create.action, ModeAction::Init(InitAction::Create));
        assert!(!init_create.requires_config());

        let mut init_help = Context::new();
        parse_args(&mut init_help, ["init", "help"]).unwrap();
        assert_eq!(init_help.action, ModeAction::Init(InitAction::Help));

        let mut ini_context = Context::new();
        let ini_error = parse_args(&mut ini_context, ["ini"]).unwrap_err();
        assert_eq!(ini_error.kind, CliErrorKind::UnknownAction);
        assert_eq!(ini_error.argument.as_deref(), Some("ini"));

        let mut log_follow = Context::new();
        parse_args(&mut log_follow, ["log", "follow", "example-app/db"]).unwrap();
        let mut logs_follow = Context::new();
        parse_args(&mut logs_follow, ["logs", "follow", "example-app/db"]).unwrap();
        assert_eq!(log_follow, logs_follow);
        assert_eq!(log_follow.mode, Mode::Logs);
        assert_eq!(log_follow.action, ModeAction::Logs(LogsAction::Follow));

        let mut log_implicit_show = Context::new();
        parse_args(&mut log_implicit_show, ["log", "example-app"]).unwrap();
        assert_eq!(log_implicit_show.action, ModeAction::Logs(LogsAction::Show));

        let mut rec_list = Context::new();
        parse_args(&mut rec_list, ["rec", "--all"]).unwrap();
        let mut recipe_list = Context::new();
        parse_args(&mut recipe_list, ["recipe", "list", "--all"]).unwrap();
        assert_eq!(rec_list, recipe_list);
        assert_eq!(rec_list.mode, Mode::Recipe);
        assert_eq!(rec_list.action, ModeAction::Recipe(RecipeAction::List));

        let mut rec_implicit_show = Context::new();
        parse_args(&mut rec_implicit_show, ["rec", "example-app"]).unwrap();
        assert_eq!(
            rec_implicit_show.action,
            ModeAction::Recipe(RecipeAction::Show)
        );

        let mut rec_edit = Context::new();
        parse_args(&mut rec_edit, ["recipe", "edit", "example-app"]).unwrap();
        assert_eq!(rec_edit.action, ModeAction::Recipe(RecipeAction::Edit));
    }

    #[test]
    fn red_team_config_path_and_flag_smuggling_exploits() {
        for smuggled_flag in [
            ["--config", "--simulate", "down", "example-app"].as_slice(),
            ["-c", "--debug", "up", "example-app"].as_slice(),
            ["logs", "--tail", "--simulate", "example-app"].as_slice(),
            ["logs", "--since", "--all", "example-app"].as_slice(),
            ["logs", "--until", "--timestamps", "example-app"].as_slice(),
        ] {
            let mut context = Context::new();
            let error = parse_args(&mut context, smuggled_flag).unwrap_err();
            assert_eq!(error.kind, CliErrorKind::MissingFlagValue);
        }

        for invalid_log_flag in [
            "--tail=--follow",
            "--tail=-10",
            "--tail=   ",
            "--tail=10\0",
            "--tail=10\n",
            "--since=--all",
            "--since=-f",
            "--since=\t",
            "--since=1h\0",
            "--until=--help",
            "--until=10m\r",
        ] {
            let mut context = Context::new();
            let error =
                parse_args(&mut context, ["logs", invalid_log_flag, "example-app"]).unwrap_err();
            assert_eq!(error.kind, CliErrorKind::InvalidFlagValue);
        }

        for invalid_config in [
            "./.",
            "./..",
            "../..",
            "/.",
            "/..",
            "configs/.",
            "configs/..",
            ".conf",
            "./.conf",
            "/etc/.conf",
            "..conf",
            "...conf",
            "-rf",
            "--simulate",
            "configs/-rf",
            " staging",
            "staging ",
            " /etc/pods",
            "staging\0",
            "staging\n",
            "staging\t",
        ] {
            assert_invalid_config_path(invalid_config);
        }

        let invalid_utf8_os = OsString::from_vec(vec![0xff, 0xfe]);
        let invalid_utf8_path = Path::new(&invalid_utf8_os);
        assert_eq!(resolve_config_name(Some(invalid_utf8_path)), "pods");
        assert_eq!(
            resolve_config_path(Some(invalid_utf8_path)),
            PathBuf::from("./pods.conf")
        );
        assert_eq!(resolve_config_name(Some(Path::new("./.."))), "pods");
        assert_eq!(
            resolve_config_path(Some(Path::new("./.."))),
            PathBuf::from("./pods.conf")
        );
        assert_eq!(
            resolve_config_path(Some(Path::new(".conf"))),
            PathBuf::from("./pods.conf")
        );
    }

    #[test]
    fn red_team_target_injection_and_bypass_exploits() {
        for invalid_target in [
            ".",
            "..",
            "@.",
            "@..",
            "@--all",
            "@-rf",
            "@@backend",
            "app\0evil",
            "app\nevil",
            "app name",
            " app",
        ] {
            let mut context = Context::new();
            let error = parse_args(&mut context, ["up", invalid_target]).unwrap_err();
            assert_eq!(
                error.kind,
                CliErrorKind::InvalidTarget,
                "expected InvalidTarget for default target {invalid_target:?}"
            );
        }

        for invalid_container in [
            ".",
            "..",
            "./db",
            "../db",
            "../..",
            "app/.",
            "app/..",
            "app/--privileged",
            "app/-f",
            "app/@db",
            "app/db\0",
            "app/db\n",
            "app/db name",
        ] {
            let mut con_context = Context::new();
            let con_error =
                parse_args(&mut con_context, ["connect", invalid_container]).unwrap_err();
            assert_eq!(
                con_error.kind,
                CliErrorKind::InvalidTarget,
                "expected InvalidTarget for connect target {invalid_container:?}"
            );

            let mut log_context = Context::new();
            let log_error = parse_args(&mut log_context, ["logs", invalid_container]).unwrap_err();
            assert_eq!(
                log_error.kind,
                CliErrorKind::InvalidTarget,
                "expected InvalidTarget for logs target {invalid_container:?}"
            );
        }

        for invalid_recipe in [".", "..", "app\0", "app\n", "app name"] {
            let mut rec_context = Context::new();
            let rec_error =
                parse_args(&mut rec_context, ["recipe", "edit", invalid_recipe]).unwrap_err();
            assert_eq!(rec_error.kind, CliErrorKind::InvalidTarget);
        }

        for invalid_command in [".", "..", "cmd/1", "migrate\0", "migrate\n", "migrate now"] {
            let mut cmd_context = Context::new();
            let cmd_error = parse_args(
                &mut cmd_context,
                ["command", "exec", "example-app", invalid_command],
            )
            .unwrap_err();
            assert_eq!(cmd_error.kind, CliErrorKind::InvalidTarget);
        }

        let mut help_extra = Context::new();
        let extra_error = parse_args(&mut help_extra, ["help", "../../etc/passwd"]).unwrap_err();
        assert_eq!(extra_error.kind, CliErrorKind::TooManyArguments);

        let mut status_help_flag = Context::new();
        let status_flag_err =
            parse_args(&mut status_help_flag, ["status", "--help"]).unwrap_err();
        assert_eq!(status_flag_err.kind, CliErrorKind::UnknownFlag);

        for valid_bypass in [
            ["command", "exec", "--version"].as_slice(),
            ["command", "exec", "example-app", "--version"].as_slice(),
            ["command", "list", "--version"].as_slice(),
            ["connect", "shell", "--version"].as_slice(),
            ["logs", "show", "--version"].as_slice(),
            ["logs", "follow", "--version"].as_slice(),
            ["recipe", "show", "--version"].as_slice(),
            ["recipe", "edit", "--version"].as_slice(),
        ] {
            let mut context = Context::new();
            parse_args(&mut context, valid_bypass).unwrap();
            assert!(!context.requires_config());
        }

        let mut bad_cfg_version = Context::new();
        let cfg_err =
            parse_args(&mut bad_cfg_version, ["config", "bogus", "--version"]).unwrap_err();
        assert_eq!(cfg_err.kind, CliErrorKind::UnknownAction);

        let mut bad_init_version = Context::new();
        let init_err =
            parse_args(&mut bad_init_version, ["init", "bogus", "--version"]).unwrap_err();
        assert_eq!(init_err.kind, CliErrorKind::UnknownAction);
    }
}
