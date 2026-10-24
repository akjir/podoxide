//! Handler for `pods up <target...>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Creates and starts target pods and containers in deterministic top-to-bottom declaration
/// order (`1..N`) (`create` + `start`).
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets in `context.parameters` against
///    `context.config.recipes.groups`, preserving first-appearance order and deduplicating
///    repeated recipes.
/// 2. Iterate sequentially over the expanded target recipe names:
///    - Read and validate only the targeted `<recipes.path>/<recipe>.pod` file.
///    - Create and start the Podman pod and its containers in top-to-bottom declaration
///      order (`1` to `N`).
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, a targeted `.pod` recipe file cannot be
/// read or validated, or a `podman` creation/startup command fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Expand and deduplicate @group and <recipe> targets via context.config.
    // Step 2: For each targeted recipe, read and validate only <recipes.path>/<recipe>.pod.
    // Step 3: Delegate to pod::up (create and start pod and containers in 1..N order).
    Ok(())
}
