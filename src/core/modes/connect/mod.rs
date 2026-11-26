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

use crate::core::context::{ConnectAction, Context};
use crate::core::log;
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;

/// Dispatches `connect` mode execution to the resolved sub-action (`help` or `shell`).
///
/// # Workflow
///
/// 1. Log that `connect` mode is active when debug logging is enabled.
/// 2. Route [`ConnectAction`] in enum declaration order (`Help`, `Shell`)
///    to the corresponding action module ([`help::handle`], [`shell::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if the selected action handler fails.
pub fn handle(context: &Context, action: ConnectAction) -> Result<ExecOutcome, PodError> {
    log::debug("Connect mode is used.");
    match action {
        ConnectAction::Help => {
            help::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
        ConnectAction::Shell => shell::handle(context),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{ConnectAction, Context};
    use crate::io::exec::command::ExecOutcome;
    use super::handle;

    #[test]
    fn handle_routes_connect_actions() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        let help_context = Context::new();
        assert_eq!(
            handle(&help_context, ConnectAction::Help).unwrap(),
            ExecOutcome::Succeeded
        );

        let mut shell_context = Context::new();
        shell_context.config = root_config;
        shell_context.parameters = vec![String::from("recipe/db")];
        assert_eq!(
            handle(&shell_context, ConnectAction::Shell).unwrap(),
            ExecOutcome::Succeeded
        );
    }
}
