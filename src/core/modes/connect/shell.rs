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

//! Handler for `pods connect [shell] <target>`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::recipe::Recipe;
use crate::error::{CliError, CliErrorKind, PodError, RecipeError, RecipeErrorKind};
use crate::io::exec::command::{self, ExecOptions};
use crate::io::exec::podman;
use crate::utilities::string::{
    is_valid_pod_or_container_name, is_valid_target_segment, is_valid_text,
};

/// Output prefix printed before simulating or executing the interactive shell command.
const CONNECT_PREFIX: &str = "Execute connect command: ";
/// Default 1-based container index used when a recipe defines a single container.
const DEFAULT_CONTAINER_INDEX: &str = "1";
/// In-container shell detection script preferring `bash` with fallback to `sh`.
const SHELL_FALLBACK_SCRIPT: &str = "command -v bash >/dev/null 2>&1 && exec bash || exec sh";

/// Resolves the target container and opens an interactive shell session via `podman exec -it`.
///
/// # Workflow
///
/// 1. Extract `<target>` (`context.parameters[0]`) and split an optional `/<container>`
///    specification (`<recipe>/<container>` vs `<target>`).
/// 2. Check whether `<recipes.path>/<recipe>.pod` exists on disk:
///    - If `<recipe>.pod` exists: read and validate only that single `.pod` file. If no
///      `/<container>` suffix was provided, require that the recipe defines exactly 1
///      container (defaulting to index `1`); otherwise resolve `<container>` by 1-based
///      index, relative name (`*db` / `db`), or full name.
///    - If `<recipe>.pod` does not exist and no `/<container>` suffix was given: treat
///      `<target>` directly as an unmanaged Podman container name without reading any
///      `.pod` file.
///    - If `<recipe>.pod` does not exist and a `/<container>` suffix was given: return a
///      missing recipe error.
/// 3. When `context.config.simulate` is `false`, verify via `podman` that the resolved
///    container exists and is running.
/// 4. Execute (or simulate) `podman exec -it <container_name> ...` interactively, preferring
///    `bash` and falling back to `sh`.
///
/// # Errors
///
/// Returns [`PodError`] if the recipe or container cannot be resolved, the target container
/// is not running, or the interactive `podman exec` process fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    execute_with(context, podman::container_exists, &mut stdout, &mut stderr)
}

/// Resolves the target container from `context`, checks runtime status via `is_running`, and runs `podman exec -it`.
///
/// # Errors
///
/// Returns [`PodError`] if the CLI target is missing or invalid, recipe loading or container
/// resolution fails, the container is not running, or `podman exec` fails.
fn execute_with<W, E, F>(
    context: &Context,
    is_running: F,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    W: Write,
    E: Write,
    F: FnOnce(&str) -> bool,
{
    let target = extract_target_parameter(context)?;
    let (recipe_name, container_spec) = parse_target(target)?;
    let recipe_exists = Recipe::file_exists(&context.config.recipes.path, recipe_name);
    let container_name =
        resolve_target_container(context, recipe_name, container_spec, recipe_exists)?;
    verify_running_container(
        &container_name,
        target,
        recipe_exists,
        context.config.simulate,
        is_running,
    )?;
    let arguments = [
        "exec",
        "-it",
        container_name.as_str(),
        "sh",
        "-c",
        SHELL_FALLBACK_SCRIPT,
    ];
    command::exec_with(
        "podman",
        &arguments,
        &ExecOptions {
            interactive: true,
            prefix: CONNECT_PREFIX,
            simulate: context.config.simulate,
        },
        stdout,
        stderr,
    )
}

/// Extracts and validates the single positional `<target>` parameter from `context.parameters`.
///
/// # Errors
///
/// Returns [`PodError`] with [`CliErrorKind::MissingTarget`] when no parameter is present,
/// [`CliErrorKind::TooManyArguments`] when extra parameters are present, or
/// [`CliErrorKind::InvalidTarget`] when the target text is empty or contains control characters.
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
    if !is_valid_text(target) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidTarget,
            target,
        )));
    }
    Ok(target)
}

/// Splits `target` on an optional `'/'` separator and validates both segments when `'/'` is present.
///
/// # Errors
///
/// Returns [`PodError`] with [`CliErrorKind::InvalidTarget`] if `target` contains `'/'` with
/// an empty or malformed recipe or container segment.
fn parse_target(target: &str) -> Result<(&str, Option<&str>), PodError> {
    if let Some((recipe_name, container_spec)) = target.split_once('/') {
        if !is_valid_target_segment(recipe_name) || !is_valid_target_segment(container_spec) {
            return Err(PodError::from(CliError::with_argument(
                CliErrorKind::InvalidTarget,
                target,
            )));
        }
        return Ok((recipe_name, Some(container_spec)));
    }
    Ok((target, None))
}

