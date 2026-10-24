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

use crate::core::context::{Context, InitAction, ModeAction};
use crate::core::log;
use crate::error::{CliError, CliErrorKind, PodError};

/// Dispatches `init` mode execution to template creation (`create`) or `help`.
///
/// # Workflow
///
/// 1. Log that `init` mode is active when debug logging is enabled.
/// 2. If `--help` (`context.flags.help`) is set, route directly to [`help::handle`]
///    without reading any files.
/// 3. Route [`ModeAction::Init`] in enum declaration order (`Create`, `Help`)
///    to the corresponding action module ([`create::handle`], [`help::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `init` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Init mode is used.");
    if context.flags.help {
        return help::handle(context);
    }
    match context.action() {
        ModeAction::Init(InitAction::Create) => create::handle(context),
        ModeAction::Init(InitAction::Help) => help::handle(context),
        _ => Err(PodError::from(CliError::with_argument(
            CliErrorKind::UnknownAction,
            context.action().to_string(),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use crate::core::context::{Context, DefaultAction, InitAction, Mode, ModeAction};
    use crate::error::{CliErrorKind, PodError};
    use super::handle;

    #[test]
    fn handle_routes_init_actions_and_rejects_mismatched_action() {
        let mut help_context = Context::new();
        help_context.mode = Mode::Init;
        help_context.action = ModeAction::Init(InitAction::Help);
        assert!(handle(&help_context).is_ok());

        let mut flag_help_context = Context::new();
        flag_help_context.mode = Mode::Init;
        flag_help_context.action = ModeAction::Init(InitAction::Create);
        flag_help_context.flags.help = true;
        assert!(handle(&flag_help_context).is_ok());

        let mut create_context = Context::new();
        create_context.mode = Mode::Init;
        create_context.action = ModeAction::Init(InitAction::Create);
        let create_err = handle(&create_context).unwrap_err();
        assert!(
            create_err
                .to_string()
                .contains("File './recipe.pod' already exists!")
        );

        let mut invalid_context = Context::new();
        invalid_context.mode = Mode::Init;
        invalid_context.action = ModeAction::Default(DefaultAction::Help);
        let error = handle(&invalid_context).unwrap_err();
        match error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::UnknownAction),
            other => panic!("expected PodError::Cli(UnknownAction), got {other:?}"),
        }
    }
}
