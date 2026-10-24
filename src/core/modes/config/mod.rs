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

//! Config mode (`pods config` / `pods cfg`) action dispatcher.

pub mod edit;
pub mod help;
pub mod show;

use crate::core::context::{ConfigAction, Context, ModeAction};
use crate::core::log;
use crate::error::{CliError, CliErrorKind, PodError};

/// Dispatches `config` mode execution to the resolved sub-action (`edit`, `help`, or `show`).
///
/// # Workflow
///
/// 1. Log that `config` mode is active when debug logging is enabled.
/// 2. Route [`ModeAction::Config`] in enum declaration order (`Edit`, `Help`, `Show`)
///    to the corresponding action module ([`edit::handle`], [`help::handle`], [`show::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `config` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Config mode is used.");
    match context.action() {
        ModeAction::Config(ConfigAction::Edit) => edit::handle(context),
        ModeAction::Config(ConfigAction::Help) => help::handle(context),
        ModeAction::Config(ConfigAction::Show) => show::handle(context),
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
    use crate::core::context::{ConfigAction, Context, DefaultAction, Mode, ModeAction};
    use crate::error::{CliErrorKind, PodError};
    use super::handle;

    #[test]
    fn handle_routes_config_actions_and_rejects_mismatched_action() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        for action in [ConfigAction::Edit, ConfigAction::Help, ConfigAction::Show] {
            let mut context = Context::new();
            context.config = root_config.clone();
            context.mode = Mode::Config;
            context.action = ModeAction::Config(action);
            assert!(handle(&context).is_ok());
        }

        let mut invalid_context = Context::new();
        invalid_context.mode = Mode::Config;
        invalid_context.action = ModeAction::Default(DefaultAction::Help);
        let error = handle(&invalid_context).unwrap_err();
        match error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::UnknownAction),
            other => panic!("expected PodError::Cli(UnknownAction), got {other:?}"),
        }
    }
}
