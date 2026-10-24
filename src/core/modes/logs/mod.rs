//! Logs mode (`pods logs` / `pods log`) action dispatcher.

pub mod follow;
pub mod help;
pub mod show;

use crate::core::context::{Context, LogsAction, ModeAction};
use crate::core::log;
use crate::error::{CliError, CliErrorKind, PodError};

/// Dispatches `logs` mode execution to the resolved sub-action (`follow`, `help`, or `show`).
///
/// # Workflow
///
/// 1. Log that `logs` mode is active when debug logging is enabled.
/// 2. If `--help` (`context.flags.help`) is set, route directly to [`help::handle`]
///    without reading `pods.conf` or any `.pod` recipe files.
/// 3. Route [`ModeAction::Logs`] in enum declaration order (`Follow`, `Help`, `Show`)
///    to the corresponding action module ([`follow::handle`], [`help::handle`], [`show::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `logs` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Logs mode is used.");
    if context.flags.help {
        return help::handle(context);
    }
    match context.action() {
        ModeAction::Logs(LogsAction::Follow) => follow::handle(context),
        ModeAction::Logs(LogsAction::Help) => help::handle(context),
        ModeAction::Logs(LogsAction::Show) => show::handle(context),
        _ => Err(PodError::from(CliError::with_argument(
            CliErrorKind::UnknownAction,
            context.action().to_string(),
        ))),
    }
}