/// Loads and validates `<recipe_name>.pod` and resolves the target container name.
///
/// # Errors
///
/// Returns [`PodError`] if the recipe fails to load or validate, has multiple containers
/// when `container_spec` is `None`, or does not contain a matching container.
fn resolve_recipe_container(
    context: &Context,
    recipe_name: &str,
    container_spec: Option<&str>,
) -> Result<String, PodError> {
    let recipe = Recipe::inspect(&context.config, recipe_name)?;
    let effective_spec = match container_spec {
        Some(spec) => spec,
        None if recipe.containers.len() == 1 => DEFAULT_CONTAINER_INDEX,
        None => {
            return Err(PodError::from(RecipeError::without_line(
                RecipeErrorKind::MultipleContainers,
                recipe_name,
            )));
        }
    };
    let container_name = recipe.resolve_container_name(effective_spec)?;
    Ok(container_name)
}

/// Resolves the absolute Podman container name from either an existing `.pod` recipe or a direct container target.
///
/// # Errors
///
/// Returns [`PodError`] if recipe or unmanaged container resolution fails.
fn resolve_target_container(
    context: &Context,
    recipe_name: &str,
    container_spec: Option<&str>,
    recipe_exists: bool,
) -> Result<String, PodError> {
    if recipe_exists {
        return resolve_recipe_container(context, recipe_name, container_spec);
    }
    resolve_unmanaged_container(recipe_name, container_spec)
}

/// Validates and returns `recipe_name` as a direct container name when no `.pod` file exists.
///
/// # Errors
///
/// Returns [`PodError`] with [`RecipeErrorKind::NotFound`] when a `/<container>` suffix was
/// specified for a non-existent recipe, or [`CliErrorKind::InvalidTarget`] when `recipe_name`
/// is not a valid Podman container name.
fn resolve_unmanaged_container(
    recipe_name: &str,
    container_spec: Option<&str>,
) -> Result<String, PodError> {
    if container_spec.is_some() {
        return Err(PodError::from(RecipeError::without_line(
            RecipeErrorKind::NotFound,
            recipe_name,
        )));
    }
    if !is_valid_pod_or_container_name(recipe_name) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidTarget,
            recipe_name,
        )));
    }
    Ok(String::from(recipe_name))
}

