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
/// 2. If `--help` (`context.flags.help`) is set, route directly to [`help::handle`]
///    without reading `pods.conf` or any `.pod` recipe files.
/// 3. Route [`ModeAction::Image`] in enum declaration order (`Help`, `Prune`, `Update`)
///    to the corresponding action module ([`help::handle`], [`prune::handle`], [`update::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `image` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Image mode is used.");
    if context.flags.help {
        return help::handle(context);
    }
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
