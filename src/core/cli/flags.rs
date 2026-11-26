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

//! Single-pass CLI flag scanner and action-aware flag validator.

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::core::config;
use crate::core::context::{
    Context, DefaultAction, Flags, ImageAction, LogsAction, Mode, ModeAction, RecipeAction,
};
use crate::error::{CliError, CliErrorKind};
use crate::utilities::{path, string};
use super::MAX_CLI_ARGS;

/// Initial capacity pre-allocated for positional CLI arguments.
const INITIAL_POSITIONALS_CAPACITY: usize = 8;

/// Validates and stores a value-taking CLI flag (`--config`, `--since`, `--tail`, `--until`) into `flags`.
pub(super) fn apply_flag_value(
    flags: &mut Flags,
    flag_name: &str,
    raw_value: Cow<'_, str>,
) -> Result<(), CliError> {
    if string::is_empty_value_literal(&raw_value) {
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
            flags.since = Some(parse_log_time_value(flag_name, raw_value)?);
            Ok(())
        }
        "--tail" => {
            flags.tail = Some(parse_log_tail_value(raw_value)?);
            Ok(())
        }
        "--until" => {
            flags.until = Some(parse_log_time_value(flag_name, raw_value)?);
            Ok(())
        }
        _ => Err(CliError::with_argument(
            CliErrorKind::UnknownFlag,
            flag_name,
        )),
    }
}

/// Scans all raw CLI arguments in a single pass, populating `flags` and returning positional arguments.
pub(super) fn scan_raw_arguments<I, T>(
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

/// Verifies that all parsed mode-specific and action-specific flags are permitted for `context.action`.
pub(super) fn validate_mode_flags(context: &Context) -> Result<(), CliError> {
    if context.flags.all() && !allows_all_flag(context) {
        return unexpected_flag("--all");
    }
    if context.flags.config.is_some() && context.mode() == Mode::Init {
        return unexpected_flag("--config");
    }
    if context.flags.full() && context.action != ModeAction::Default(DefaultAction::Status) {
        return unexpected_flag("--full");
    }
    if context.flags.force()
        && (context.action != ModeAction::Image(ImageAction::Prune) || context.flags.preview())
    {
        return unexpected_flag("--force");
    }
    if context.flags.preview() && context.action != ModeAction::Image(ImageAction::Prune) {
        return unexpected_flag("--preview");
    }
    validate_logs_mode_flags(context)
}

/// Evaluates whether `--all` is permitted for the resolved action and target list in `context`.
#[must_use]
const fn allows_all_flag(context: &Context) -> bool {
    match context.action {
        ModeAction::Default(DefaultAction::Status) => context.parameters.is_empty(),
        ModeAction::Image(ImageAction::Prune) | ModeAction::Recipe(RecipeAction::List) => true,
        _ => false,
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

/// Converts an owned `OsString` command-line argument to an owned UTF-8 `String` without reallocating.
fn os_string_to_string(argument: OsString) -> Result<String, CliError> {
    argument
        .into_string()
        .map_err(|_| CliError::new(CliErrorKind::InvalidUtf8))
}

/// Validates a `--config` file path argument and converts it to a [`PathBuf`].
fn parse_config_path(raw_value: Cow<'_, str>) -> Result<PathBuf, CliError> {
    if path::has_parent_dir(Path::new(raw_value.as_ref()))
        || config::extract_config_stem(raw_value.as_ref()).is_none()
    {
        return Err(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            "--config",
        ));
    }
    Ok(PathBuf::from(raw_value.into_owned()))
}

/// Parses a `--flag=value` argument token, rejecting `=value` on boolean flags with consistent flag subjects.
fn parse_equals_flag(flags: &mut Flags, flag_name: &str, raw_value: &str) -> Result<(), CliError> {
    match flag_name {
        "--config" | "--since" | "--tail" | "--until" => {
            apply_flag_value(flags, flag_name, Cow::Borrowed(raw_value))
        }
        "--all" | "--debug" | "--force" | "--full" | "--preview" | "--simulate"
        | "--timestamps" => Err(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            flag_name,
        )),
        _ => Err(CliError::with_argument(
            CliErrorKind::UnknownFlag,
            flag_name,
        )),
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
        "--all" => flags.set_all(true),
        "--config" | "--since" | "--tail" | "--until" => {
            let value = consume_next_value(iterator, argument_count, flag_name)?;
            return apply_flag_value(flags, flag_name, Cow::Owned(value));
        }
        "--debug" => flags.set_debug(true),
        "--force" => flags.set_force(true),
        "--full" => flags.set_full(true),
        "--preview" => flags.set_preview(true),
        "--simulate" => flags.set_simulate(true),
        "--timestamps" => flags.set_timestamps(true),
        _ => {
            return Err(CliError::with_argument(
                CliErrorKind::UnknownFlag,
                flag_name,
            ));
        }
    }
    Ok(())
}

/// Validates a `--tail` flag value, permitting non-negative ASCII integers or `"all"` (case-insensitive).
fn parse_log_tail_value(raw_value: Cow<'_, str>) -> Result<String, CliError> {
    if !string::is_valid_log_tail(raw_value.as_ref()) {
        return Err(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            "--tail",
        ));
    }
    Ok(raw_value.into_owned())
}

/// Validates a `--since` or `--until` duration/timestamp value, permitting spaces while rejecting control characters and leading dashes.
fn parse_log_time_value(flag_name: &str, raw_value: Cow<'_, str>) -> Result<String, CliError> {
    if !string::is_valid_log_time(raw_value.as_ref()) {
        return Err(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            flag_name,
        ));
    }
    Ok(raw_value.into_owned())
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

/// Constructs a [`CliErrorKind::UnexpectedFlag`] error for `flag_name`.
fn unexpected_flag(flag_name: &str) -> Result<(), CliError> {
    Err(CliError::with_argument(
        CliErrorKind::UnexpectedFlag,
        flag_name,
    ))
}

/// Verifies that log-specific flags (`--since`, `--tail`, `--timestamps`, `--until`) are used only with `logs show` or `logs follow`.
fn validate_logs_mode_flags(context: &Context) -> Result<(), CliError> {
    let allows_logs = matches!(
        context.action,
        ModeAction::Logs(LogsAction::Follow | LogsAction::Show)
    );
    if context.flags.since.is_some() && !allows_logs {
        return unexpected_flag("--since");
    }
    if context.flags.tail.is_some() && !allows_logs {
        return unexpected_flag("--tail");
    }
    if context.flags.timestamps() && !allows_logs {
        return unexpected_flag("--timestamps");
    }
    if context.flags.until.is_some() && !allows_logs {
        return unexpected_flag("--until");
    }
    Ok(())
}
