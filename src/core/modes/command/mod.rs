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

use crate::core::context::{CommandAction, Context, ModeAction};
use crate::core::log;
use crate::error::{CliError, CliErrorKind, PodError};

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
