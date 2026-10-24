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

//! Handler for `pods config edit`.

use crate::core::context::Context;
use crate::error::PodError;

/// Opens the active `pods.conf` configuration file in the configured external editor.
///
/// # Workflow
///
/// 1. Use `context.config` already loaded from `context.config.path`. Never read or parse
///    any `.pod` recipe files.
/// 2. Verify that `context.config.editor` is non-empty; return an error if no editor is
///    configured.
/// 3. Execute `<editor> <config.path>` interactively via `src/io/exec/command.rs` (or print
///    the command without executing it when `context.config.simulate` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if `general.editor` is empty or if spawning the editor process fails.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Verify that context.config.editor is configured (non-empty).
    // Step 2: Spawn the editor process interactively with context.config.path (or simulate).
    // Note: Do not read or inspect any .pod recipe files.
    Ok(())
}
