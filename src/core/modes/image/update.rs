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

use std::io;

use crate::core::context::Context;
use crate::core::modes::default::update::execute_update_with;
use crate::core::update::SummaryKind;
use crate::error::PodError;
use crate::io::exec::podman;

/// Pulls the latest container images from configured registries for all containers in target
/// recipes or explicitly targeted containers (`<recipe>/<container>`) without reloading running
/// pods, printing a grouped `Update Summary:` on completion.
///
/// # Workflow
///
/// 1. Expand `@<group>` and `<recipe>` targets (or resolve `<recipe>/<container>` targets) in
///    `context.parameters` against `context.config.recipes.groups`, preserving first-appearance
///    order and deduplicating repeated targets.
/// 2. For pod/group targets: pull the latest image (`podman pull <image_ref>`) for each
///    container in `1..N` declaration order.
/// 3. For container targets: pull the latest image only for the targeted containers.
/// 4. Render the grouped `Update Summary:` section (`Updated`, `Failed`, `Up to date`, or
///    `Simulated`) and concluding totals line.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion or resolution fails, a targeted `.pod` recipe file
/// cannot be read or validated, or `podman pull` fails to spawn.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut pull_image = podman::pull_image;
    let _ = execute_update_with(
        context,
        SummaryKind::ImagePull,
        &mut pull_image,
        &mut stdout,
        &mut stderr,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{Context, ImageAction, Mode, ModeAction};
    use crate::core::log;
    use crate::core::modes::default::update::execute_update_with;
    use crate::core::update::SummaryKind;
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError};
    use crate::io::exec::command::ExecOutcome;
    use crate::io::exec::podman::PullOutcome;
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

        let mut mock_pull = |image_ref: &str,
                             prefix: &str,
                             _simulate: bool,
                             stdout: &mut Vec<u8>,
                             _stderr: &mut Vec<u8>|
         -> Result<PullOutcome, PodError> {
            let outcome = if image_ref.contains("example-app") {
                PullOutcome::Updated
            } else {
                PullOutcome::UpToDate
            };
            let suffix = if outcome == PullOutcome::Updated {
                "Updated."
            } else {
                "Up to date."
            };
            log::write_print(stdout, &format!("{prefix}{suffix}"));
            Ok(outcome)
        };

        let mut pod_out = Vec::new();
        let pod_outcome = execute_update_with(
            &context,
            SummaryKind::ImagePull,
            &mut mock_pull,
            &mut pod_out,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(pod_outcome, ExecOutcome::Succeeded);
        let pod_text = String::from_utf8(pod_out).unwrap();
        assert!(pod_text.contains("Update Summary:\n"));
        assert!(pod_text.contains(
            "  Updated (1):\n    - example-stack (Example Web Stack): example-stack-app (docker.io/example/example-app:latest)\n"
        ));
        assert!(
            pod_text.contains("Checked 1 pod (2 images): 1 updated, 1 up to date, 0 failed.\n")
        );

        context.parameters = vec![
            String::from("recipe/1"),
            String::from("recipe/*db"),
            String::from("recipe/db"),
            String::from("recipe/example-stack-db"),
            String::from("recipe/app"),
        ];
        assert!(handle(&context).is_ok());

        let mut cnt_out = Vec::new();
        let cnt_outcome = execute_update_with(
            &context,
            SummaryKind::ImagePull,
            &mut mock_pull,
            &mut cnt_out,
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(cnt_outcome, ExecOutcome::Succeeded);
        let cnt_text = String::from_utf8(cnt_out).unwrap();
        assert!(cnt_text.contains(
            "  Updated (1):\n    - example-stack-app (docker.io/example/example-app:latest)\n"
        ));
        assert!(cnt_text.contains("  Up to date (1):\n    - example-stack-db\n"));
        assert!(cnt_text.contains("Checked 2 images: 1 updated, 1 up to date, 0 failed.\n"));
    }
}
