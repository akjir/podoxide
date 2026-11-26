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

//! Init mode (`pods init`) action dispatcher.

pub mod create;
pub mod help;

use crate::core::context::{Context, InitAction};
use crate::core::log;
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;

/// Dispatches `init` mode execution to template creation (`create`) or `help`.
///
/// # Workflow
///
/// 1. Log that `init` mode is active when debug logging is enabled.
/// 2. Route [`InitAction`] in enum declaration order (`Create`, `Help`)
///    to the corresponding action module ([`create::handle`], [`help::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if the selected action handler fails.
pub fn handle(context: &Context, action: InitAction) -> Result<ExecOutcome, PodError> {
    log::debug("Init mode is used.");
    match action {
        InitAction::Create => create::handle(context)?,
        InitAction::Help => help::handle(context)?,
    }
    Ok(ExecOutcome::Succeeded)
}

#[cfg(test)]
mod tests {
    use crate::core::context::{Context, InitAction};
    use crate::io::exec::command::ExecOutcome;
    use super::handle;

    #[test]
    fn handle_routes_init_actions() {
        let context = Context::new();
        assert_eq!(
            handle(&context, InitAction::Help).unwrap(),
            ExecOutcome::Succeeded
        );
        assert_eq!(
            handle(&context, InitAction::Create).unwrap(),
            ExecOutcome::Succeeded
        );
    }
}
