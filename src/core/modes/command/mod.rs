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

use crate::core::context::{CommandAction, Context, ModeAction};
use crate::core::log;
use crate::core::pod::PodCommand;
use crate::core::recipe::Recipe;
use crate::error::{CliError, CliErrorKind, ConfigError, ConfigErrorKind, PodError};
use crate::utilities::collection::{MAX_COLLECTION_PREALLOC, bounded_capacity};
use crate::utilities::string::is_valid_identifier;

/// Dispatches `command` mode execution to the resolved sub-action (`exec`, `help`, or `list`).
///
/// # Workflow
///
/// 1. Log that `command` mode is active when debug logging is enabled.
/// 2. Route [`ModeAction::Command`] in enum declaration order (`Exec`, `Help`, `List`)
///    to the corresponding action module ([`exec::handle`], [`help::handle`], [`list::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `command` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Command mode is used.");
    match context.action() {
        ModeAction::Command(CommandAction::Exec) => exec::handle(context),
        ModeAction::Command(CommandAction::Help) => help::handle(context),
        ModeAction::Command(CommandAction::List) => list::handle(context),
        _ => Err(PodError::from(CliError::with_argument(
            CliErrorKind::UnknownAction,
            context.action().to_string(),
        ))),
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
    let capacity = bounded_capacity(recipe.pod.commands.len(), MAX_COLLECTION_PREALLOC);
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

/// Evaluates whether `(key, command)` represents a populated, well-formed command entry.
#[must_use]
pub(super) fn is_valid_command_entry(key: &str, command: &PodCommand) -> bool {
    if !is_valid_identifier(key) || key != command.name {
        return false;
    }
    !command.container.is_empty()
        || !command.description.is_empty()
        || !command.execute.is_empty()
        || command.user.is_some()
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
    use crate::core::context::{CommandAction, Context, DefaultAction, Mode, ModeAction};
    use crate::core::pod::PodCommand;
    use crate::core::recipe::Recipe;
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError};
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
    fn handle_routes_command_actions_and_rejects_mismatched_action() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        let mut help_context = Context::new();
        help_context.mode = Mode::Command;
        help_context.action = ModeAction::Command(CommandAction::Help);
        assert!(handle(&help_context).is_ok());

        let mut list_context = Context::new();
        list_context.config = root_config.clone();
        list_context.mode = Mode::Command;
        list_context.action = ModeAction::Command(CommandAction::List);
        list_context.parameters = vec![String::from("recipe")];
        assert!(handle(&list_context).is_ok());

        let mut exec_context = Context::new();
        exec_context.config = root_config;
        exec_context.mode = Mode::Command;
        exec_context.action = ModeAction::Command(CommandAction::Exec);
        exec_context.parameters = vec![String::from("recipe"), String::from("migrate")];
        assert!(handle(&exec_context).is_ok());

        let mut invalid_context = Context::new();
        invalid_context.mode = Mode::Command;
        invalid_context.action = ModeAction::Default(DefaultAction::Help);
        let error = handle(&invalid_context).unwrap_err();
        match error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::UnknownAction),
            other => panic!("expected PodError::Cli(UnknownAction), got {other:?}"),
        }
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
