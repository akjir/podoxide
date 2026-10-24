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

//! Default mode (`pods <action> [targets...]`) action dispatcher.

pub mod create;
pub mod down;
pub mod help;
pub mod reload;
pub mod remove;
pub mod restart;
pub mod start;
pub mod status;
pub mod stop;
pub mod up;
pub mod update;
pub mod version;

use crate::core::context::{Context, DefaultAction, ModeAction};
use crate::core::log;
use crate::core::recipe::Recipe;
use crate::error::{CliError, CliErrorKind, PodError};

/// Expands `@<group>` and `<recipe>` targets from `context.parameters`, loads and validates each targeted `.pod` recipe, and executes `action`.
///
/// # Errors
///
/// Returns [`PodError`] if `context.parameters` is empty, target expansion fails, a targeted `.pod` recipe file cannot be read or validated, or `action` fails.
pub fn execute_lifecycle_action(
    context: &Context,
    action: fn(&Recipe, bool) -> Result<(), PodError>,
) -> Result<(), PodError> {
    if context.parameters.is_empty() {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    }
    let targets = context.config.expand_targets(&context.parameters)?;
    for target in &targets {
        let mut recipe = Recipe::load(&context.config.recipes.path, target)?;
        recipe.validate(&context.config, target)?;
        action(&recipe, context.config.simulate)?;
    }
    Ok(())
}

/// Dispatches default mode execution to the resolved lifecycle, status, help, or version action.
///
/// # Workflow
///
/// 1. Log that `default` mode is active when debug logging is enabled.
/// 2. Route [`ModeAction::Default`] in enum declaration order (`Create`, `Down`, `Help`,
///    `Reload`, `Remove`, `Restart`, `Start`, `Status`, `Stop`, `Up`, `Update`, `Version`)
///    to the corresponding action module.
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `default` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Default mode is used.");
    match context.action() {
        ModeAction::Default(DefaultAction::Create) => create::handle(context),
        ModeAction::Default(DefaultAction::Down) => down::handle(context),
        ModeAction::Default(DefaultAction::Help) => help::handle(context),
        ModeAction::Default(DefaultAction::Reload) => reload::handle(context),
        ModeAction::Default(DefaultAction::Remove) => remove::handle(context),
        ModeAction::Default(DefaultAction::Restart) => restart::handle(context),
        ModeAction::Default(DefaultAction::Start) => start::handle(context),
        ModeAction::Default(DefaultAction::Status) => status::handle(context),
        ModeAction::Default(DefaultAction::Stop) => stop::handle(context),
        ModeAction::Default(DefaultAction::Up) => up::handle(context),
        ModeAction::Default(DefaultAction::Update) => update::handle(context),
        ModeAction::Default(DefaultAction::Version) => version::handle(context),
        _ => Err(PodError::from(CliError::with_argument(
            CliErrorKind::UnknownAction,
            context.action().to_string(),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{CommandAction, Context, DefaultAction, Mode, ModeAction};
    use crate::core::pod;
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError};
    use super::{execute_lifecycle_action, handle};

    #[test]
    fn execute_lifecycle_action_rejects_empty_and_unknown_targets() {
        let mut context = Context::new();
        context.config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        let empty_error = execute_lifecycle_action(&context, pod::up).unwrap_err();
        match empty_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("unknown_recipe")];
        let unknown_error = execute_lifecycle_action(&context, pod::up).unwrap_err();
        match unknown_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }
    }

    #[test]
    fn handle_routes_default_actions_and_rejects_mismatched_action() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();
        let actions = [
            DefaultAction::Create,
            DefaultAction::Down,
            DefaultAction::Help,
            DefaultAction::Reload,
            DefaultAction::Remove,
            DefaultAction::Restart,
            DefaultAction::Start,
            DefaultAction::Status,
            DefaultAction::Stop,
            DefaultAction::Up,
            DefaultAction::Update,
            DefaultAction::Version,
        ];

        for action in actions {
            let mut context = Context::new();
            context.config = root_config.clone();
            context.mode = Mode::Default;
            context.action = ModeAction::Default(action);
            if action.requires_targets() {
                context.parameters = vec![String::from("recipe")];
            }
            assert!(handle(&context).is_ok());
        }

        let mut invalid_context = Context::new();
        invalid_context.mode = Mode::Default;
        invalid_context.action = ModeAction::Command(CommandAction::Help);
        let error = handle(&invalid_context).unwrap_err();
        match error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::UnknownAction),
            other => panic!("expected PodError::Cli(UnknownAction), got {other:?}"),
        }
    }
}
