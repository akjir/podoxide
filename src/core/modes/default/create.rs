//! Handler for `pods create <target...>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Creates target pods and their containers in top-to-bottom declaration order (`1..N`)
/// without starting them.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets in `context.parameters` against
///    `context.config.recipes.groups`, preserving first-appearance order and deduplicating
///    repeated recipes.
/// 2. Iterate sequentially over the expanded target recipe names:
///    - Read and validate only the targeted `<recipes.path>/<recipe>.pod` file.
///    - Create the Podman pod (`podman pod create ...`) with configured port mappings and
///      options.
///    - Create each container (`podman create ...`) in top-to-bottom declaration order
///      (`1` to `N`) without starting the pod or containers.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, a targeted `.pod` recipe file cannot be
/// read or validated, or a `podman` creation command fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Expand and deduplicate @group and <recipe> targets via context.config.
    // Step 2: For each targeted recipe, read and validate only <recipes.path>/<recipe>.pod.
    // Step 3: Delegate to pod::create to create the pod and containers (1..N) without starting them.
    Ok(())
}
