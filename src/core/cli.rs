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

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::core::context::{
    CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, Flags, ImageAction,
    InitAction, LogsAction, Mode, ModeAction, RecipeAction,
};
use crate::error::{CliError, CliErrorKind};
use crate::utilities::path::{
    extract_config_stem, has_parent_dir, resolve_config_name, resolve_config_path,
};
use crate::utilities::string::{is_empty_value_literal, is_valid_target_segment, is_valid_token};

/// Initial capacity pre-allocated for positional CLI arguments.
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
    let (mode, action, parameters) = parse_mode(positionals)?;
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

/// Validates and stores a value-taking CLI flag (`--config`, `--since`, `--tail`, `--until`) into `flags`.
fn apply_flag_value(
    flags: &mut Flags,
    flag_name: &str,
    raw_value: Cow<'_, str>,
) -> Result<(), CliError> {
    if is_empty_value_literal(&raw_value) {
        return Err(CliError::with_argument(
            CliErrorKind::EmptyFlagValue,
            flag_name,
        ));
    }
    match flag_name {
        "--config" => {
            flags.config = Some(parse_config_path(raw_value)?);
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
    let value = os_string_to_string(next_raw.into())?;
    if value.starts_with('-') {
        return Err(CliError::with_argument(
            CliErrorKind::MissingFlagValue,
            flag_name,
        ));
    }
    Ok(value)
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

/// Converts an owned `OsString` command-line argument to an owned UTF-8 `String` without reallocating.
fn os_string_to_string(argument: OsString) -> Result<String, CliError> {
    argument
        .into_string()
        .map_err(|_| CliError::new(CliErrorKind::InvalidUtf8))
}

/// Parses positional arguments for `pods command exec <recipe> <command|index>`.
fn parse_command_exec(mut positionals: Vec<String>) -> Result<(ModeAction, Vec<String>), CliError> {
    ensure_no_extra_arguments(&positionals, 3)?;
    let Some(recipe) = positionals.get(1) else {
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_recipe_only_target(recipe)?;
    let Some(command_name) = positionals.get(2) else {
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_recipe_only_target(command_name)?;
    positionals.remove(0);
    Ok((ModeAction::Command(CommandAction::Exec), positionals))
}

/// Parses implicit `pods command <recipe> [<command|index>]` arguments into `list` or `exec`.
fn parse_command_implicit(
    positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(recipe) = positionals.first() else {
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_recipe_only_target(recipe)?;
    let Some(command_name) = positionals.get(1) else {
        return Ok((ModeAction::Command(CommandAction::List), positionals));
    };
    ensure_no_extra_arguments(&positionals, 2)?;
    validate_recipe_only_target(command_name)?;
    Ok((ModeAction::Command(CommandAction::Exec), positionals))
}

/// Parses positional arguments for `pods command list <recipe>`.
fn parse_command_list(mut positionals: Vec<String>) -> Result<(ModeAction, Vec<String>), CliError> {
    ensure_no_extra_arguments(&positionals, 2)?;
    let Some(recipe) = positionals.get(1) else {
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_recipe_only_target(recipe)?;
    positionals.remove(0);
    Ok((ModeAction::Command(CommandAction::List), positionals))
}

/// Parses positional arguments for `command` (`cmd`) mode.
fn parse_command_positionals(
    positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Command(CommandAction::Help), Vec::new()));
    };
    match first.as_str() {
        "exec" => parse_command_exec(positionals),
        "help" => {
            ensure_no_extra_arguments(&positionals, 1)?;
            Ok((ModeAction::Command(CommandAction::Help), Vec::new()))
        }
        "list" => parse_command_list(positionals),
        _ => parse_command_implicit(positionals),
    }
}

/// Validates a `--config` file path argument and converts it to a [`PathBuf`].
fn parse_config_path(raw_value: Cow<'_, str>) -> Result<PathBuf, CliError> {
    if has_parent_dir(Path::new(raw_value.as_ref()))
        || extract_config_stem(raw_value.as_ref()).is_none()
    {
        return Err(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            raw_value.as_ref(),
        ));
    }
    Ok(PathBuf::from(raw_value.into_owned()))
}

/// Parses positional arguments for `config` (`cfg`) mode.
fn parse_config_positionals(
    positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Config(ConfigAction::Show), Vec::new()));
    };
    let action = ConfigAction::from_str(first)?;
    ensure_no_extra_arguments(&positionals, 1)?;
    Ok((ModeAction::Config(action), Vec::new()))
}

