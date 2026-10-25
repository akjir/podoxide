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

use crate::core::container;
use crate::core::context::Context;
use crate::core::modes::default::execute_lifecycle_action;
use crate::core::pod;
use crate::error::PodError;

/// Pulls the latest container images from configured registries for all containers in target
/// recipes or explicitly targeted containers (`<recipe>/<container>`) without reloading running pods.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets (or resolve `<recipe>/<container>` targets) in
///    `context.parameters` against `context.config.recipes.groups`, preserving first-appearance
///    order and deduplicating repeated targets.
/// 2. For pod/group targets: pull the latest image (`podman pull <image_ref>`) for each
///    container in `1..N` declaration order.
/// 3. For container targets: pull the latest image only for the targeted containers.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion or resolution fails, a targeted `.pod` recipe file
/// cannot be read or validated, or `podman pull` fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    execute_lifecycle_action(context, pod::pull, container::pull)
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

        context.parameters = vec![
            String::from("recipe/1"),
            String::from("recipe/*db"),
            String::from("recipe/db"),
            String::from("recipe/example-stack-db"),
        ];
        assert!(handle(&context).is_ok());
    }
}
