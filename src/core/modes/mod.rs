//! Mode enumeration re-exports and O(1) mode dispatcher.

pub use crate::core::context::{
    CommandAction, ConfigAction, ConnectAction, DefaultAction, ImageAction, InitAction,
    LogsAction, Mode, ModeAction, RecipeAction,
};
use crate::core::context::Context;
use crate::error::PodError;

const MODE_COUNT: usize = 8;
static MODE_DISPATCH_TABLE: [ModeHandler; MODE_COUNT] = [
    handle_command,
    handle_config,
    handle_connect,
    handle_default,
    handle_image,
    handle_init,
    handle_logs,
    handle_recipe,
];

type ModeHandler = fn(&Context) -> Result<(), PodError>;

/// Dispatches execution to the active mode handler in O(1) time.
///
/// # Errors
///
/// Returns [`PodError`] if the selected mode handler fails.
pub fn dispatch_mode(context: &Context) -> Result<(), PodError> {
    let handler = MODE_DISPATCH_TABLE[context.mode().as_index()];
    handler(context)
}

fn handle_command(_context: &Context) -> Result<(), PodError> {
    Ok(())
}

fn handle_config(_context: &Context) -> Result<(), PodError> {
    Ok(())
}

fn handle_connect(_context: &Context) -> Result<(), PodError> {
    Ok(())
}

fn handle_default(_context: &Context) -> Result<(), PodError> {
    Ok(())
}

fn handle_image(_context: &Context) -> Result<(), PodError> {
    Ok(())
}

fn handle_init(_context: &Context) -> Result<(), PodError> {
    Ok(())
}

fn handle_logs(_context: &Context) -> Result<(), PodError> {
    Ok(())
}

fn handle_recipe(_context: &Context) -> Result<(), PodError> {
    Ok(())
}