/// Parses positional arguments for `connect` (`con`) mode.
fn parse_connect_positionals(
    positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Connect(ConnectAction::Help), Vec::new()));
    };
    match first.as_str() {
        "help" => {
            ensure_no_extra_arguments(&positionals, 1)?;
            Ok((ModeAction::Connect(ConnectAction::Help), Vec::new()))
        }
        "shell" => parse_connect_shell(positionals, 1),
        _ => parse_connect_shell(positionals, 0),
    }
}

/// Extracts and validates the container target for `pods connect [shell] <target>`.
fn parse_connect_shell(
    mut positionals: Vec<String>,
    target_index: usize,
) -> Result<(ModeAction, Vec<String>), CliError> {
    ensure_no_extra_arguments(&positionals, target_index + 1)?;
    let Some(target) = positionals.get(target_index) else {
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_container_target(target)?;
    if target_index == 1 {
        positionals.remove(0);
    }
    Ok((ModeAction::Connect(ConnectAction::Shell), positionals))
}

/// Parses positional arguments for `default` mode lifecycle, status, help, and version actions.
fn parse_default_positionals(
    mut positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Default(DefaultAction::Help), Vec::new()));
    };
    let action = DefaultAction::from_str(first)?;
    positionals.remove(0);

    if matches!(action, DefaultAction::Help | DefaultAction::Version) {
        ensure_no_extra_arguments(&positionals, 0)?;
    }
    if action.requires_targets() && positionals.is_empty() {
        return Err(CliError::new(CliErrorKind::MissingTarget));
    }
    validate_group_or_recipe_targets(&positionals)?;
    Ok((ModeAction::Default(action), positionals))
}

/// Parses a `--flag=value` argument token, rejecting `=value` on boolean flags.
fn parse_equals_flag(
    flags: &mut Flags,
    flag_name: &str,
    raw_value: &str,
) -> Result<(), CliError> {
    match flag_name {
        "--config" | "--since" | "--tail" | "--until" => {
            apply_flag_value(flags, flag_name, Cow::Borrowed(raw_value))
        }
        "--all" | "--debug" | "--force" | "--full" | "--preview" | "--simulate"
        | "--timestamps" => Err(CliError::with_argument(
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
        "--config" | "--since" | "--tail" | "--until" => {
            let value = consume_next_value(iterator, argument_count, flag_name)?;
            return apply_flag_value(flags, flag_name, Cow::Owned(value));
        }
        "--debug" => flags.debug = true,
        "--force" => flags.force = true,
        "--full" => flags.full = true,
        "--preview" => flags.preview = true,
        "--simulate" => flags.simulate = true,
        "--timestamps" => flags.timestamps = true,
        _ => return Err(CliError::with_argument(CliErrorKind::UnknownFlag, flag_name)),
    }
    Ok(())
}

/// Parses positional arguments for `image` (`img`) mode.
fn parse_image_positionals(
    mut positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Image(ImageAction::Help), Vec::new()));
    };
    let action = ImageAction::from_str(first)?;
    positionals.remove(0);

    match action {
        ImageAction::Help | ImageAction::Prune => {
            ensure_no_extra_arguments(&positionals, 0)?;
            Ok((ModeAction::Image(action), Vec::new()))
        }
        ImageAction::Update => {
            if positionals.is_empty() {
                return Err(CliError::new(CliErrorKind::MissingTarget));
            }
            validate_group_or_recipe_targets(&positionals)?;
            Ok((ModeAction::Image(ImageAction::Update), positionals))
        }
    }
}

/// Parses positional arguments for `init` mode.
fn parse_init_positionals(
    positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Init(InitAction::Create), Vec::new()));
    };
    let action = InitAction::from_str(first)?;
    ensure_no_extra_arguments(&positionals, 1)?;
    Ok((ModeAction::Init(action), Vec::new()))
}

/// Validates a `--since`, `--tail`, or `--until` flag value, rejecting leading dashes, whitespace, and control characters.
fn parse_log_flag_value(raw_value: Cow<'_, str>) -> Result<String, CliError> {
    if !is_valid_token(raw_value.as_ref()) {
        return Err(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            raw_value.as_ref(),
        ));
    }
    Ok(raw_value.into_owned())
}

