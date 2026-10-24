//! Handler for `pods recipe [show] <recipe>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Reads a single `.pod` recipe file and outputs its raw contents prefixed with 1-based line
/// numbers.
///
/// # Workflow
///
/// 1. Extract `<recipe>` (`context.parameters[0]`) and verify that `<recipe>` belongs to at
///    least one group in `context.config.recipes.groups`.
/// 2. Construct `<recipes.path>/<recipe>.pod` and read only the raw text lines of that single
///    file. Do not parse or validate the file into a PSL AST, and do not read any other
///    `.pod` files.
/// 3. Print each line prefixed with its 1-based line number (`format!("{line_num:3}: {line}")`).
///
/// # Errors
///
/// Returns [`PodError`] if `<recipe>` is not registered in `pods.conf` or if reading the
/// `.pod` file from disk fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Extract <recipe> and verify membership in context.config.recipes.groups.
    // Step 2: Read raw lines of <recipes.path>/<recipe>.pod without parsing AST or other files.
    // Step 3: Output each line formatted with a 3-character right-aligned 1-based line number.
    Ok(())
}
