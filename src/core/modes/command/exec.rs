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

//! Handler for `pods command exec <recipe> <command|index>`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::pod::PodCommand;
use crate::core::recipe::Recipe;
use crate::error::{CliError, CliErrorKind, PodError, RecipeError, RecipeErrorKind};
use crate::io::exec::command::{self, ExecOptions};
use crate::utilities::collection::{MAX_COLLECTION_PREALLOC, bounded_capacity};
use crate::utilities::string::{is_valid_text, is_valid_token, parse_ascii_u32};
use super::{get_valid_commands, is_valid_command_entry, load_target_recipe};

const BASE_EXEC_ARG_COUNT: usize = 5;

/// Resolves and executes a predefined maintenance command declared in a recipe's `pod` block.
///
/// # Workflow
///
/// 1. Extract `<recipe>` (`context.parameters[0]`) and `<command|index>` (`context.parameters[1]`).
/// 2. Verify that `<recipe>` is registered in `context.config.recipes.groups`.
/// 3. Read and validate only the single targeted recipe file (`<recipes.path>/<recipe>.pod`).
///    Do not read any other `.pod` files in the directory.
/// 4. Resolve the target `command <name>` block from `recipe.pod.commands`:
///    - If `<command|index>` parses as a 1-based positive integer, collect and sort valid
///      commands alphabetically (suppressing missing-description warnings) and select the
///      command at that 1-based index.
///    - Otherwise, look up the command block by its exact identifier `<command>`.
/// 5. Validate that the resolved command block defines valid `container`, `execute`, and
///    optional `user` fields.
/// 6. Resolve `command.container` (`<index>`, `*<relative>`, `<relative>`, or `<full_name>`)
///    against the recipe's container list.
/// 7. Build and execute (or simulate) `podman exec -it [-u <user>] <container_name> <execute>`.
///
/// # Errors
///
/// Returns [`PodError`] if `<recipe>` or `<command|index>` is missing, the recipe is not
/// registered, reading or validating the `.pod` file fails, the command or target container
/// cannot be resolved, or `podman exec` fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    execute_with(context, &mut stdout, &mut stderr)
}

/// Builds the `podman exec -it` argument vector for `command` targeting `container_name`.
#[must_use]
fn build_exec_args(command: &PodCommand, container_name: &str) -> Vec<String> {
    let token_count = command.execute.split_whitespace().count();
    let capacity = bounded_capacity(
        BASE_EXEC_ARG_COUNT.saturating_add(token_count),
        MAX_COLLECTION_PREALLOC,
    );
    let mut arguments = Vec::with_capacity(capacity);
    arguments.push(String::from("exec"));
    arguments.push(String::from("-it"));
    if let Some(user) = &command.user {
        arguments.push(String::from("-u"));
        arguments.push(user.clone());
    }
    arguments.push(String::from(container_name));
    arguments.extend(command.execute.split_whitespace().map(str::to_owned));
    arguments
}

