//! Handler for `pods remove <target...>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Removes target containers in reverse bottom-to-top order (`N..1`) and removes the parent
/// pod without stopping them first.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets in `context.parameters` against
///    `context.config.recipes.groups`, preserving first-appearance order and deduplicating
///    repeated recipes.
/// 2. Iterate sequentially over the expanded target recipe names:
///    - Read and validate only the targeted `<recipes.path>/<recipe>.pod` file.
///    - Remove containers in reverse bottom-to-top order (`N` down to `1`) and remove the
///      parent pod.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, a targeted `.pod` recipe file cannot be
/// read or validated, or a `podman` removal command fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Expand and deduplicate @group and <recipe> targets via context.config.
    // Step 2: For each targeted recipe, read and validate only <recipes.path>/<recipe>.pod.
    // Step 3: Delegate to pod::remove (remove containers in reverse N..1 order, then remove pod).
    Ok(())
}
