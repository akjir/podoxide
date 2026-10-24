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

//! Connect mode (`pods connect` / `pods con`) action dispatcher.

pub mod help;
pub mod shell;

use crate::core::context::{ConnectAction, Context, ModeAction};
use crate::core::log;
use crate::error::{CliError, CliErrorKind, PodError};

/// Dispatches `connect` mode execution to the resolved sub-action (`help` or `shell`).
///
/// # Workflow
///
/// 1. Log that `connect` mode is active when debug logging is enabled.
/// 2. Route [`ModeAction::Connect`] in enum declaration order (`Help`, `Shell`)
///    to the corresponding action module ([`help::handle`], [`shell::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `connect` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Connect mode is used.");
    match context.action() {
        ModeAction::Connect(ConnectAction::Help) => help::handle(context),
        ModeAction::Connect(ConnectAction::Shell) => shell::handle(context),
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
    use crate::core::context::{ConnectAction, Context, DefaultAction, Mode, ModeAction};
    use crate::error::{CliErrorKind, PodError};
    use super::handle;

    #[test]
    fn handle_routes_connect_actions_and_rejects_mismatched_action() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        let mut help_context = Context::new();
        help_context.mode = Mode::Connect;
        help_context.action = ModeAction::Connect(ConnectAction::Help);
        assert!(handle(&help_context).is_ok());

        let mut shell_context = Context::new();
        shell_context.config = root_config;
        shell_context.mode = Mode::Connect;
        shell_context.action = ModeAction::Connect(ConnectAction::Shell);
        shell_context.parameters = vec![String::from("recipe/db")];
        assert!(handle(&shell_context).is_ok());

        let mut invalid_context = Context::new();
        invalid_context.mode = Mode::Connect;
        invalid_context.action = ModeAction::Default(DefaultAction::Help);
        let error = handle(&invalid_context).unwrap_err();
        match error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::UnknownAction),
            other => panic!("expected PodError::Cli(UnknownAction), got {other:?}"),
        }
    }
}