/// Verifies that `container_name` is running in Podman when `simulate` is `false`.
///
/// # Errors
///
/// Returns [`PodError`] with [`RecipeErrorKind::ContainerNotRunning`] when `recipe_exists` is
/// `true` and the container is not running, or [`CliErrorKind::TargetNotFound`] when
/// `recipe_exists` is `false` and the target container is not running.
fn verify_running_container<F>(
    container_name: &str,
    target: &str,
    recipe_exists: bool,
    simulate: bool,
    is_running: F,
) -> Result<(), PodError>
where
    F: FnOnce(&str) -> bool,
{
    if simulate || is_running(container_name) {
        return Ok(());
    }
    if recipe_exists {
        return Err(PodError::from(RecipeError::without_line(
            RecipeErrorKind::ContainerNotRunning,
            container_name,
        )));
    }
    Err(PodError::from(CliError::with_argument(
        CliErrorKind::TargetNotFound,
        target,
    )))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::core::config::Config;
    use crate::core::context::Context;
    use crate::error::{CliErrorKind, PodError, RecipeErrorKind};
    use crate::io::system::fs;
    use super::{execute_with, handle};

    const INVALID_RECIPE_PSL: &str = "\
pod invalid_pod {
    name \"Invalid Pod\"
}
";

    const MULTI_CONTAINER_RECIPE_PSL: &str = "\
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

    const SINGLE_CONTAINER_RECIPE_PSL: &str = "\
pod si_po {
    name \"Single Pod\"
    path /pods/si_po
    registry docker.io
}
container {
    image alpine
    restart never
}
";

    fn build_test_context(suffix: &str) -> (PathBuf, Context) {
        let mut directory = std::env::temp_dir();
        directory.push(format!(
            "podoxide_connect_shell_{}_{}",
            std::process::id(),
            suffix
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        fs::write_file(
            &directory.join("recipe_001_valid.pod"),
            SINGLE_CONTAINER_RECIPE_PSL,
        )
        .unwrap();
        fs::write_file(
            &directory.join("recipe_006_commands.pod"),
            MULTI_CONTAINER_RECIPE_PSL,
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
    fn execute_with_rejects_invalid_parameters_and_targets() {
        let (directory, mut context) = build_test_context("invalid_params");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let missing_error =
            execute_with(&context, |_| true, &mut stdout, &mut stderr).unwrap_err();
        match missing_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("   ")];
        let blank_error = execute_with(&context, |_| true, &mut stdout, &mut stderr).unwrap_err();
        match blank_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::InvalidTarget),
            other => panic!("expected PodError::Cli(InvalidTarget), got {other:?}"),
        }

        context.parameters = vec![
            String::from("recipe_001_valid"),
            String::from("extra_parameter"),
        ];
        let extra_error = execute_with(&context, |_| true, &mut stdout, &mut stderr).unwrap_err();
        match extra_error {
            PodError::Cli(cli_error) => {
                assert_eq!(cli_error.kind, CliErrorKind::TooManyArguments);
            }
            other => panic!("expected PodError::Cli(TooManyArguments), got {other:?}"),
        }

        for invalid_target in [
            "recipe_001_valid/",
            "/db",
            "recipe_001_valid/db/extra",
            "recipe_001_valid/-flag",
            "Invalid Container Name!",
            "-flag",
            "Upper_Case",
            "bad..name",
        ] {
            context.parameters = vec![String::from(invalid_target)];
            let error = execute_with(&context, |_| true, &mut stdout, &mut stderr).unwrap_err();
            match error {
                PodError::Cli(cli_error) => {
                    assert_eq!(cli_error.kind, CliErrorKind::InvalidTarget);
                }
                other => panic!(
                    "expected PodError::Cli(InvalidTarget) for {invalid_target:?}, got {other:?}"
                ),
            }
        }

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn execute_with_rejects_missing_or_invalid_recipes_and_non_running_containers() {
        let (directory, mut context) = build_test_context("recipe_errors");
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        context.parameters = vec![String::from("recipe_006_commands")];
        let multi_error = execute_with(&context, |_| true, &mut stdout, &mut stderr).unwrap_err();
        match multi_error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::MultipleContainers);
            }
            other => panic!("expected PodError::Recipe(MultipleContainers), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe_006_commands/nonexistent")];
        let not_found_container =
            execute_with(&context, |_| true, &mut stdout, &mut stderr).unwrap_err();
        match not_found_container {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotFound);
            }
            other => panic!("expected PodError::Recipe(ContainerNotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("nonexistent_recipe/1")];
        let missing_recipe =
            execute_with(&context, |_| true, &mut stdout, &mut stderr).unwrap_err();
        match missing_recipe {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::NotFound);
            }
            other => panic!("expected PodError::Recipe(NotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe_002_invalid")];
        let invalid_recipe =
            execute_with(&context, |_| true, &mut stdout, &mut stderr).unwrap_err();
        match invalid_recipe {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::MissingPodRegistry);
            }
            other => panic!("expected PodError::Recipe(MissingPodRegistry), got {other:?}"),
        }

        context.config.simulate = false;
        context.parameters = vec![String::from("recipe_001_valid")];
        let not_running_recipe =
            execute_with(&context, |_| false, &mut stdout, &mut stderr).unwrap_err();
        match not_running_recipe {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotRunning);
                assert_eq!(recipe_error.token.as_deref(), Some("si_po-1"));
            }
            other => panic!("expected PodError::Recipe(ContainerNotRunning), got {other:?}"),
        }

        context.parameters = vec![String::from("my_custom_container")];
        let not_running_direct =
            execute_with(&context, |_| false, &mut stdout, &mut stderr).unwrap_err();
        match not_running_direct {
            PodError::Cli(cli_error) => {
                assert_eq!(cli_error.kind, CliErrorKind::TargetNotFound);
                assert_eq!(cli_error.argument.as_deref(), Some("my_custom_container"));
            }
            other => panic!("expected PodError::Cli(TargetNotFound), got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn execute_with_simulates_single_multi_and_direct_container_targets() {
        let (directory, mut context) = build_test_context("simulate");

        let cases = [
            (
                "recipe_001_valid",
                "Execute connect command: \npodman exec -it si_po-1 sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'\n",
            ),
            (
                "recipe_006_commands/1",
                "Execute connect command: \npodman exec -it cmd_pod-1 sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'\n",
            ),
            (
                "recipe_006_commands/2",
                "Execute connect command: \npodman exec -it cmd_pod-db sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'\n",
            ),
            (
                "recipe_006_commands/*db",
                "Execute connect command: \npodman exec -it cmd_pod-db sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'\n",
            ),
            (
                "recipe_006_commands/db",
                "Execute connect command: \npodman exec -it cmd_pod-db sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'\n",
            ),
            (
                "recipe_006_commands/absolute_db",
                "Execute connect command: \npodman exec -it absolute_db sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'\n",
            ),
            (
                "my_custom_container",
                "Execute connect command: \npodman exec -it my_custom_container sh -c 'command -v bash >/dev/null 2>&1 && exec bash || exec sh'\n",
            ),
        ];

        for (target, expected_output) in cases {
            context.parameters = vec![String::from(target)];
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            execute_with(&context, |_| false, &mut stdout, &mut stderr).unwrap();
            assert_eq!(String::from_utf8(stdout).unwrap(), expected_output);
            assert!(stderr.is_empty());
        }

        context.parameters = vec![String::from("recipe_001_valid")];
        assert!(handle(&context).is_ok());

        // Invalid recipes.path directory falls back cleanly to direct container name resolution.
        context.config.recipes.path = PathBuf::from("../invalid_dir");
        context.parameters = vec![String::from("my_custom_container")];
        let mut fallback_stdout = Vec::new();
        let mut fallback_stderr = Vec::new();
        execute_with(
            &context,
            |_| false,
            &mut fallback_stdout,
            &mut fallback_stderr,
        )
        .unwrap();
        assert!(
            String::from_utf8(fallback_stdout)
                .unwrap()
                .contains("podman exec -it my_custom_container")
        );

        let _ = std::fs::remove_dir_all(&directory);
    }
}
