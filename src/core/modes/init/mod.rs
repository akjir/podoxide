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