/// Resolves, validates, and executes `command_spec` against an already loaded `recipe`.
///
/// # Errors
///
/// Returns [`PodError`] if `command_spec` cannot be resolved, the command definition is
/// incomplete or invalid, the target container does not exist, or `podman exec` fails.
fn exec_command<W: Write, E: Write>(
    recipe: &Recipe,
    command_spec: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    let command = resolve_command(recipe, command_spec, stderr)?;
    validate_command(command, recipe.pod.line)?;
    let container_name = recipe.resolve_container_name(&command.container)?;
    let arguments = build_exec_args(command, &container_name);
    let prefix = format!(
        "Execute command '{}' in container '{container_name}': ",
        command.execute
    );
    command::exec_with(
        "podman",
        &arguments,
        &ExecOptions {
            interactive: false,
            prefix: &prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Loads the target recipe from `context` and executes the requested command.
///
/// # Errors
///
/// Returns [`PodError`] if required CLI parameters are missing, the recipe is not registered
/// or fails to load, or command resolution/execution fails.
fn execute_with<W: Write, E: Write>(
    context: &Context,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    let (Some(target), Some(command_spec)) =
        (context.parameters.first(), context.parameters.get(1))
    else {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    };
    let recipe = load_target_recipe(context, target)?;
    exec_command(
        &recipe,
        command_spec,
        context.config.simulate,
        stdout,
        stderr,
    )
}

/// Constructs a [`PodError::Recipe`] with [`RecipeErrorKind::InvalidCommand`] at `line` (or without a line when `0`).
#[must_use]
fn recipe_command_error(line: usize, token: &str) -> PodError {
    if line == 0 {
        PodError::from(RecipeError::without_line(
            RecipeErrorKind::InvalidCommand,
            token,
        ))
    } else {
        PodError::from(RecipeError::with_token(
            RecipeErrorKind::InvalidCommand,
            line,
            token,
        ))
    }
}

/// Resolves `command_spec` by 1-based numeric index or exact command name from `recipe.pod.commands`.
///
/// # Errors
///
/// Returns [`PodError`] with [`RecipeErrorKind::InvalidCommand`] when no valid command matches.
fn resolve_command<'a, E: Write>(
    recipe: &'a Recipe,
    command_spec: &str,
    stderr: &mut E,
) -> Result<&'a PodCommand, PodError> {
    if let Some(index) = parse_ascii_u32(command_spec).and_then(|value| usize::try_from(value).ok())
    {
        let valid_commands = get_valid_commands(recipe, true, stderr);
        return index
            .checked_sub(1)
            .and_then(|zero_based| valid_commands.get(zero_based).copied())
            .ok_or_else(|| recipe_command_error(recipe.pod.line, command_spec));
    }
    recipe
        .pod
        .commands
        .get(command_spec)
        .filter(|command| is_valid_command_entry(command_spec, command))
        .ok_or_else(|| recipe_command_error(recipe.pod.line, command_spec))
}

/// Validates that `command` specifies safe, non-empty `container` and `execute` fields and an optional valid `user` token.
///
/// # Errors
///
/// Returns [`PodError`] with [`RecipeErrorKind::InvalidCommand`] if `container` or `execute`
/// is missing/invalid, `execute` starts with `-`, or `user` is not a valid token.
fn validate_command(command: &PodCommand, line: usize) -> Result<(), PodError> {
    if !is_valid_text(&command.container) {
        return Err(recipe_command_error(line, &command.name));
    }
    let execute = command.execute.trim();
    if !is_valid_text(execute) || execute.starts_with('-') {
        return Err(recipe_command_error(line, &command.name));
    }
    if let Some(user) = &command.user
        && !is_valid_token(user)
    {
        return Err(recipe_command_error(line, user));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::core::config::Config;
    use crate::core::context::Context;
    use crate::core::pod::PodCommand;
    use crate::core::recipe::Recipe;
    use crate::error::{CliErrorKind, PodError, RecipeErrorKind};
    use crate::io::system::fs;
    use super::{exec_command, execute_with, handle};

    const COMMANDS_RECIPE_PSL: &str = "\
pod cmd_pod {
    name \"Command Pod\"
    description \"A pod with commands.\"
    path /pods/cmd_pod
    registry docker.io

    command add_index {
        description \"Adds missing database indices.\"
        container *db
        user 33
        execute \"script.sh add-missing-indices\"
    }
    command run_absolute {
        container absolute_db
        execute \"script.sh run-absolute\"
    }
    command run_int_user {
        container absolute_db
        user 1000
        execute \"script.sh run-int-user\"
    }
    command run_int_container {
        container 1
        execute \"script.sh run-int-container\"
    }
    command run_int_container_2 {
        container 2
        execute \"script.sh run-int-container-2\"
    }
    command missing_container {
        execute \"script.sh no-container\"
    }
    command missing_execute {
        container *db
    }
    command missing_target_container {
        container *nonexistent
        execute \"script.sh\"
    }
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

    fn build_test_context(suffix: &str) -> (PathBuf, Context) {
        let mut directory = std::env::temp_dir();
        directory.push(format!(
            "podoxide_cmd_exec_{}_{}",
            std::process::id(),
            suffix
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();

        let recipe_path = directory.join("recipe_006_commands.pod");
        fs::write_file(&recipe_path, COMMANDS_RECIPE_PSL).unwrap();

        let mut config = Config::new();
        config.simulate = true;
        config.recipes.path = directory.clone();
        config.recipes.groups.insert(
            String::from("all"),
            vec![String::from("recipe_006_commands")],
        );

        let mut context = Context::new();
        context.config = config;
        (directory, context)
    }

    #[test]
    fn exec_command_rejects_flag_injection_and_handles_zero_line() {
        let mut recipe = Recipe::new();
        recipe.has_pod = true;
        recipe.pod.id = String::from("cmd_pod");
        recipe.pod.line = 0;

        let mut flag_exec = PodCommand::new();
        flag_exec.name = String::from("flag_exec");
        flag_exec.container = String::from("*db");
        flag_exec.execute = String::from("--privileged sh");
        recipe
            .pod
            .commands
            .insert(String::from("flag_exec"), flag_exec);

        let mut flag_user = PodCommand::new();
        flag_user.name = String::from("flag_user");
        flag_user.container = String::from("*db");
        flag_user.execute = String::from("sh");
        flag_user.user = Some(String::from("-u=0"));
        recipe
            .pod
            .commands
            .insert(String::from("flag_user"), flag_user);

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        for command_spec in ["flag_exec", "flag_user", "missing"] {
            let error =
                exec_command(&recipe, command_spec, true, &mut stdout, &mut stderr).unwrap_err();
            match error {
                PodError::Recipe(recipe_error) => {
                    assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidCommand);
                    assert_eq!(recipe_error.line, None);
                }
                other => panic!("expected PodError::Recipe(InvalidCommand), got {other:?}"),
            }
        }
    }

    #[test]
    fn execute_with_rejects_invalid_commands_and_missing_parameters() {
        let (directory, mut context) = build_test_context("errors");

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let empty_error = execute_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match empty_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe_006_commands")];
        let one_parameter_error = execute_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match one_parameter_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        for invalid_spec in ["missing_container", "missing_execute", "unknown", "0", "99"] {
            context.parameters = vec![
                String::from("recipe_006_commands"),
                String::from(invalid_spec),
            ];
            let error = execute_with(&context, &mut stdout, &mut stderr).unwrap_err();
            match error {
                PodError::Recipe(recipe_error) => {
                    assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidCommand);
                }
                other => panic!(
                    "expected PodError::Recipe(InvalidCommand) for {invalid_spec}, got {other:?}"
                ),
            }
        }

        context.parameters = vec![
            String::from("recipe_006_commands"),
            String::from("missing_target_container"),
        ];
        let container_error = execute_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match container_error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotFound);
            }
            other => panic!("expected PodError::Recipe(ContainerNotFound), got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn execute_with_simulates_named_and_indexed_commands() {
        let (directory, mut context) = build_test_context("simulate");

        let cases = [
            (
                "add_index",
                "Execute command 'script.sh add-missing-indices' in container 'cmd_pod-db': \npodman exec -it -u 33 cmd_pod-db script.sh add-missing-indices\n",
            ),
            (
                "run_absolute",
                "Execute command 'script.sh run-absolute' in container 'absolute_db': \npodman exec -it absolute_db script.sh run-absolute\n",
            ),
            (
                "run_int_user",
                "Execute command 'script.sh run-int-user' in container 'absolute_db': \npodman exec -it -u 1000 absolute_db script.sh run-int-user\n",
            ),
            (
                "run_int_container",
                "Execute command 'script.sh run-int-container' in container 'cmd_pod-1': \npodman exec -it cmd_pod-1 script.sh run-int-container\n",
            ),
            (
                "run_int_container_2",
                "Execute command 'script.sh run-int-container-2' in container 'cmd_pod-db': \npodman exec -it cmd_pod-db script.sh run-int-container-2\n",
            ),
            (
                "1",
                "Execute command 'script.sh add-missing-indices' in container 'cmd_pod-db': \npodman exec -it -u 33 cmd_pod-db script.sh add-missing-indices\n",
            ),
            (
                "5",
                "Execute command 'script.sh run-absolute' in container 'absolute_db': \npodman exec -it absolute_db script.sh run-absolute\n",
            ),
        ];

        for (command_spec, expected_output) in cases {
            context.parameters = vec![
                String::from("recipe_006_commands"),
                String::from(command_spec),
            ];
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            execute_with(&context, &mut stdout, &mut stderr).unwrap();
            assert_eq!(String::from_utf8(stdout).unwrap(), expected_output);
            assert!(stderr.is_empty());
        }

        context.parameters = vec![
            String::from("recipe_006_commands"),
            String::from("add_index"),
        ];
        assert!(handle(&context).is_ok());

        let _ = std::fs::remove_dir_all(&directory);
    }
}
