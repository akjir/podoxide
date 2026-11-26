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

//! Logs mode (`pods logs` / `pods log`) action dispatcher.

pub mod follow;
pub mod help;
pub mod show;

use std::io::{self, Write};

use crate::core::context::{Context, Flags, LogsAction};
use crate::core::log;
use crate::core::recipe::Recipe;
use crate::error::{CliError, CliErrorKind, PodError};
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::{self, PodLogs};
use crate::utilities::string;

/// Output prefix printed before simulating or executing the `podman pod logs` command.
const LOGS_PREFIX: &str = "Execute log command: ";

/// Dispatches `logs` mode execution to the resolved sub-action (`follow`, `help`, or `show`).
///
/// # Workflow
///
/// 1. Log that `logs` mode is active when debug logging is enabled.
/// 2. Route [`LogsAction`] in enum declaration order (`Follow`, `Help`, `Show`)
///    to the corresponding action module ([`follow::handle`], [`help::handle`], [`show::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if the selected action handler fails.
pub fn handle(context: &Context, action: LogsAction) -> Result<ExecOutcome, PodError> {
    log::debug("Logs mode is used.");
    match action {
        LogsAction::Follow => follow::handle(context),
        LogsAction::Help => {
            help::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
        LogsAction::Show => show::handle(context),
    }
}

/// Resolves the target recipe and optional container and executes `podman pod logs` (with `-f` when `follow` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if the target is missing or invalid, loading or validating the recipe
/// fails, the container specification cannot be resolved, or `podman pod logs` fails.
pub(super) fn execute_logs(context: &Context, follow: bool) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    execute_logs_with(context, follow, &mut stdout, &mut stderr)
}

/// Resolves the target recipe and optional container and executes `podman pod logs`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if the target is missing or invalid, loading or validating the recipe
/// fails, the container specification cannot be resolved, or `podman pod logs` fails.
pub(super) fn execute_logs_with<W: Write, E: Write>(
    context: &Context,
    follow: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    let target = extract_target_parameter(context)?;
    let (recipe_name, container_spec) = parse_target(target)?;
    let (pod_id, container_name) = load_and_resolve_target(context, recipe_name, container_spec)?;
    validate_log_flags(&context.flags)?;
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

/// Loads and validates `<recipe_name>.pod` and resolves the pod identifier and optional container name.
///
/// # Errors
///
/// Returns [`PodError`] if the recipe fails to load or validate, or if `container_spec` does
/// not match any container in the recipe.
fn load_and_resolve_target(
    context: &Context,
    recipe_name: &str,
    container_spec: Option<&str>,
) -> Result<(String, Option<String>), PodError> {
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
    if let Some((recipe_name, container_spec)) = target.split_once('/') {
        if !string::is_valid_target_segment(recipe_name)
            || !string::is_valid_target_segment(container_spec)
        {
            return Err(PodError::from(CliError::with_argument(
                CliErrorKind::InvalidTarget,
                target,
            )));
        }
        return Ok((recipe_name, Some(container_spec)));
    }
    if !string::is_valid_target_segment(target) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidTarget,
            target,
        )));
    }
    Ok((target, None))
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
mod tests {
    use std::path::{Path, PathBuf};

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{Context, Flags, LogsAction};
    use crate::error::{CliErrorKind, PodError, RecipeErrorKind};
    use crate::io::exec::command::ExecOutcome;
    use crate::io::system::fs;
    use super::{execute_logs_with, handle};

