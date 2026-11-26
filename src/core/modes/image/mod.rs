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

//! Image mode (`pods image` / `pods img`) action dispatcher.

pub mod help;
mod preview;
pub mod prune;
pub mod update;

use crate::core::context::{Context, ImageAction};
use crate::core::log;
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;

/// Dispatches `image` mode execution to the resolved sub-action (`help`, `prune`, or `update`).
///
/// # Workflow
///
/// 1. Log that `image` mode is active when debug logging is enabled.
/// 2. Route [`ImageAction`] in enum declaration order (`Help`, `Prune`, `Update`)
///    to the corresponding action module ([`help::handle`], [`prune::handle`], [`update::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if the selected action handler fails.
pub fn handle(context: &Context, action: ImageAction) -> Result<ExecOutcome, PodError> {
    log::debug("Image mode is used.");
    match action {
        ImageAction::Help => {
            help::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
        ImageAction::Prune => prune::handle(context),
        ImageAction::Update => update::handle(context),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{Context, ImageAction};
    use crate::io::exec::command::ExecOutcome;
    use super::handle;

    #[test]
    fn handle_routes_image_actions() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        for action in [ImageAction::Help, ImageAction::Prune, ImageAction::Update] {
            let mut context = Context::new();
            context.config = root_config.clone();
            if action == ImageAction::Update {
                context.parameters = vec![String::from("recipe")];
            }
            assert_eq!(
                handle(&context, action).unwrap(),
                ExecOutcome::Succeeded,
                "expected image::{action} to succeed"
            );
        }
    }
}
