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

//! Handler for `pods up <target...>`.

use crate::core::container;
use crate::core::context::Context;
use crate::core::pod;
use crate::error::PodError;
use super::execute_lifecycle_action;

/// Creates and starts target pods and containers in deterministic top-to-bottom declaration
/// order (`1..N`) (`create` + `start`), or creates and starts explicitly targeted containers.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets (or resolve `<recipe>/<container>` targets) in
///    `context.parameters` against `context.config.recipes.groups`, preserving first-appearance
///    order and deduplicating repeated targets.
/// 2. For pod/group targets: create the Podman pod and create and start its containers in
///    top-to-bottom declaration order (`1..N`).
/// 3. For container targets: create and start only the targeted containers without creating the
///    parent pod.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion or resolution fails, a targeted `.pod` recipe file
/// cannot be read or validated, or a `podman` creation/startup command fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    execute_lifecycle_action(context, pod::up, container::up)
}
