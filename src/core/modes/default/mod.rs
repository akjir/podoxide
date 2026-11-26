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

use crate::core::context::{Context, DefaultAction};
use crate::core::log;
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;

/// Dispatches default mode execution to the resolved lifecycle, status, help, or version action.
///
/// # Workflow
///
/// 1. Log that `default` mode is active when debug logging is enabled.
/// 2. Route [`DefaultAction`] in enum declaration order (`Create`, `Down`, `Help`,
///    `Reload`, `Remove`, `Restart`, `Start`, `Status`, `Stop`, `Up`, `Update`, `Version`)
///    to the corresponding action module.
///
/// # Errors
///
/// Returns [`PodError`] if the selected action handler fails.
pub fn handle(context: &Context, action: DefaultAction) -> Result<ExecOutcome, PodError> {
    log::debug("Default mode is used.");
    match action {
        DefaultAction::Create => create::handle(context),
        DefaultAction::Down => down::handle(context),
        DefaultAction::Help => {
            help::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
        DefaultAction::Reload => reload::handle(context),
        DefaultAction::Remove => remove::handle(context),
        DefaultAction::Restart => restart::handle(context),
        DefaultAction::Start => start::handle(context),
        DefaultAction::Status => {
            status::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
        DefaultAction::Stop => stop::handle(context),
        DefaultAction::Up => up::handle(context),
        DefaultAction::Update => update::handle(context),
        DefaultAction::Version => {
            version::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{Context, DefaultAction};
    use crate::io::exec::command::ExecOutcome;
    use super::handle;

    #[test]
    fn handle_routes_default_actions() {
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
            if action.requires_targets() {
                context.parameters = vec![String::from("recipe")];
            }
            assert_eq!(
                handle(&context, action).unwrap(),
                ExecOutcome::Succeeded,
                "expected default::{action} to succeed"
            );
        }
    }
}
