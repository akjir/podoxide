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
/// 2. If `--help` (`context.flags.help`) is set, route directly to [`help::handle`]
///    without reading `pods.conf` or any `.pod` recipe files.
/// 3. Route [`ModeAction::Command`] in enum declaration order (`Exec`, `Help`, `List`)
///    to the corresponding action module ([`exec::handle`], [`help::handle`], [`list::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `command` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Command mode is used.");
    if context.flags.help {
        return help::handle(context);
    }
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
