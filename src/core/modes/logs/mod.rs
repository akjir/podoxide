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
mod runner;
pub mod show;

use crate::core::context::{Context, LogsAction};
use crate::core::log;
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;

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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{Context, LogsAction};
    use crate::io::exec::command::ExecOutcome;
    use super::handle;

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
