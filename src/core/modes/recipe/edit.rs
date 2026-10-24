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

//! Handler for `pods recipe edit <recipe>`.

use crate::core::context::Context;
use crate::error::PodError;

/// Opens the specified `.pod` recipe file in the external editor configured in `pods.conf`.
///
/// # Workflow
///
/// 1. Extract `<recipe>` (`context.parameters[0]`) and verify that `<recipe>` belongs to at
///    least one group in `context.config.recipes.groups`.
/// 2. Verify that `context.config.editor` is non-empty; return an error if no editor is
///    configured.
/// 3. Construct the target path `<recipes.path>/<recipe>.pod` without reading or parsing the
///    `.pod` file contents.
/// 4. Execute `<editor> <full_path>` interactively via `src/io/exec/command.rs` (or print
///    the command without executing it when `context.config.simulate` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if `<recipe>` is not registered in `pods.conf`, `general.editor` is
/// empty, or spawning the editor process fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Extract <recipe> and verify membership in context.config.recipes.groups.
    // Step 2: Verify that context.config.editor is non-empty.
    // Step 3: Build <recipes.path>/<recipe>.pod and launch the editor interactively (or simulate).
    // Note: Do not read or parse the .pod file contents.
    Ok(())
}
