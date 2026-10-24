//! Handler for `pods help` (global CLI help).

use crate::core::context::Context;
use crate::error::PodError;

/// Formats and outputs the global CLI help text for `pods` (default mode).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header, global usage synopsis (`pods [MODE] [OPTIONS] [ACTION] [TARGETS...]`),
///    available modes and 3-letter aliases, global options, default mode lifecycle and status
///    actions, and target syntax via `log::print`.
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Format and print the global default mode help text without reading any files.
    Ok(())
}
