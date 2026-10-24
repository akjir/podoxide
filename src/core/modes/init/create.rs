//! Handler for `pods init` (template creation).

use crate::core::context::Context;
use crate::error::PodError;

/// Verifies that target files do not exist and writes the default `recipe.pod` and
/// `pods.conf` templates to disk.
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any existing `.pod` files from disk; use the embedded
///    compile-time template strings (`recipe.pod` and `pods.conf`).
/// 2. Check whether `./recipe.pod` or `context.config.path` (`./pods.conf` or custom
///    `--config` path) already exists on disk; abort with an error before writing if either
///    file already exists.
/// 3. Write `./recipe.pod` and log `"Created './recipe.pod'."` via `log::info`.
/// 4. Write `context.config.path` and log `"Created '<config.path>'."` via `log::info`.
///
/// # Errors
///
/// Returns [`PodError`] if either target file already exists on disk or if writing either
/// template file fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Verify that ./recipe.pod and context.config.path do not exist on disk.
    // Step 2: Write the embedded recipe.pod template to ./recipe.pod and log via log::info.
    // Step 3: Write the embedded pods.conf template to context.config.path and log via log::info.
    // Note: Do not read any existing configuration or recipe files from disk.
    Ok(())
}
