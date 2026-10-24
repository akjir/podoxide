//! Handler for `pods stop <target...>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Stops running target containers in reverse bottom-to-top order (`N..1`) and stops the
/// parent pod without removing them.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets in `context.parameters` against
///    `context.config.recipes.groups`, preserving first-appearance order and deduplicating
///    repeated recipes.
/// 2. Iterate sequentially over the expanded target recipe names:
///    - Read and validate only the targeted `<recipes.path>/<recipe>.pod` file.
///    - Stop containers in reverse bottom-to-top order (`N` down to `1`) and stop the parent
///      pod without removing the containers or pod.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, a targeted `.pod` recipe file cannot be
/// read or validated, or a `podman` stop command fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Expand and deduplicate @group and <recipe> targets via context.config.
    // Step 2: For each targeted recipe, read and validate only <recipes.path>/<recipe>.pod.
    // Step 3: Delegate to pod::stop (stop containers in reverse N..1 order, then stop pod).
    Ok(())
}