/// Parses positional arguments for `logs` (`log`) mode.
fn parse_logs_positionals(
    positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Logs(LogsAction::Help), Vec::new()));
    };
    match first.as_str() {
        "follow" => parse_logs_target(LogsAction::Follow, positionals, 1),
        "help" => {
            ensure_no_extra_arguments(&positionals, 1)?;
            Ok((ModeAction::Logs(LogsAction::Help), Vec::new()))
        }
        "show" => parse_logs_target(LogsAction::Show, positionals, 1),
        _ => parse_logs_target(LogsAction::Show, positionals, 0),
    }
}

/// Extracts and validates the `<recipe>[/<container>]` target for `logs show` or `logs follow`.
fn parse_logs_target(
    action: LogsAction,
    mut positionals: Vec<String>,
    target_index: usize,
) -> Result<(ModeAction, Vec<String>), CliError> {
    ensure_no_extra_arguments(&positionals, target_index + 1)?;
    let Some(target) = positionals.get(target_index) else {
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_container_target(target)?;
    if target_index == 1 {
        positionals.remove(0);
    }
    Ok((ModeAction::Logs(action), positionals))
}

/// Resolves the top-level [`Mode`], [`ModeAction`], and remaining target parameters from `positionals`.
fn parse_mode(mut positionals: Vec<String>) -> Result<(Mode, ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((
            Mode::Default,
            ModeAction::Default(DefaultAction::Help),
            Vec::new(),
        ));
    };
    if let Ok(mode) = Mode::from_str(first) {
        positionals.remove(0);
        let (action, parameters) = parse_mode_action(mode, positionals)?;
        return Ok((mode, action, parameters));
    }
    let (action, parameters) = parse_default_positionals(positionals)?;
    Ok((Mode::Default, action, parameters))
}

/// Routes positional arguments to the mode-specific action parser for `mode`.
fn parse_mode_action(
    mode: Mode,
    positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    match mode {
        Mode::Command => parse_command_positionals(positionals),
        Mode::Config => parse_config_positionals(positionals),
        Mode::Connect => parse_connect_positionals(positionals),
        Mode::Default => parse_default_positionals(positionals),
        Mode::Image => parse_image_positionals(positionals),
        Mode::Init => parse_init_positionals(positionals),
        Mode::Logs => parse_logs_positionals(positionals),
        Mode::Recipe => parse_recipe_positionals(positionals),
    }
}

/// Parses positional arguments for `recipe` (`rec`) mode.
fn parse_recipe_positionals(
    positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Recipe(RecipeAction::List), Vec::new()));
    };
    match first.as_str() {
        "edit" => parse_recipe_target(RecipeAction::Edit, positionals, 1),
        "help" => {
            ensure_no_extra_arguments(&positionals, 1)?;
            Ok((ModeAction::Recipe(RecipeAction::Help), Vec::new()))
        }
        "list" => {
            ensure_no_extra_arguments(&positionals, 1)?;
            Ok((ModeAction::Recipe(RecipeAction::List), Vec::new()))
        }
        "show" => parse_recipe_target(RecipeAction::Show, positionals, 1),
        _ => parse_recipe_target(RecipeAction::Show, positionals, 0),
    }
}

