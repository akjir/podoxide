//! Handler for `pods config edit`.

use crate::core::context::Context;
use crate::error::PodError;

/// Opens the active `pods.conf` configuration file in the configured external editor.
///
/// # Workflow
///
/// 1. Use `context.config` already loaded from `context.config.path`. Never read or parse
///    any `.pod` recipe files.
/// 2. Verify that `context.config.editor` is non-empty; return an error if no editor is
///    configured.
/// 3. Execute `<editor> <config.path>` interactively via `src/io/exec/command.rs` (or print
///    the command without executing it when `context.config.simulate` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if `general.editor` is empty or if spawning the editor process fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Verify that context.config.editor is configured (non-empty).
    // Step 2: Spawn the editor process interactively with context.config.path (or simulate).
    // Note: Do not read or inspect any .pod recipe files.
    Ok(())
}
