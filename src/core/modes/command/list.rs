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

//! Handler for `pods command list <recipe>`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::core::pod::PodCommand;
use crate::core::recipe::Recipe;
use crate::error::{CliError, CliErrorKind, PodError};
use super::{get_valid_commands, load_target_recipe};

/// Collects, sorts, and displays all valid maintenance commands defined in a target recipe.
///
/// # Workflow
///
/// 1. Extract `<recipe>` (`context.parameters[0]`).
/// 2. Verify that `<recipe>` is registered in `context.config.recipes.groups`.
/// 3. Read and validate only the single targeted recipe file (`<recipes.path>/<recipe>.pod`).
///    Do not read any other `.pod` files in the directory.
/// 4. If `recipe.pod.commands` is empty, print `"There are no commands defined in recipe '<recipe>'."`
///    and return `Ok(())`.
/// 5. Collect all valid command blocks, sort them alphabetically by command name, and emit a
///    warning (`log::write_warning`) for any command missing a `description` directive.
/// 6. Print the 1-based numbered command list (`"  <idx>) <name>: <description>"`).
///
/// # Errors
///
/// Returns [`PodError`] if `<recipe>` is missing, not registered in `pods.conf`, or if loading
/// and validating `<recipe>.pod` fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    list_with(context, &mut stdout, &mut stderr)
}

/// Formats a single 1-based indexed command entry for `pods command list`.
#[must_use]
fn format_command_entry(command: &PodCommand, index: usize, total: usize) -> String {
    let prefix = if total > 9 && index < 10 {
        format!(" {index})")
    } else {
        format!("{index})")
    };
    let description = command.description.trim();
    if description.is_empty() {
        format!("  {prefix} {}", command.name)
    } else {
        format!("  {prefix} {}: {description}", command.name)
    }
}

/// Formats and writes the command list for an already loaded `recipe` to `stdout` and `stderr`.
fn list_commands<W: Write, E: Write>(
    recipe: &Recipe,
    target: &str,
    stdout: &mut W,
    stderr: &mut E,
) {
    if recipe.pod.commands.is_empty() {
        log::write_print(
            stdout,
            &format!("There are no commands defined in recipe '{target}'."),
        );
        return;
    }
    let valid_commands = get_valid_commands(recipe, false, stderr);
    if valid_commands.is_empty() {
        log::write_print(
            stdout,
            &format!("There is no valid command in recipe '{target}'."),
        );
        return;
    }
    log::write_print(stdout, &format!("Commands for recipe '{target}':"));
    let total = valid_commands.len();
    for (zero_based, command) in valid_commands.iter().enumerate() {
        let line = format_command_entry(command, zero_based + 1, total);
        log::write_print(stdout, &line);
    }
}

/// Loads the target recipe from `context` and writes its command list to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if `context.parameters` is empty, the recipe is not registered in
/// `context.config`, or loading and validating the `.pod` file fails.
fn list_with<W: Write, E: Write>(
    context: &Context,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    let Some(target) = context.parameters.first() else {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    };
    let recipe = load_target_recipe(context, target)?;
    list_commands(&recipe, target, stdout, stderr);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::str::FromStr;

    use crate::core::config::Config;
    use crate::core::context::Context;
    use crate::core::pod::PodCommand;
    use crate::core::recipe::Recipe;
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError};
    use crate::io::system::fs;
    use super::{handle, list_commands, list_with};

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

    fn build_temp_directory(suffix: &str) -> PathBuf {
        let mut directory = std::env::temp_dir();
        directory.push(format!(
            "podoxide_cmd_list_{}_{}",
            std::process::id(),
            suffix
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn list_commands_handles_empty_invalid_and_padded_lists() {
        let empty_recipe = Recipe::new();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        list_commands(&empty_recipe, "recipe_011", &mut stdout, &mut stderr);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "There are no commands defined in recipe 'recipe_011'.\n"
        );
        assert!(stderr.is_empty());

        let mut invalid_recipe = Recipe::new();
        invalid_recipe
            .pod
            .commands
            .insert(String::from("invalid_cmd"), PodCommand::new());
        let mut stdout = Vec::new();
        list_commands(&invalid_recipe, "test_recipe", &mut stdout, &mut stderr);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "There is no valid command in recipe 'test_recipe'.\n"
        );

        let mut ten_recipe = Recipe::new();
        for index in 1..=10 {
            let name = format!("cmd_{index:02}");
            let mut command = PodCommand::new();
            command.name = name.clone();
            command.container = String::from("*db");
            command.execute = String::from("true");
            if index == 1 {
                command.description = String::from("First command.");
            }
            ten_recipe.pod.commands.insert(name, command);
        }
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        list_commands(&ten_recipe, "ten_recipe", &mut stdout, &mut stderr);
        let stdout_text = String::from_utf8(stdout).unwrap();
        assert!(stdout_text.contains("Commands for recipe 'ten_recipe':\n"));
        assert!(stdout_text.contains("   1) cmd_01: First command.\n"));
        assert!(stdout_text.contains("   9) cmd_09\n"));
        assert!(stdout_text.contains("  10) cmd_10\n"));
    }

    #[test]
    fn list_with_outputs_sorted_commands_and_warnings() {
        let directory = build_temp_directory("sorted");
        let recipe_path = directory.join("recipe_006_commands.pod");
        fs::write_file(&recipe_path, COMMANDS_RECIPE_PSL).unwrap();

        let mut config = Config::new();
        config.recipes.path = directory.clone();
        config.recipes.groups.insert(
            String::from("all"),
            vec![String::from("recipe_006_commands")],
        );

        let mut context = Context::new();
        context.config = config;
        context.parameters = vec![String::from("recipe_006_commands")];

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        list_with(&context, &mut stdout, &mut stderr).unwrap();

        let stdout_text = String::from_utf8(stdout).unwrap();
        let stderr_text = String::from_utf8(stderr).unwrap();

        assert!(stderr_text.contains("WARNING: Command 'missing_container' has no description.\n"));
        assert!(stderr_text.contains("WARNING: Command 'missing_execute' has no description.\n"));
        assert!(stderr_text.contains("WARNING: Command 'run_absolute' has no description.\n"));
        assert!(stderr_text.contains("WARNING: Command 'run_int_container' has no description.\n"));
        assert!(
            stderr_text.contains("WARNING: Command 'run_int_container_2' has no description.\n")
        );
        assert!(stderr_text.contains("WARNING: Command 'run_int_user' has no description.\n"));
        assert!(!stderr_text.contains("add_index"));

        let expected_list = "\
Commands for recipe 'recipe_006_commands':
  1) add_index: Adds missing database indices.
  2) missing_container
  3) missing_execute
  4) run_absolute
  5) run_int_container
  6) run_int_container_2
  7) run_int_user
";
        assert_eq!(stdout_text, expected_list);

        assert!(handle(&context).is_ok());
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn list_with_rejects_missing_and_unregistered_targets() {
        let mut context = Context::new();
        context.config = Config::from_str("recipes {\n    groups {\n        all recipe\n    }\n}")
            .unwrap();

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let empty_error = list_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match empty_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("unknown")];
        let unknown_error = list_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match unknown_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }
    }
}
