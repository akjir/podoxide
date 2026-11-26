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

//! Command mode (`pods command` / `pods cmd`) action dispatcher.

pub mod exec;
pub mod help;
pub mod list;

use std::io::Write;

use crate::core::context::{CommandAction, Context};
use crate::core::log;
use crate::core::pod::PodCommand;
use crate::core::recipe::{Recipe, tokenizer};
use crate::error::{ConfigError, ConfigErrorKind, PodError};
use crate::io::exec::command::ExecOutcome;
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::string;

/// Dispatches `command` mode execution to the resolved sub-action (`exec`, `help`, or `list`).
///
/// # Workflow
///
/// 1. Log that `command` mode is active when debug logging is enabled.
/// 2. Route [`CommandAction`] in enum declaration order (`Exec`, `Help`, `List`)
///    to the corresponding action module ([`exec::handle`], [`help::handle`], [`list::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if the selected action handler fails.
pub fn handle(context: &Context, action: CommandAction) -> Result<ExecOutcome, PodError> {
    log::debug("Command mode is used.");
    match action {
        CommandAction::Exec => exec::handle(context),
        CommandAction::Help => {
            help::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
        CommandAction::List => {
            list::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
    }
}

/// Collects all valid [`PodCommand`] definitions from `recipe.pod.commands` in alphabetical order.
///
/// When `suppress_warnings` is `false`, emits a warning to `stderr` for each valid command
/// whose `description` is empty or whitespace-only.
#[must_use]
pub(super) fn get_valid_commands<'a, E: Write>(
    recipe: &'a Recipe,
    suppress_warnings: bool,
    stderr: &mut E,
) -> Vec<&'a PodCommand> {
    if recipe.pod.commands.is_empty() {
        return Vec::new();
    }
    let capacity = collection::bounded_capacity(recipe.pod.commands.len(), MAX_COLLECTION_PREALLOC);
    let mut valid_commands = Vec::with_capacity(capacity);
    for (name, command) in &recipe.pod.commands {
        if !is_valid_command_entry(name, command) {
            continue;
        }
        if !suppress_warnings && command.description.trim().is_empty() {
            log::write_warning(stderr, &format!("Command '{name}' has no description."));
        }
        valid_commands.push(command);
    }
    valid_commands
}

/// Evaluates whether `(key, command)` represents a populated, well-formed command entry matching [`Recipe::inspect`].
#[must_use]
pub(super) fn is_valid_command_entry(key: &str, command: &PodCommand) -> bool {
    string::is_valid_identifier(key)
        && key == command.name
        && string::is_valid_text(&command.container)
        && tokenizer::validate_execute_tokens(&command.execute, key, 0).is_ok()
        && (command.description.is_empty() || string::is_valid_text(&command.description))
        && command.user.as_deref().is_none_or(string::is_valid_token)
}

/// Verifies that `target` is registered in `context.config` and loads and validates `<target>.pod`.
///
/// # Errors
///
/// Returns [`PodError`] if `target` is not registered in `context.config.recipes.groups`, or if
/// loading or validating `<recipes.path>/<target>.pod` fails.
pub(super) fn load_target_recipe(context: &Context, target: &str) -> Result<Recipe, PodError> {
    if !context.config.has_recipe(target) {
        return Err(PodError::from(ConfigError::without_line(
            ConfigErrorKind::RecipeNotFound,
            target,
        )));
    }
    Recipe::inspect(&context.config, target)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{CommandAction, Context};
    use crate::core::pod::PodCommand;
    use crate::core::recipe::Recipe;
    use crate::error::{ConfigErrorKind, PodError};
    use crate::io::exec::command::ExecOutcome;
    use super::{get_valid_commands, handle, is_valid_command_entry, load_target_recipe};

    #[test]
    fn get_valid_commands_filters_invalid_entries_and_controls_warnings() {
        let mut recipe = Recipe::new();
        let mut stderr = Vec::new();
        assert!(get_valid_commands(&recipe, false, &mut stderr).is_empty());
        assert!(stderr.is_empty());

        let mut valid_with_description = PodCommand::new();
        valid_with_description.name = String::from("add_index");
        valid_with_description.description = String::from("Adds missing database indices.");
        valid_with_description.container = String::from("*db");
        valid_with_description.execute = String::from("script.sh");
        recipe
            .pod
            .commands
            .insert(String::from("add_index"), valid_with_description);

        let mut valid_without_description = PodCommand::new();
        valid_without_description.name = String::from("run_absolute");
        valid_without_description.container = String::from("absolute_db");
        valid_without_description.execute = String::from("script.sh");
        recipe
            .pod
            .commands
            .insert(String::from("run_absolute"), valid_without_description);

        let mut empty_command = PodCommand::new();
        empty_command.name = String::from("empty_command");
        recipe
            .pod
            .commands
            .insert(String::from("empty_command"), empty_command);

        let mut mismatched_command = PodCommand::new();
        mismatched_command.name = String::from("other_name");
        mismatched_command.execute = String::from("script.sh");
        recipe
            .pod
            .commands
            .insert(String::from("mismatch"), mismatched_command);

        assert!(!is_valid_command_entry("1bad", &PodCommand::new()));

        for bad_execute in [
            "--privileged sh",
            "'--privileged' sh",
            "sh -c 'unclosed",
            "'' arg",
            "'my script.sh' arg",
        ] {
            let mut malformed = PodCommand::new();
            malformed.name = String::from("malformed");
            malformed.container = String::from("*db");
            malformed.execute = String::from(bad_execute);
            assert!(
                !is_valid_command_entry("malformed", &malformed),
                "expected malformed execute {bad_execute:?} to be rejected"
            );
        }

        let commands = get_valid_commands(&recipe, false, &mut stderr);
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0].name, "add_index");
        assert_eq!(commands[1].name, "run_absolute");
        let warnings = String::from_utf8(stderr).unwrap();
        assert!(warnings.contains("WARNING: Command 'run_absolute' has no description."));
        assert!(!warnings.contains("add_index"));

        let mut suppressed_stderr = Vec::new();
        let commands_suppressed = get_valid_commands(&recipe, true, &mut suppressed_stderr);
        assert_eq!(commands_suppressed.len(), 2);
        assert!(suppressed_stderr.is_empty());
    }

    #[test]
    fn handle_routes_command_actions() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        let help_context = Context::new();
        assert_eq!(
            handle(&help_context, CommandAction::Help).unwrap(),
            ExecOutcome::Succeeded
        );

        let mut list_context = Context::new();
        list_context.config = root_config.clone();
        list_context.parameters = vec![String::from("recipe")];
        assert_eq!(
            handle(&list_context, CommandAction::List).unwrap(),
            ExecOutcome::Succeeded
        );

        let mut exec_context = Context::new();
        exec_context.config = root_config;
        exec_context.parameters = vec![String::from("recipe"), String::from("migrate")];
        assert_eq!(
            handle(&exec_context, CommandAction::Exec).unwrap(),
            ExecOutcome::Succeeded
        );
    }

    #[test]
    fn load_target_recipe_rejects_unregistered_recipe() {
        let mut context = Context::new();
        context.config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        let error = load_target_recipe(&context, "unknown_recipe").unwrap_err();
        match error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }
    }
}
