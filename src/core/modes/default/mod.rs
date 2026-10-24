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
