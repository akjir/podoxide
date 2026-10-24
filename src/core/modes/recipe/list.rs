//! Handler for `pods recipe [list] [--all]`.

use crate::core::context::Context;
use crate::error::PodError;

/// Lists all recipes registered in `pods.conf` with descriptions and file status, plus
/// unlinked `.pod` files on disk when `--all` is set.
///
/// # Workflow
///
/// 1. Collect the sorted list and lookup set of unique recipe names referenced across
///    `context.config.recipes.groups`.
/// 2. For each configured recipe:
///    - Check whether `<recipes.path>/<recipe>.pod` exists on disk (`[OK]` vs `[NOT FOUND]`).
///    - When the file exists, read and parse `<recipe>.pod` (suppressing validation warnings)
///      to extract `pod.name` and `pod.description`.
///    - Format the numbered entry `"  <idx>) <recipe> (<name>): <description>"` aligned with
///      its status column.
/// 3. When `context.flags.all` is `true`, list `.pod` filenames in `recipes.path` that are
///    not referenced in `context.config.recipes.groups` under `Unlinked Recipe Files:` with
///    status `[UNREFERENCED]`. Do not read or parse the contents of unreferenced `.pod` files.
///
/// # Errors
///
/// Returns [`PodError`] if directory inspection fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Collect unique configured recipe names from context.config.recipes.groups.
    // Step 2: Check file existence and parse existing configured .pod files for name/description.
    // Step 3: When --all is set, list unreferenced .pod filenames on disk without reading their contents.
    // Step 4: Print the aligned Recipes and (optional) Unlinked Recipe Files sections.
    Ok(())
}
