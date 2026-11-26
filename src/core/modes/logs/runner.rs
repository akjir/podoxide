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

//! Shared target validation, flag validation, and `podman pod logs` execution for `pods logs`.

use std::io::{self, Write};

use crate::core::context::{Context, Flags};
use crate::core::modes::targets;
use crate::core::recipe::Recipe;
use crate::error::{CliError, CliErrorKind, ConfigError, ConfigErrorKind, PodError};
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::{self, PodLogs};
use crate::utilities::string;

/// Output prefix printed before simulating or executing the `podman pod logs` command.
const LOGS_PREFIX: &str = "Execute log command: ";

/// Resolves the target recipe and optional container and executes `podman pod logs` (with `-f` when `follow` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if the target or flags are invalid, the recipe is not registered, loading
/// or validating the recipe fails, the container specification cannot be resolved, or
/// `podman pod logs` fails.
pub(super) fn execute_logs(context: &Context, follow: bool) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    execute_logs_with(context, follow, &mut stdout, &mut stderr)
}

/// Resolves the target recipe and optional container and executes `podman pod logs`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if the target or flags are invalid, the recipe is not registered, loading
/// or validating the recipe fails, the container specification cannot be resolved, or
/// `podman pod logs` fails.
pub(super) fn execute_logs_with<W: Write, E: Write>(
    context: &Context,
    follow: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    let target = extract_target_parameter(context)?;
    let (recipe_name, container_spec) = parse_target(target)?;
    validate_log_flags(&context.flags)?;
    let (pod_id, container_name) = load_and_resolve_target(context, recipe_name, container_spec)?;
    podman::logs_pod(
        &PodLogs {
            container_name: container_name.as_deref(),
            follow,
            pod_name: &pod_id,
            since: context.flags.since.as_deref(),
            tail: context.flags.tail.as_deref(),
            timestamps: context.flags.timestamps(),
            until: context.flags.until.as_deref(),
        },
        LOGS_PREFIX,
        context.config.simulate,
        stdout,
        stderr,
    )
}

/// Extracts the single positional `<recipe>[/<container>]` parameter from `context.parameters`.
///
/// # Errors
///
/// Returns [`PodError`] with [`CliErrorKind::MissingTarget`] when no parameter is present, or
/// [`CliErrorKind::TooManyArguments`] when extra parameters are present.
fn extract_target_parameter(context: &Context) -> Result<&str, PodError> {
    let Some(target) = context.parameters.first() else {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    };
    if let Some(extra) = context.parameters.get(1) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::TooManyArguments,
            extra,
        )));
    }
    Ok(target)
}

/// Verifies that `recipe_name` is registered in `context.config`, loads and validates `<recipe_name>.pod`, and resolves the pod identifier and optional container name.
///
/// # Errors
///
/// Returns [`PodError`] if `recipe_name` is not registered in `context.config.recipes.groups`,
/// the recipe fails to load or validate, or `container_spec` does not match any container.
fn load_and_resolve_target(
    context: &Context,
    recipe_name: &str,
    container_spec: Option<&str>,
) -> Result<(String, Option<String>), PodError> {
    if !context.config.has_recipe(recipe_name) {
        return Err(PodError::from(ConfigError::without_line(
            ConfigErrorKind::RecipeNotFound,
            recipe_name,
        )));
    }
    let recipe = Recipe::inspect(&context.config, recipe_name)?;
    let resolved_container = match container_spec {
        Some(spec) => Some(recipe.resolve_container_name(spec)?),
        None => None,
    };
    Ok((recipe.pod.id, resolved_container))
}

/// Splits `target` into `<recipe>` and an optional `/<container>` specification and validates both segments.
///
/// # Errors
///
/// Returns [`PodError`] with [`CliErrorKind::InvalidTarget`] if the recipe segment or
/// container segment is empty, starts with `-` or `@`, or contains invalid characters.
fn parse_target(target: &str) -> Result<(&str, Option<&str>), PodError> {
    let (recipe_name, container_spec) = targets::split_recipe_container_spec(target)?;
    if container_spec.is_none() && !string::is_valid_target_segment(recipe_name) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidTarget,
            target,
        )));
    }
    Ok((recipe_name, container_spec))
}

/// Validates `--since`, `--until`, and `--tail` flag values in `flags`.
fn validate_log_flags(flags: &Flags) -> Result<(), PodError> {
    validate_time_flag("--since", flags.since.as_deref())?;
    validate_time_flag("--until", flags.until.as_deref())?;
    validate_tail_flag(flags.tail.as_deref())
}

/// Validates that an optional `--tail` value (when `Some`) is a non-negative integer or `"all"`.
fn validate_tail_flag(value: Option<&str>) -> Result<(), PodError> {
    let Some(raw_value) = value else {
        return Ok(());
    };
    if !string::is_valid_log_tail(raw_value) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            "--tail",
        )));
    }
    Ok(())
}

/// Validates that an optional `--since` or `--until` value (when `Some`) is safe timestamp/duration text.
fn validate_time_flag(flag_name: &str, value: Option<&str>) -> Result<(), PodError> {
    let Some(raw_value) = value else {
        return Ok(());
    };
    if !string::is_valid_log_time(raw_value) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidFlagValue,
            flag_name,
        )));
    }
    Ok(())
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
