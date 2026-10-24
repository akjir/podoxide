//! Recipe mode (`pods recipe` / `pods rec`) action dispatcher.

pub mod edit;
pub mod help;
pub mod list;
pub mod show;

use crate::core::context::{Context, ModeAction, RecipeAction};
use crate::core::log;
use crate::error::{CliError, CliErrorKind, PodError};

/// Dispatches `recipe` mode execution to the resolved sub-action (`edit`, `help`, `list`, or
/// `show`).
///
/// # Workflow
///
/// 1. Log that `recipe` mode is active when debug logging is enabled.
/// 2. If `--help` (`context.flags.help`) is set, route directly to [`help::handle`]
///    without reading `pods.conf` or any `.pod` recipe files.
/// 3. Route [`ModeAction::Recipe`] in enum declaration order (`Edit`, `Help`, `List`, `Show`)
///    to the corresponding action module ([`edit::handle`], [`help::handle`], [`list::handle`],
///    [`show::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `recipe` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Recipe mode is used.");
    if context.flags.help {
        return help::handle(context);
    }
    match context.action() {
        ModeAction::Recipe(RecipeAction::Edit) => edit::handle(context),
        ModeAction::Recipe(RecipeAction::Help) => help::handle(context),
        ModeAction::Recipe(RecipeAction::List) => list::handle(context),
        ModeAction::Recipe(RecipeAction::Show) => show::handle(context),
        _ => Err(PodError::from(CliError::with_argument(
            CliErrorKind::UnknownAction,
            context.action().to_string(),
        ))),
    }
}