    const COMMANDS_RECIPE_PSL: &str = "\
pod cmd_pod {
    name \"Command Pod\"
    description \"A pod with multiple containers.\"
    path /pods/cmd_pod
    registry docker.io
}
container {
    image alpine
    restart never
}
container *db {
    image postgres
    restart never
}
container absolute_db {
    image mariadb
    restart never
}
";

    const INVALID_RECIPE_PSL: &str = "\
pod invalid_pod {
    name \"Invalid Pod\"
}
";

    const SIMPLE_RECIPE_PSL: &str = "\
pod simple_container {
    name \"Simple Container\"
    path /pods/simple_container
    registry docker.io
}
container {
    image alpine
    restart never
}
";

    fn assert_invalid_flag_rejected(context: &Context, flags: Flags) {
        let mut local_context = context.clone();
        local_context.flags = flags;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let error = execute_logs_with(&local_context, false, &mut stdout, &mut stderr).unwrap_err();
        match error {
            PodError::Cli(cli_error) => {
                assert_eq!(cli_error.kind, CliErrorKind::InvalidFlagValue);
            }
            other => panic!("expected PodError::Cli(InvalidFlagValue), got {other:?}"),
        }
    }

    fn build_test_context(suffix: &str) -> (PathBuf, Context) {
        let mut directory = std::env::temp_dir();
        directory.push(format!(
            "podoxide_mode_logs_{}_{}",
            std::process::id(),
            suffix
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        fs::write_file(
            &directory.join("recipe_011_simple_container.pod"),
            SIMPLE_RECIPE_PSL,
        )
        .unwrap();
        fs::write_file(
            &directory.join("recipe_006_commands.pod"),
            COMMANDS_RECIPE_PSL,
        )
        .unwrap();
        fs::write_file(
            &directory.join("recipe_002_invalid.pod"),
            INVALID_RECIPE_PSL,
        )
        .unwrap();

        let mut config = Config::new();
        config.simulate = true;
        config.recipes.path = directory.clone();

        let mut context = Context::new();
        context.config = config;
        (directory, context)
    }

    #[test]
    fn execute_logs_with_rejects_invalid_targets_flags_and_recipes() {
        let (directory, mut context) = build_test_context("errors");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let missing_error =
            execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
        match missing_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![
            String::from("recipe_011_simple_container"),
            String::from("recipe_006_commands"),
        ];
        let extra_error = execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
        match extra_error {
            PodError::Cli(cli_error) => {
                assert_eq!(cli_error.kind, CliErrorKind::TooManyArguments);
            }
            other => panic!("expected PodError::Cli(TooManyArguments), got {other:?}"),
        }

        for invalid_target in [
            "",
            "   ",
            "@all",
            "@backend/db",
            "-flag",
            "recipe_006_commands/",
            "/db",
            "recipe_006_commands/db/extra",
            "recipe_006_commands/-f",
            "recipe_006_commands/@db",
            "recipe_006_commands/..",
            "../recipe_006_commands",
        ] {
            context.parameters = vec![String::from(invalid_target)];
            let error = execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
            match error {
                PodError::Cli(cli_error) => {
                    assert_eq!(cli_error.kind, CliErrorKind::InvalidTarget);
                }
                other => panic!(
                    "expected PodError::Cli(InvalidTarget) for {invalid_target:?}, got {other:?}"
                ),
            }
        }

        context.parameters = vec![String::from("missing_recipe")];
        let not_found_recipe =
            execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
        match not_found_recipe {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::NotFound);
            }
            other => panic!("expected PodError::Recipe(NotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe_002_invalid")];
        let invalid_recipe =
            execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
        match invalid_recipe {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::MissingPodRegistry);
            }
            other => panic!("expected PodError::Recipe(MissingPodRegistry), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe_006_commands/missing")];
        let not_found_container =
            execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
        match not_found_container {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotFound);
            }
            other => panic!("expected PodError::Recipe(ContainerNotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe_011_simple_container")];
        for bad_time in ["-f", " -f", "", "   ", "10\n20"] {
            let mut since_flags = Flags::new();
            since_flags.since = Some(String::from(bad_time));
            assert_invalid_flag_rejected(&context, since_flags);

            let mut until_flags = Flags::new();
            until_flags.until = Some(String::from(bad_time));
            assert_invalid_flag_rejected(&context, until_flags);
        }
        for bad_tail in ["-f", "", "10\n20", "two words", "abc"] {
            let mut tail_flags = Flags::new();
            tail_flags.tail = Some(String::from(bad_tail));
            assert_invalid_flag_rejected(&context, tail_flags);
        }

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn execute_logs_with_simulates_show_follow_options_and_container_filters() {
        let (directory, mut context) = build_test_context("simulate");

        let mut option_flags = Flags::new();
        option_flags.since = Some(String::from("2026-01-01 12:00:00"));
        option_flags.tail = Some(String::from("all"));
        option_flags.until = Some(String::from("2026-01-01 13:00:00"));
        option_flags.set_timestamps(true);

        let cases = [
            (
                false,
                Flags::new(),
                "recipe_011_simple_container",
                "Execute log command: \npodman pod logs -n --color simple_container\n",
            ),
            (
                true,
                Flags::new(),
                "recipe_011_simple_container",
                "Execute log command: \npodman pod logs -n --color -f simple_container\n",
            ),
            (
                false,
                option_flags,
                "recipe_011_simple_container",
                "Execute log command: \npodman pod logs -n --color --since '2026-01-01 12:00:00' --until '2026-01-01 13:00:00' --tail all --timestamps simple_container\n",
            ),
            (
                false,
                Flags::new(),
                "recipe_006_commands/1",
                "Execute log command: \npodman pod logs -n --color -c cmd_pod-1 cmd_pod\n",
            ),
            (
                false,
                Flags::new(),
                "recipe_006_commands/*db",
                "Execute log command: \npodman pod logs -n --color -c cmd_pod-db cmd_pod\n",
            ),
            (
                false,
                Flags::new(),
                "recipe_006_commands/db",
                "Execute log command: \npodman pod logs -n --color -c cmd_pod-db cmd_pod\n",
            ),
            (
                false,
                Flags::new(),
                "recipe_006_commands/cmd_pod-db",
                "Execute log command: \npodman pod logs -n --color -c cmd_pod-db cmd_pod\n",
            ),
            (
                true,
                Flags::new(),
                "recipe_006_commands/absolute_db",
                "Execute log command: \npodman pod logs -n --color -f -c absolute_db cmd_pod\n",
            ),
        ];

        for (follow, flags, target, expected_output) in cases {
            context.flags = flags;
            context.parameters = vec![String::from(target)];
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            assert_eq!(
                execute_logs_with(&context, follow, &mut stdout, &mut stderr).unwrap(),
                ExecOutcome::Succeeded
            );
            assert_eq!(String::from_utf8(stdout).unwrap(), expected_output);
            assert!(stderr.is_empty());
        }

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn handle_routes_logs_actions() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        let help_context = Context::new();
        assert_eq!(
            handle(&help_context, LogsAction::Help).unwrap(),
            ExecOutcome::Succeeded
        );

        for action in [LogsAction::Follow, LogsAction::Show] {
            let mut context = Context::new();
            context.config = root_config.clone();
            context.parameters = vec![String::from("recipe/db")];
            assert_eq!(
                handle(&context, action).unwrap(),
                ExecOutcome::Succeeded,
                "expected logs::{action} to succeed"
            );
        }
    }
}
