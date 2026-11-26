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

//! Handler for `pods update <target...>`.

use std::io;

use crate::core::context::Context;
use crate::core::modes::update_runner;
use crate::core::update::{PullCache, SummaryKind, UpdateRunners};
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman;

/// Pulls the latest container images for target recipes and reloads the pods when at least one
/// image was updated (`image update` + conditional `reload`), or pulls and conditionally reloads
/// explicitly targeted containers, printing a grouped `Update Summary:` on completion.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets (or resolve `<recipe>/<container>` targets) in
///    `context.parameters` against `context.config.recipes.groups`, preserving first-appearance
///    order and deduplicating repeated targets.
/// 2. Load and validate all targeted `.pod` recipes upfront before modifying running pods.
/// 3. For pod/group targets: pull container images with indented progress output (deduplicating
///    shared image references across containers and pods), detect containers running stale image
///    IDs when a pre-pulled image returns up-to-date, reload the pod (`down` + `up`) when at
///    least one image changed and no pull failed (or when `--simulate` is active), and separate
///    consecutive pods with a blank line.
/// 4. For container targets: pull and conditionally reload each targeted container without tearing
///    down an existing parent pod.
/// 5. Render the grouped `Update Summary:` section (`Reloaded`, `Failed`, `Up to date`, or
///    `Simulated`) and concluding totals line.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion or resolution fails, a targeted `.pod` recipe file
/// cannot be read or validated, or a `podman` pull/reload command fails to spawn.
pub fn handle(context: &Context) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut cache = PullCache::new();
    let mut pull_image = podman::pull_image;
    let simulate = context.config.simulate;
    let mut query_container = |container_name: &str| {
        if simulate {
            None
        } else {
            podman::query_container_image_id(container_name)
        }
    };
    let mut query_image = |image_reference: &str| {
        if simulate {
            None
        } else {
            podman::query_image_id(image_reference)
        }
    };
    let mut runners = UpdateRunners {
        cache: &mut cache,
        pull_image: &mut pull_image,
        query_container_image: &mut query_container,
        query_image: &mut query_image,
    };
    update_runner::execute_update_with_inspectors(
        context,
        SummaryKind::LifecycleUpdate,
        &mut runners,
        &mut stdout,
        &mut stderr,
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::Context;
    use crate::io::exec::command::ExecOutcome;
    use super::handle;

    #[test]
    fn handle_simulates_pod_and_container_updates() {
        let mut context = Context::new();
        context.config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();
        context.parameters = vec![String::from("@all")];
        assert_eq!(handle(&context).unwrap(), ExecOutcome::Succeeded);

        context.parameters = vec![String::from("recipe/db")];
        assert_eq!(handle(&context).unwrap(), ExecOutcome::Succeeded);
    }
}
