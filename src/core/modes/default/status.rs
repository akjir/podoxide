// Podoxide
// Copyright (C) 2026  Stefan Stark
//
// This program is free software: you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation, either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License along with
// this program.  If not, see <https://www.gnu.org/licenses/>.

//! Handler for `pods status [targets...]`.

use crate::core::context::Context;
use crate::error::PodError;

/// Queries `podman ps -a`, cross-references managed recipes, and formats the runtime status
/// table.
///
/// # Workflow
///
/// 1. Resolve the list of recipe names to inspect:
///    - If `context.parameters` is non-empty, expand and deduplicate `@<group>` and `<recipe>`
///      targets against `context.config.recipes.groups`.
///    - If `context.parameters` is empty, collect all unique configured recipes from
///      `context.config.recipes.groups`.
/// 2. Query `podman ps -a --format ...` to retrieve all host container records.
/// 3. Read and validate only the resolved `.pod` recipe files to build the map of expected
///    managed container names and pod identifiers.
/// 4. Filter or merge container records (`--all` includes unmanaged host containers; missing
///    managed containers are synthesized with status `"Not Found"`).
/// 5. Sort rows by `(pod, names)` and print the aligned status table (`--full` appends
///    `COMMAND` and `PORTS` columns).
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, `podman ps` fails, or a required recipe
/// cannot be processed.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Resolve target recipes from context.parameters or all configured recipes.
    // Step 2: Query `podman ps -a --format ...` for runtime container states.
    // Step 3: Read and validate only the resolved .pod files to map expected container names.
    // Step 4: Merge managed and (when --all is set) unmanaged containers, synthesizing "Not Found" rows.
    // Step 5: Sort rows by (pod, container_name) and print the formatted table (--full adds COMMAND, PORTS).
    Ok(())
}
