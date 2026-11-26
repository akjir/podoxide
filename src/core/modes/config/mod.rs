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
pub mod inspect;
pub mod show;

use crate::core::context::{ConfigAction, Context};
use crate::core::log;
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;

/// Dispatches `config` mode execution to the resolved sub-action (`edit`, `help`, `inspect`, or `show`).
///
/// # Workflow
///
/// 1. Log that `config` mode is active when debug logging is enabled.
/// 2. Route [`ConfigAction`] in enum declaration order (`Edit`, `Help`, `Inspect`, `Show`)
///    to the corresponding action module ([`edit::handle`], [`help::handle`], [`inspect::handle`], [`show::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if the selected action handler fails.
pub fn handle(context: &Context, action: ConfigAction) -> Result<ExecOutcome, PodError> {
    log::debug("Config mode is used.");
    match action {
        ConfigAction::Edit => edit::handle(context)?,
        ConfigAction::Help => help::handle(context)?,
        ConfigAction::Inspect => inspect::handle(context)?,
        ConfigAction::Show => show::handle(context)?,
    }
    Ok(ExecOutcome::Succeeded)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{ConfigAction, Context};
    use crate::io::exec::command::ExecOutcome;
    use super::handle;

    #[test]
    fn handle_routes_config_actions() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        for action in [
            ConfigAction::Edit,
            ConfigAction::Help,
            ConfigAction::Inspect,
            ConfigAction::Show,
        ] {
            let mut context = Context::new();
            context.config = root_config.clone();
            assert_eq!(
                handle(&context, action).unwrap(),
                ExecOutcome::Succeeded,
                "expected config::{action} to succeed"
            );
        }
    }
}
