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

//! Handler for `pods image update <target...>`.

use crate::core::context::Context;
use crate::core::modes::default::execute_lifecycle_action;
use crate::core::pod;
use crate::error::PodError;

/// Pulls the latest container images from configured registries for all containers in target
/// recipes without reloading running pods.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets in `context.parameters` against
///    `context.config.recipes.groups`, preserving first-appearance order and deduplicating
///    repeated recipes.
/// 2. Iterate sequentially over the expanded target recipe names:
///    - Read and validate only the targeted `<recipes.path>/<recipe>.pod` file.
///    - For each container in `1..N` declaration order, resolve the full image reference
///      (`<container.registry | pod.registry>/<container.image>`) and execute (or simulate)
///      `podman pull <image_ref>`.
///    - Do not stop, remove, or reload the running pod.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion fails, a targeted `.pod` recipe file cannot be
/// read or validated, or `podman pull` fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    execute_lifecycle_action(context, pod::pull)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{Context, ImageAction, Mode, ModeAction};
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError};
    use super::handle;

    #[test]
    fn handle_pulls_recipe_and_group_targets_and_rejects_invalid_targets() {
        let mut context = Context::new();
        context.config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();
        context.mode = Mode::Image;
        context.action = ModeAction::Image(ImageAction::Update);

        let missing_error = handle(&context).unwrap_err();
        match missing_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("unknown_recipe")];
        let unknown_error = handle(&context).unwrap_err();
        match unknown_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("@all")];
        assert!(handle(&context).is_ok());
    }
}