/// Extracts and validates the `<recipe>` target for `recipe show` or `recipe edit`.
fn parse_recipe_target(
    action: RecipeAction,
    mut positionals: Vec<String>,
    target_index: usize,
) -> Result<(ModeAction, Vec<String>), CliError> {
    ensure_no_extra_arguments(&positionals, target_index + 1)?;
    let Some(recipe) = positionals.get(target_index) else {
        return Err(CliError::new(CliErrorKind::MissingTarget));
    };
    validate_recipe_only_target(recipe)?;
    if target_index == 1 {
        positionals.remove(0);
    }
    Ok((ModeAction::Recipe(action), positionals))
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
    let token = os_string_to_string(raw_argument)?;
    if token.starts_with('-') {
        return parse_flag_token(&token, iterator, argument_count, flags);
    }
    positionals.push(token);
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
    if context.flags.config.is_some() && context.mode == Mode::Init {
        return unexpected_flag("--config");
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
    use std::borrow::Cow;
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::{Path, PathBuf};

    use crate::core::context::{
        CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, Flags, ImageAction,
        InitAction, LogsAction, Mode, ModeAction, RecipeAction,
    };
    use crate::error::CliErrorKind;
    use crate::utilities::path::{resolve_config_name, resolve_config_path};
    use super::{
        MAX_CLI_ARGS, apply_flag_value, parse_args, parse_command_implicit, parse_mode_action,
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

        let invalid_flag_utf8 = OsString::from_vec(vec![0xff, 0xfe]);
        let mut flag_utf8_context = Context::new();
        let flag_utf8_error = parse_args(
            &mut flag_utf8_context,
            [OsString::from("--config"), invalid_flag_utf8, OsString::from("status")],
        )
        .unwrap_err();
        assert_eq!(flag_utf8_error.kind, CliErrorKind::InvalidUtf8);
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
                "--config",
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

        let mut version_action = Context::new();
        parse_args(&mut version_action, ["version"]).unwrap();
        assert_eq!(version_action.mode, Mode::Default);
        assert_eq!(
            version_action.action,
            ModeAction::Default(DefaultAction::Version)
        );
        assert!(!version_action.requires_config());

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

        for missing_flag in ["--config", "--tail", "--since", "--until"] {
            let mut missing_context = Context::new();
            let error =
                parse_args(&mut missing_context, ["logs", "example-app", missing_flag]).unwrap_err();
            assert_eq!(error.kind, CliErrorKind::MissingFlagValue);
        }

        for unknown_flag in [
            "--unknown",
            "--foo=bar",
            "--help",
            "-c",
            "-c=staging",
            "-h",
            "--version",
            "-v",
            "-x",
            "-",
        ] {
            let mut unknown_context = Context::new();
            let error = parse_args(&mut unknown_context, [unknown_flag]).unwrap_err();
            assert_eq!(error.kind, CliErrorKind::UnknownFlag);
        }

        for (mode_arg, bad_flag) in [
            ("init", "--config=staging"),
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

        let fallback_error =
            apply_flag_value(&mut Flags::new(), "--unknown", Cow::Borrowed("value")).unwrap_err();
        assert_eq!(fallback_error.kind, CliErrorKind::UnknownFlag);

        let (default_mode_action, default_parameters) =
            parse_mode_action(Mode::Default, vec![String::from("status")]).unwrap();
        assert_eq!(
            default_mode_action,
            ModeAction::Default(DefaultAction::Status)
        );
        assert!(default_parameters.is_empty());

        let implicit_empty = parse_command_implicit(Vec::new()).unwrap_err();
        assert_eq!(implicit_empty.kind, CliErrorKind::MissingTarget);
    }

    #[test]
    fn modes_and_three_letter_aliases_and_reject_ini() {
        let mut command_alias_exec = Context::new();
        parse_args(
            &mut command_alias_exec,
            ["cmd", "exec", "example-app", "migrate"],
        )
        .unwrap();
        let mut command_exec = Context::new();
        parse_args(
            &mut command_exec,
            ["command", "exec", "example-app", "migrate"],
        )
        .unwrap();
        assert_eq!(command_alias_exec, command_exec);
        assert_eq!(command_alias_exec.mode, Mode::Command);
        assert_eq!(
            command_alias_exec.action,
            ModeAction::Command(CommandAction::Exec)
        );
        assert_eq!(command_alias_exec.default_action(), None);
        assert_eq!(command_alias_exec.parameters, vec!["example-app", "migrate"]);

        let mut command_default_help = Context::new();
        parse_args(&mut command_default_help, ["command"]).unwrap();
        let mut command_explicit_help = Context::new();
        parse_args(&mut command_explicit_help, ["command", "help"]).unwrap();
        assert_eq!(command_default_help, command_explicit_help);
        assert_eq!(
            command_default_help.action,
            ModeAction::Command(CommandAction::Help)
        );

        let mut command_implicit_list = Context::new();
        parse_args(&mut command_implicit_list, ["cmd", "example-app"]).unwrap();
        let mut command_explicit_list = Context::new();
        parse_args(&mut command_explicit_list, ["command", "list", "example-app"]).unwrap();
        assert_eq!(command_implicit_list, command_explicit_list);
        assert_eq!(
            command_implicit_list.action,
            ModeAction::Command(CommandAction::List)
        );
        assert_eq!(command_implicit_list.parameters, vec!["example-app"]);

        let mut command_implicit_exec = Context::new();
        parse_args(&mut command_implicit_exec, ["command", "example-app", "1"]).unwrap();
        assert_eq!(
            command_implicit_exec.action,
            ModeAction::Command(CommandAction::Exec)
        );
        assert_eq!(command_implicit_exec.parameters, vec!["example-app", "1"]);

        let mut config_alias_show = Context::new();
        parse_args(&mut config_alias_show, ["cfg"]).unwrap();
        let mut config_show = Context::new();
        parse_args(&mut config_show, ["config", "show"]).unwrap();
        assert_eq!(config_alias_show, config_show);
        assert_eq!(config_alias_show.mode, Mode::Config);
        assert_eq!(config_alias_show.action, ModeAction::Config(ConfigAction::Show));

        let mut config_edit = Context::new();
        parse_args(&mut config_edit, ["cfg", "edit"]).unwrap();
        assert_eq!(config_edit.action, ModeAction::Config(ConfigAction::Edit));

        let mut config_help = Context::new();
        parse_args(&mut config_help, ["config", "help"]).unwrap();
        assert_eq!(config_help.action, ModeAction::Config(ConfigAction::Help));

        let mut connect_alias_shell = Context::new();
        parse_args(&mut connect_alias_shell, ["con", "example-app/db"]).unwrap();
        let mut connect_shell = Context::new();
        parse_args(&mut connect_shell, ["connect", "shell", "example-app/db"]).unwrap();
        assert_eq!(connect_alias_shell, connect_shell);
        assert_eq!(connect_alias_shell.mode, Mode::Connect);
        assert_eq!(
            connect_alias_shell.action,
            ModeAction::Connect(ConnectAction::Shell)
        );

        let mut connect_default_help = Context::new();
        parse_args(&mut connect_default_help, ["connect"]).unwrap();
        let mut connect_explicit_help = Context::new();
        parse_args(&mut connect_explicit_help, ["connect", "help"]).unwrap();
        assert_eq!(connect_default_help, connect_explicit_help);
        assert_eq!(
            connect_default_help.action,
            ModeAction::Connect(ConnectAction::Help)
        );

        let mut image_alias_prune = Context::new();
        parse_args(
            &mut image_alias_prune,
            ["img", "prune", "--all", "--force", "--preview"],
        )
        .unwrap();
        let mut image_prune = Context::new();
        parse_args(
            &mut image_prune,
            ["image", "prune", "--all", "--force", "--preview"],
        )
        .unwrap();
        assert_eq!(image_alias_prune, image_prune);
        assert_eq!(image_alias_prune.mode, Mode::Image);
        assert_eq!(image_alias_prune.action, ModeAction::Image(ImageAction::Prune));
        assert!(image_alias_prune.requires_config());

        let mut image_default_help = Context::new();
        parse_args(&mut image_default_help, ["image"]).unwrap();
        let mut image_explicit_help = Context::new();
        parse_args(&mut image_explicit_help, ["image", "help"]).unwrap();
        assert_eq!(image_default_help, image_explicit_help);
        assert_eq!(
            image_default_help.action,
            ModeAction::Image(ImageAction::Help)
        );

        let mut image_update = Context::new();
        parse_args(&mut image_update, ["img", "update", "@backend"]).unwrap();
        assert_eq!(image_update.action, ModeAction::Image(ImageAction::Update));
        assert!(image_update.requires_config());

        let mut init_create = Context::new();
        parse_args(&mut init_create, ["init"]).unwrap();
        let mut init_explicit_create = Context::new();
        parse_args(&mut init_explicit_create, ["init", "create"]).unwrap();
        assert_eq!(init_create, init_explicit_create);
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

        let mut logs_default_help = Context::new();
        parse_args(&mut logs_default_help, ["logs"]).unwrap();
        let mut logs_explicit_help = Context::new();
        parse_args(&mut logs_explicit_help, ["logs", "help"]).unwrap();
        assert_eq!(logs_default_help, logs_explicit_help);
        assert_eq!(logs_default_help.action, ModeAction::Logs(LogsAction::Help));

        let mut log_implicit_show = Context::new();
        parse_args(&mut log_implicit_show, ["log", "example-app"]).unwrap();
        assert_eq!(log_implicit_show.action, ModeAction::Logs(LogsAction::Show));

        let mut recipe_alias_list = Context::new();
        parse_args(&mut recipe_alias_list, ["rec", "--all"]).unwrap();
        let mut recipe_list = Context::new();
        parse_args(&mut recipe_list, ["recipe", "list", "--all"]).unwrap();
        assert_eq!(recipe_alias_list, recipe_list);
        assert_eq!(recipe_alias_list.mode, Mode::Recipe);
        assert_eq!(recipe_alias_list.action, ModeAction::Recipe(RecipeAction::List));

        let mut recipe_help = Context::new();
        parse_args(&mut recipe_help, ["recipe", "help"]).unwrap();
        assert_eq!(recipe_help.action, ModeAction::Recipe(RecipeAction::Help));

        let mut recipe_implicit_show = Context::new();
        parse_args(&mut recipe_implicit_show, ["rec", "example-app"]).unwrap();
        let mut recipe_explicit_show = Context::new();
        parse_args(&mut recipe_explicit_show, ["recipe", "show", "example-app"]).unwrap();
        assert_eq!(recipe_implicit_show, recipe_explicit_show);
        assert_eq!(
            recipe_implicit_show.action,
            ModeAction::Recipe(RecipeAction::Show)
        );

        let mut recipe_edit = Context::new();
        parse_args(&mut recipe_edit, ["recipe", "edit", "example-app"]).unwrap();
        assert_eq!(recipe_edit.action, ModeAction::Recipe(RecipeAction::Edit));
    }

    #[test]
    fn red_team_config_path_and_flag_smuggling_exploits() {
        for smuggled_flag in [
            ["--config", "--simulate", "down", "example-app"].as_slice(),
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
            "../pods.conf",
            "configs/../staging",
            "../../etc/passwd",
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
            let mut connect_context = Context::new();
            let connect_error =
                parse_args(&mut connect_context, ["connect", invalid_container]).unwrap_err();
            assert_eq!(
                connect_error.kind,
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
            let mut recipe_context = Context::new();
            let recipe_error =
                parse_args(&mut recipe_context, ["recipe", "edit", invalid_recipe]).unwrap_err();
            assert_eq!(recipe_error.kind, CliErrorKind::InvalidTarget);
        }

        for invalid_command in [".", "..", "cmd/1", "migrate\0", "migrate\n", "migrate now"] {
            let mut command_context = Context::new();
            let command_error = parse_args(
                &mut command_context,
                ["command", "exec", "example-app", invalid_command],
            )
            .unwrap_err();
            assert_eq!(command_error.kind, CliErrorKind::InvalidTarget);
        }

        let mut help_extra = Context::new();
        let extra_error = parse_args(&mut help_extra, ["help", "../../etc/passwd"]).unwrap_err();
        assert_eq!(extra_error.kind, CliErrorKind::TooManyArguments);

        let mut version_extra = Context::new();
        let version_extra_error =
            parse_args(&mut version_extra, ["version", "extra"]).unwrap_err();
        assert_eq!(version_extra_error.kind, CliErrorKind::TooManyArguments);

        let mut status_help_flag = Context::new();
        let status_flag_error =
            parse_args(&mut status_help_flag, ["status", "--help"]).unwrap_err();
        assert_eq!(status_flag_error.kind, CliErrorKind::UnknownFlag);

        for missing_target_args in [
            ["command", "exec"].as_slice(),
            ["command", "exec", "example-app"].as_slice(),
            ["command", "list"].as_slice(),
            ["connect", "shell"].as_slice(),
            ["image", "update"].as_slice(),
            ["logs", "show"].as_slice(),
            ["logs", "follow"].as_slice(),
            ["recipe", "show"].as_slice(),
            ["recipe", "edit"].as_slice(),
        ] {
            let mut context = Context::new();
            let error = parse_args(&mut context, missing_target_args).unwrap_err();
            assert_eq!(error.kind, CliErrorKind::MissingTarget);
        }

        let mut bad_config = Context::new();
        let config_error = parse_args(&mut bad_config, ["config", "bogus"]).unwrap_err();
        assert_eq!(config_error.kind, CliErrorKind::UnknownAction);

        let mut bad_init = Context::new();
        let init_error = parse_args(&mut bad_init, ["init", "bogus"]).unwrap_err();
        assert_eq!(init_error.kind, CliErrorKind::UnknownAction);
    }
}
