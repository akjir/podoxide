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

use crate::core::context::{Context, DefaultAction, ModeAction};
use crate::core::log;
use crate::error::{CliError, CliErrorKind, PodError};

/// Dispatches default mode execution to the resolved lifecycle, status, or help action.
///
/// # Workflow
///
/// 1. Log that `default` mode is active when debug logging is enabled.
/// 2. If `--help` (`context.flags.help`) is set and the action is not [`DefaultAction::Status`],
///    route directly to [`help::handle`] without reading `pods.conf` or any `.pod` files.
///    (When `context.action` is [`DefaultAction::Status`] and `--help` is set, [`status::handle`]
///    routes to status-specific help without reading any files.)
/// 3. Route [`ModeAction::Default`] in enum declaration order (`Create`, `Down`, `Help`,
///    `Reload`, `Remove`, `Restart`, `Start`, `Status`, `Stop`, `Up`, `Update`) to the
///    corresponding action module.
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `default` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Default mode is used.");
    if context.flags.help && context.action() != ModeAction::Default(DefaultAction::Status) {
        return help::handle(context);
    }
    match context.action() {
        ModeAction::Default(DefaultAction::Create) => create::handle(context),
        ModeAction::Default(DefaultAction::Down) => down::handle(context),
        ModeAction::Default(DefaultAction::Help) => help::handle(context),
        ModeAction::Default(DefaultAction::Reload) => reload::handle(context),
        ModeAction::Default(DefaultAction::Remove) => remove::handle(context),
        ModeAction::Default(DefaultAction::Restart) => restart::handle(context),
        ModeAction::Default(DefaultAction::Start) => start::handle(context),
        ModeAction::Default(DefaultAction::Status) => status::handle(context),
        ModeAction::Default(DefaultAction::Stop) => stop::handle(context),
        ModeAction::Default(DefaultAction::Up) => up::handle(context),
        ModeAction::Default(DefaultAction::Update) => update::handle(context),
        _ => Err(PodError::from(CliError::with_argument(
            CliErrorKind::UnknownAction,
            context.action().to_string(),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use crate::core::context::{CommandAction, Context, DefaultAction, Mode, ModeAction};
    use crate::error::{CliErrorKind, PodError};
    use super::handle;

    #[test]
    fn handle_routes_default_actions_and_rejects_mismatched_action() {
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
        ];

        for action in actions {
            let mut context = Context::new();
            context.mode = Mode::Default;
            context.action = ModeAction::Default(action);
            assert!(handle(&context).is_ok());
        }

        let mut flag_help_context = Context::new();
        flag_help_context.mode = Mode::Default;
        flag_help_context.action = ModeAction::Default(DefaultAction::Up);
        flag_help_context.flags.help = true;
        assert!(handle(&flag_help_context).is_ok());

        let mut status_help_context = Context::new();
        status_help_context.mode = Mode::Default;
        status_help_context.action = ModeAction::Default(DefaultAction::Status);
        status_help_context.flags.help = true;
        assert!(handle(&status_help_context).is_ok());

        let mut invalid_context = Context::new();
        invalid_context.mode = Mode::Default;
        invalid_context.action = ModeAction::Command(CommandAction::Help);
        let error = handle(&invalid_context).unwrap_err();
        match error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::UnknownAction),
            other => panic!("expected PodError::Cli(UnknownAction), got {other:?}"),
        }
    }
}
