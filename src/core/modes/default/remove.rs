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

//! Handler for `pods remove <target...>`.

use crate::core::container;
use crate::core::context::Context;
use crate::core::pod;
use crate::error::PodError;
use super::execute_lifecycle_action;

/// Removes target containers in reverse bottom-to-top order (`N..1`) and removes the parent
/// pod without stopping them first, or removes explicitly targeted containers.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets (or resolve `<recipe>/<container>` targets) in
///    `context.parameters` against `context.config.recipes.groups`, preserving first-appearance
///    order and deduplicating repeated targets.
/// 2. For pod/group targets: remove containers in reverse bottom-to-top order (`N..1`) and
///    remove the parent pod.
/// 3. For container targets: remove only the targeted containers without removing the parent pod.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion or resolution fails, a targeted `.pod` recipe file
/// cannot be read or validated, or a `podman` removal command fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    execute_lifecycle_action(context, pod::remove, |item, _pod, simulate| {
        container::remove(item, simulate)
    })
}
