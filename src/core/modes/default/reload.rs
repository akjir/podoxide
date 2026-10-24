//! Handler for `pods reload <target...>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Tears down and rebuilds target pods and containers from their `.pod` recipes (`down` + `up`).
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets in `context.parameters` against
///    `context.config.recipes.groups`, preserving first-appearance order and deduplicating
///    repeated recipes.
/// 2. Iterate sequentially over the expanded target recipe names:
///    - Read and validate only the targeted `<recipes.path>/<recipe>.pod` file.
///    - Execute `pod::down` (stop and remove containers in reverse `N..1` order and remove
///      the pod).
///    - Execute `pod::up` (create and start the pod and containers in top-to-bottom `1..N`
///      order).
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, a targeted `.pod` recipe file cannot be
/// read or validated, or a `podman` teardown/creation command fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Expand and deduplicate @group and <recipe> targets via context.config.
    // Step 2: For each targeted recipe, read and validate only <recipes.path>/<recipe>.pod.
    // Step 3: Delegate to pod::reload (down followed by up).
    Ok(())
}
