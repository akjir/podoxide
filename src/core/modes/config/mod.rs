//! Config mode (`pods config` / `pods cfg`) action dispatcher.

pub mod edit;
pub mod help;
pub mod show;

use crate::core::context::{ConfigAction, Context, ModeAction};
use crate::core::log;
use crate::error::{CliError, CliErrorKind, PodError};

/// Dispatches `config` mode execution to the resolved sub-action (`edit`, `help`, or `show`).
///
/// # Workflow
///
/// 1. Log that `config` mode is active when debug logging is enabled.
/// 2. If `--help` (`context.flags.help`) is set, route directly to [`help::handle`]
///    without reading `pods.conf` or any `.pod` recipe files.
/// 3. Route [`ModeAction::Config`] in enum declaration order (`Edit`, `Help`, `Show`)
///    to the corresponding action module ([`edit::handle`], [`help::handle`], [`show::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `config` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Config mode is used.");
    if context.flags.help {
        return help::handle(context);
    }
    match context.action() {
        ModeAction::Config(ConfigAction::Edit) => edit::handle(context),
        ModeAction::Config(ConfigAction::Help) => help::handle(context),
        ModeAction::Config(ConfigAction::Show) => show::handle(context),
        _ => Err(PodError::from(CliError::with_argument(
            CliErrorKind::UnknownAction,
            context.action().to_string(),
        ))),
    }
}
