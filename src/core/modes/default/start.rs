//! Handler for `pods start <target...>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Starts existing stopped target pods and their containers in top-to-bottom declaration
/// order (`1..N`) without recreating them.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets in `context.parameters` against
///    `context.config.recipes.groups`, preserving first-appearance order and deduplicating
///    repeated recipes.
/// 2. Iterate sequentially over the expanded target recipe names:
///    - Read and validate only the targeted `<recipes.path>/<recipe>.pod` file.
///    - Start the Podman pod and start each container in top-to-bottom declaration order
///      (`1` to `N`).
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, a targeted `.pod` recipe file cannot be
/// read or validated, or a `podman` start command fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Expand and deduplicate @group and <recipe> targets via context.config.
    // Step 2: For each targeted recipe, read and validate only <recipes.path>/<recipe>.pod.
    // Step 3: Delegate to pod::start (start pod and containers in 1..N order).
    Ok(())
}
