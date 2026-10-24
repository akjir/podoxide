//! Handler for `pods update <target...>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Pulls the latest container images for target recipes and rebuilds the pods
/// (`image update` + `reload`).
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets in `context.parameters` against
///    `context.config.recipes.groups`, preserving first-appearance order and deduplicating
///    repeated recipes.
/// 2. Iterate sequentially over the expanded target recipe names:
///    - Read and validate only the targeted `<recipes.path>/<recipe>.pod` file.
///    - Pull the latest container images from their resolved registries (`<registry>/<image>`).
///    - Reload the pod and its containers (`down` + `up`).
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, a targeted `.pod` recipe file cannot be
/// read or validated, or a `podman` pull/reload command fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Expand and deduplicate @group and <recipe> targets via context.config.
    // Step 2: For each targeted recipe, read and validate only <recipes.path>/<recipe>.pod.
    // Step 3: Delegate to pod::update (pull container images followed by pod reload).
    Ok(())
}
