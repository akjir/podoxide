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
pub mod prune;
pub mod update;

use crate::core::context::{Context, ImageAction, ModeAction};
use crate::core::log;
use crate::error::{CliError, CliErrorKind, PodError};

/// Dispatches `image` mode execution to the resolved sub-action (`help`, `prune`, or `update`).
///
/// # Workflow
///
/// 1. Log that `image` mode is active when debug logging is enabled.
/// 2. Route [`ModeAction::Image`] in enum declaration order (`Help`, `Prune`, `Update`)
///    to the corresponding action module ([`help::handle`], [`prune::handle`], [`update::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `image` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Image mode is used.");
    match context.action() {
        ModeAction::Image(ImageAction::Help) => help::handle(context),
        ModeAction::Image(ImageAction::Prune) => prune::handle(context),
        ModeAction::Image(ImageAction::Update) => update::handle(context),
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
    use crate::core::context::{Context, DefaultAction, ImageAction, Mode, ModeAction};
    use crate::error::{CliErrorKind, PodError};
    use super::handle;

    #[test]
    fn handle_routes_image_actions_and_rejects_mismatched_action() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        for action in [ImageAction::Help, ImageAction::Prune, ImageAction::Update] {
            let mut context = Context::new();
            context.config = root_config.clone();
            context.mode = Mode::Image;
            context.action = ModeAction::Image(action);
            if action == ImageAction::Update {
                context.parameters = vec![String::from("recipe")];
            }
            assert!(handle(&context).is_ok());
        }

        let mut invalid_context = Context::new();
        invalid_context.mode = Mode::Image;
        invalid_context.action = ModeAction::Default(DefaultAction::Help);
        let error = handle(&invalid_context).unwrap_err();
        match error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::UnknownAction),
            other => panic!("expected PodError::Cli(UnknownAction), got {other:?}"),
        }
    }
}
