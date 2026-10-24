//! Handler for `pods image prune`.

use crate::core::context::Context;
use crate::error::PodError;

/// Previews reclaimable image disk space (`--preview`) or prunes dangling and unused
/// container images (`--all`, `--force`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. If `context.flags.preview` is `true`:
///    - Query existing container IDs via `podman ps -aq` and inspect their attached image IDs
///      (`podman inspect -f '{{.Image}}' ...`).
///    - Query candidate images via `podman images [--filter dangling=true] --format ...`
///      (omitting the dangling filter when `context.flags.all` is `true`).
///    - Filter out images attached to existing containers, sum reclaimable byte sizes, and
///      print the formatted preview list and total reclaimable space.
/// 3. If `context.flags.preview` is `false`:
///    - Unless `context.flags.force` or `context.config.simulate` is `true`, prompt the user
///      for interactive `"yes"` confirmation before deleting images.
///    - Execute (or simulate) `podman image prune -f` (appending `-a` when `context.flags.all`
///      is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if querying or pruning images via `podman` fails or interactive I/O
/// fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Execute without reading pods.conf or any .pod files.
    // Step 2: If --preview is set, query in-use images and candidate images, then print reclaimable space.
    // Step 3: Otherwise, confirm interactively (unless --force or --simulate) and run `podman image prune -f [-a]`.
    Ok(())
}
