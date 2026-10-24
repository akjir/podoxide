//! Handler for `pods down <target...>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Stops target containers in reverse bottom-to-top order (`N..1`) and removes the containers
/// and parent pod (`stop` + `remove`).
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets in `context.parameters` against
///    `context.config.recipes.groups`, preserving first-appearance order and deduplicating
///    repeated recipes.
/// 2. Iterate sequentially over the expanded target recipe names:
///    - Read and validate only the targeted `<recipes.path>/<recipe>.pod` file.
///    - Stop containers in reverse bottom-to-top order (`N` down to `1`) and stop the pod.
///    - Remove containers in reverse bottom-to-top order (`N` down to `1`) and remove the
///      parent pod.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, a targeted `.pod` recipe file cannot be
/// read or validated, or a `podman` stop/remove command fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Expand and deduplicate @group and <recipe> targets via context.config.
    // Step 2: For each targeted recipe, read and validate only <recipes.path>/<recipe>.pod.
    // Step 3: Delegate to pod::down (stop + remove in reverse N..1 container order).
    Ok(())
}
