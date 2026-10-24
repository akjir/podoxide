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
/// 2. If `--help` (`context.flags.help`) is set, route directly to [`help::handle`]
///    without reading `pods.conf` or any `.pod` recipe files.
/// 3. Route [`ModeAction::Connect`] in enum declaration order (`Help`, `Shell`)
///    to the corresponding action module ([`help::handle`], [`shell::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `connect` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Connect mode is used.");
    if context.flags.help {
        return help::handle(context);
    }
    match context.action() {
        ModeAction::Connect(ConnectAction::Help) => help::handle(context),
        ModeAction::Connect(ConnectAction::Shell) => shell::handle(context),
        _ => Err(PodError::from(CliError::with_argument(
            CliErrorKind::UnknownAction,
            context.action().to_string(),
        ))),
    }
}
