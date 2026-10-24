//! Handler for `pods image update <target...>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Pulls the latest container images from configured registries for all containers in target
/// recipes without reloading running pods.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets in `context.parameters` against
///    `context.config.recipes.groups`, preserving first-appearance order and deduplicating
///    repeated recipes.
/// 2. Iterate sequentially over the expanded target recipe names:
///    - Read and validate only the targeted `<recipes.path>/<recipe>.pod` file.
///    - For each container in `1..N` declaration order, resolve the full image reference
///      (`<container.registry | pod.registry>/<container.image>`) and execute (or simulate)
///      `podman pull <image_ref>`.
///    - Do not stop, remove, or reload the running pod.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, a targeted `.pod` recipe file cannot be
/// read or validated, or `podman pull` fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Expand and deduplicate @group and <recipe> targets via context.config.
    // Step 2: For each targeted recipe, read and validate only <recipes.path>/<recipe>.pod.
    // Step 3: Pull each container image (`podman pull <registry>/<image>`) without reloading pods.
    Ok(())
}
