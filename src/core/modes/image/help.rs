//! Handler for `pods image help`.

use crate::core::context::Context;
use crate::error::PodError;

/// Formats and outputs the CLI help text for `pods image` (`pods img`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header, `pods image` usage synopsis, actions (`update`, `prune`,
///    `help`), options (`--all`, `--config`, `--debug`, `--force`, `--preview`, `--simulate`),
///    and `<recipe>` / `@<group>` target syntax via `log::print`.
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Format and print the image mode help text without reading any files.
    Ok(())
}
