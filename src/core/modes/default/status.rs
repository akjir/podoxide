//! Handler for `pods status [targets...]` and `pods status help`.

use crate::core::context::Context;
use crate::error::PodError;

/// Queries `podman ps -a`, cross-references managed recipes, and formats the runtime status
/// table (or displays `status` help when requested).
///
/// # Workflow
///
/// 1. Check whether `context.flags.help` is `true` or `context.parameters.first()` equals
///    `"help"`:
///    - If so, delegate immediately to [`handle_help`] and return `Ok(())` without reading
///      `pods.conf` or any `.pod` recipe files.
/// 2. Resolve the list of recipe names to inspect:
///    - If `context.parameters` is non-empty, expand and deduplicate `@<group>` and `<recipe>`
///      targets against `context.config.recipes.groups`.
///    - If `context.parameters` is empty, collect all unique configured recipes from
///      `context.config.recipes.groups`.
/// 3. Query `podman ps -a --format ...` to retrieve all host container records.
/// 4. Read and validate only the resolved `.pod` recipe files to build the map of expected
///    managed container names and pod identifiers.
/// 5. Filter or merge container records (`--all` includes unmanaged host containers; missing
///    managed containers are synthesized with status `"Not Found"`).
/// 6. Sort rows by `(pod, names)` and print the aligned status table (`--full` appends
///    `COMMAND` and `PORTS` columns).
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, `podman ps` fails, or a required recipe
/// cannot be processed.
pub fn handle(context: &Context) -> Result<(), PodError> {
    if context.flags.help
        || context
            .parameters()
            .first()
            .is_some_and(|parameter| parameter == "help")
    {
        return handle_help(context);
    }

    // Step 1: Resolve target recipes from context.parameters or all configured recipes.
    // Step 2: Query `podman ps -a --format ...` for runtime container states.
    // Step 3: Read and validate only the resolved .pod files to map expected container names.
    // Step 4: Merge managed and (when --all is set) unmanaged containers, synthesizing "Not Found" rows.
    // Step 5: Sort rows by (pod, container_name) and print the formatted table (--full adds COMMAND, PORTS).
    Ok(())
}

/// Formats and outputs the help text specific to `pods status`.
fn handle_help(_context: &Context) -> Result<(), PodError> {
    // Step 1: Format and print the `pods status` help text without reading any files.
    Ok(())
}
