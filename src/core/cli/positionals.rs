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

//! Positional CLI argument resolver for modes, sub-actions, and target specifications.

use std::str::FromStr;

use crate::core::context::{
    CommandAction, ConfigAction, ConnectAction, DefaultAction, ImageAction, InitAction, LogsAction,
    Mode, ModeAction, RecipeAction,
};
use crate::error::{CliError, CliErrorKind};
use crate::utilities::string;

/// Parses implicit `pods command <recipe> [<command|index>]` arguments into `list` or `exec`.
pub(super) fn parse_command_implicit(
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

/// Resolves the [`ModeAction`] and remaining target parameters from `positionals`.
pub(super) fn parse_mode(
    mut positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Default(DefaultAction::Help), Vec::new()));
    };
    if let Ok(mode) = Mode::from_str(first) {
        positionals.remove(0);
        return parse_mode_action(mode, positionals);
    }
    parse_default_positionals(positionals)
}

/// Routes positional arguments to the mode-specific action parser for `mode`.
pub(super) fn parse_mode_action(
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

/// Verifies that `positionals` contains at most `max_count` arguments.
fn ensure_no_extra_arguments(positionals: &[String], max_count: usize) -> Result<(), CliError> {
    if let Some(extra) = positionals.get(max_count) {
        return Err(CliError::with_argument(
            CliErrorKind::TooManyArguments,
            extra,
        ));
    }
    Ok(())
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

/// Parses positional arguments for `config` (`cfg`) mode.
fn parse_config_positionals(
    positionals: Vec<String>,
) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Config(ConfigAction::Inspect), Vec::new()));
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
    validate_lifecycle_targets(&positionals)?;
    Ok((ModeAction::Default(action), positionals))
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
            validate_lifecycle_targets(&positionals)?;
            Ok((ModeAction::Image(ImageAction::Update), positionals))
        }
    }
}

/// Parses positional arguments for `init` mode.
fn parse_init_positionals(positionals: Vec<String>) -> Result<(ModeAction, Vec<String>), CliError> {
    let Some(first) = positionals.first() else {
        return Ok((ModeAction::Init(InitAction::Create), Vec::new()));
    };
    let action = InitAction::from_str(first)?;
    ensure_no_extra_arguments(&positionals, 1)?;
    Ok((ModeAction::Init(action), Vec::new()))
}

/// Parses positional arguments for `logs` (`log`) mode.
fn parse_logs_positionals(positionals: Vec<String>) -> Result<(ModeAction, Vec<String>), CliError> {
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
        "inspect" => parse_recipe_target(RecipeAction::Inspect, positionals, 1),
        "list" => {
            ensure_no_extra_arguments(&positionals, 1)?;
            Ok((ModeAction::Recipe(RecipeAction::List), Vec::new()))
        }
        "show" => parse_recipe_target(RecipeAction::Show, positionals, 1),
        _ => parse_recipe_target(RecipeAction::Show, positionals, 0),
    }
}

/// Extracts and validates the `<recipe>` target for `recipe edit`, `recipe inspect`, or `recipe show`.
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

/// Validates a container target (`<target>` or `<recipe>/<container>`) for `connect` and `logs` modes.
fn validate_container_target(target: &str) -> Result<(), CliError> {
    if let Some((recipe, container)) = target.split_once('/') {
        if !string::is_valid_target_segment(recipe) || !string::is_valid_target_segment(container) {
            return Err(CliError::with_argument(CliErrorKind::InvalidTarget, target));
        }
        return Ok(());
    }
    if !string::is_valid_target_segment(target) {
        return Err(CliError::with_argument(CliErrorKind::InvalidTarget, target));
    }
    Ok(())
}

/// Validates a `<recipe>/<container>` target argument requiring an explicit `'/'` separator.
fn validate_explicit_container_target(target: &str) -> Result<(), CliError> {
    let Some((recipe, container)) = target.split_once('/') else {
        return Err(CliError::with_argument(CliErrorKind::InvalidTarget, target));
    };
    if !string::is_valid_target_segment(recipe) || !string::is_valid_target_segment(container) {
        return Err(CliError::with_argument(CliErrorKind::InvalidTarget, target));
    }
    Ok(())
}

/// Validates a single `<recipe>` or `@<group>` target argument.
fn validate_group_or_recipe_target(target: &str) -> Result<(), CliError> {
    let segment = target.strip_prefix('@').unwrap_or(target);
    if !string::is_valid_target_segment(segment) {
        return Err(CliError::with_argument(CliErrorKind::InvalidTarget, target));
    }
    Ok(())
}

/// Validates a single lifecycle target as `<recipe>/<container>` (when containing `'/'`) or `<recipe>`/`@<group>`.
fn validate_lifecycle_target(target: &str) -> Result<(), CliError> {
    if target.contains('/') {
        return validate_explicit_container_target(target);
    }
    validate_group_or_recipe_target(target)
}

/// Validates a slice of lifecycle targets, checking syntax on every target before enforcing uniform `<recipe>`/`@<group>` or `<recipe>/<container>` targeting.
fn validate_lifecycle_targets(parameters: &[String]) -> Result<(), CliError> {
    for parameter in parameters {
        validate_lifecycle_target(parameter)?;
    }
    validate_mixed_targets(parameters)
}

/// Rejects mixing `<recipe>`/`@<group>` targets with `<recipe>/<container>` targets.
fn validate_mixed_targets(parameters: &[String]) -> Result<(), CliError> {
    let Some((first, rest)) = parameters.split_first() else {
        return Ok(());
    };
    let expect_container = first.contains('/');
    for parameter in rest {
        if parameter.contains('/') != expect_container {
            return Err(CliError::with_argument(
                CliErrorKind::MixedTargets,
                parameter,
            ));
        }
    }
    Ok(())
}

/// Validates a single `<recipe>` or `<command|index>` target that does not allow `@group` or `/` syntax.
fn validate_recipe_only_target(target: &str) -> Result<(), CliError> {
    if !string::is_valid_target_segment(target) {
        return Err(CliError::with_argument(CliErrorKind::InvalidTarget, target));
    }
    Ok(())
}
