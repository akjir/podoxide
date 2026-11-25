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

//! Handler for `pods update <target...>` and shared update orchestration with grouped summaries.

use std::io::{self, Write};

use crate::core::config::Config;
use crate::core::container::{self, Container};
use crate::core::context::Context;
use crate::core::log;
use crate::core::pod::{self, Pod};
use crate::core::recipe::Recipe;
use crate::core::update::{
    ContainerUpdateResult, SummaryKind, SummaryScope, UpdateAction, UpdateSummary,
};
use crate::error::{CliError, CliErrorKind, PodError};
use crate::io::exec::podman::{self, PullOutcome};
use crate::utilities::collection::{MAX_COLLECTION_PREALLOC, bounded_capacity};
use super::{collect_container_specs, expand_pod_targets, resolve_container_targets};

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
/// 3. For pod/group targets: pull container images with indented progress output, reload the pod
///    (`down` + `up`) when at least one image changed and no pull failed (or when `--simulate` is
///    active), and separate consecutive pods with a blank line.
/// 4. For container targets: pull and conditionally reload each targeted container without tearing
///    down an existing parent pod.
/// 5. Render the grouped `Update Summary:` section (`Reloaded`, `Failed`, `Up to date`, or
///    `Simulated`) and concluding totals line.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion or resolution fails, a targeted `.pod` recipe file
/// cannot be read or validated, or a `podman` pull/reload command fails to spawn.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut pull_image = podman::pull_image;
    execute_update_with(
        context,
        SummaryKind::LifecycleUpdate,
        &mut pull_image,
        &mut stdout,
        &mut stderr,
    )
}

/// Resolves targets from `context`, runs pod or container updates/pulls via `pull_image`, and renders the grouped `Update Summary:` to `stdout`.
///
/// # Errors
///
/// Returns [`PodError`] if target resolution, recipe validation, or command spawning fails.
pub(crate) fn execute_update_with<W, E, P>(
    context: &Context,
    kind: SummaryKind,
    pull_image: &mut P,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
{
    let Some(first) = context.parameters.first() else {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    };
    if first.contains('/') {
        return execute_container_updates(context, kind, pull_image, stdout, stderr);
    }
    execute_pod_updates(context, kind, pull_image, stdout, stderr)
}

/// Resolves `<recipe>/<container>` targets, executes pulls or lifecycle updates, and writes the container summary.
fn execute_container_updates<W, E, P>(
    context: &Context,
    kind: SummaryKind,
    pull_image: &mut P,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
{
    if log::is_debug_enabled() {
        log::debug(&format!("Targets   - {}", context.parameters.join(" ")));
    }
    let specs = collect_container_specs(context)?;
    let (recipes, resolved) = resolve_container_targets(&context.config, &specs)?;
    let mut summary = UpdateSummary::new(kind, SummaryScope::Container);
    for (index, (recipe_name, container_name)) in resolved.iter().enumerate() {
        let Some(recipe) = recipes.get(recipe_name) else {
            continue;
        };
        if index > 0 && kind == SummaryKind::LifecycleUpdate {
            log::write_print(stdout, "");
        }
        let container = recipe.resolve_container(container_name)?;
        let result = run_single_container_update(
            kind,
            container,
            &recipe.pod,
            context.config.simulate,
            pull_image,
            stdout,
            stderr,
        )?;
        summary.record_container(result.action, &result.record);
    }
    summary.write_to(stdout);
    Ok(())
}

/// Expands `@<group>` and `<recipe>` targets, executes pod pulls or lifecycle updates, and writes the pod summary.
fn execute_pod_updates<W, E, P>(
    context: &Context,
    kind: SummaryKind,
    pull_image: &mut P,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
{
    let targets = expand_pod_targets(context)?;
    let recipes = load_target_recipes(&context.config, &targets)?;
    let mut summary = UpdateSummary::new(kind, SummaryScope::Pod);
    for (index, recipe) in recipes.iter().enumerate() {
        if index > 0 {
            log::write_print(stdout, "");
        }
        let result = match kind {
            SummaryKind::ImagePull => {
                pod::pull_with_runner(recipe, context.config.simulate, pull_image, stdout, stderr)?
            }
            SummaryKind::LifecycleUpdate => pod::update_with_runner(
                recipe,
                context.config.simulate,
                pull_image,
                stdout,
                stderr,
            )?,
        };
        summary.record_pod(&result);
    }
    summary.write_to(stdout);
    Ok(())
}

/// Loads and validates all target `.pod` recipes upfront in expansion order.
fn load_target_recipes(config: &Config, targets: &[String]) -> Result<Vec<Recipe>, PodError> {
    let capacity = bounded_capacity(targets.len(), MAX_COLLECTION_PREALLOC);
    let mut recipes = Vec::with_capacity(capacity);
    for target in targets {
        recipes.push(Recipe::inspect(config, target)?);
    }
    Ok(recipes)
}

/// Executes a single container pull or lifecycle update and returns its [`ContainerUpdateResult`].
fn run_single_container_update<W, E, P>(
    kind: SummaryKind,
    container: &Container,
    pod: &Pod,
    simulate: bool,
    pull_image: &mut P,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ContainerUpdateResult, PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
{
    match kind {
        SummaryKind::ImagePull => {
            let record = container::pull_with_runner(
                container, pod, "", simulate, pull_image, stdout, stderr,
            )?;
            Ok(ContainerUpdateResult {
                action: UpdateAction::UpToDate,
                record,
            })
        }
        SummaryKind::LifecycleUpdate => {
            container::update_with_runner(container, pod, simulate, pull_image, stdout, stderr)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs as std_fs;
    use std::io::Write;
    use std::path::PathBuf;
    use std::process;
    use std::str::FromStr;

    use crate::core::config::Config;
    use crate::core::context::Context;
    use crate::core::log;
    use crate::core::update::SummaryKind;
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError};
    use crate::io::exec::podman::PullOutcome;
    use crate::io::system::fs;
    use super::{execute_update_with, handle};

    const CACHE_RECIPE_PSL: &str = "\
pod cache {
    name \"Redis Cache\"
    registry docker.io
}
container *redis {
    image library/redis:7-alpine
}
";

    const PROXY_RECIPE_PSL: &str = "\
pod proxy {
    name \"Reverse Proxy\"
    registry docker.io
}
container *nginx {
    image library/nginx:stable-alpine
}
";

    const STACK_RECIPE_PSL: &str = "\
pod example-stack {
    name \"Example Web Stack\"
    path /pods/example-stack
    registry docker.io
}
container *db {
    image example/db-server:latest
}
container *app {
    image example/example-app:latest
}
";

    fn create_update_workspace(suffix: &str) -> (PathBuf, Config) {
        let temp_dir =
            env::temp_dir().join(format!("podoxide_update_test_{}_{suffix}", process::id()));
        let _ = std_fs::remove_dir_all(&temp_dir);
        let recipes_dir = temp_dir.join("recipes");
        std_fs::create_dir_all(&recipes_dir).unwrap();

        fs::write_file(&recipes_dir.join("stack.pod"), STACK_RECIPE_PSL).unwrap();
        fs::write_file(&recipes_dir.join("cache.pod"), CACHE_RECIPE_PSL).unwrap();
        fs::write_file(&recipes_dir.join("proxy.pod"), PROXY_RECIPE_PSL).unwrap();

        let config_input = "\
general {
    editor vim
    simulate true
}

pods {
    path /pods
}

recipes {
    path ./recipes

    groups {
        all stack cache proxy
    }
}
";
        let mut config = Config::from_str(config_input).unwrap();
        config.recipes.path = recipes_dir;
        (temp_dir, config)
    }

    fn mock_mixed_pull<W: Write, E: Write>(
        image_reference: &str,
        prefix: &str,
        _simulate: bool,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<PullOutcome, PodError> {
        let outcome = match image_reference {
            "docker.io/example/example-app:latest" => PullOutcome::Updated,
            "docker.io/library/nginx:stable-alpine" => PullOutcome::Failed,
            _ => PullOutcome::UpToDate,
        };
        let suffix = match outcome {
            PullOutcome::Failed => "Failed.",
            PullOutcome::Simulated => "Simulated.",
            PullOutcome::UpToDate => "Up to date.",
            PullOutcome::Updated => "Updated.",
        };
        log::write_print(stdout, &format!("{prefix}{suffix}"));
        if outcome == PullOutcome::Failed {
            log::write_warning(stderr, "Command exited with code '1'!");
        }
        Ok(outcome)
    }

    #[test]
    fn execute_update_with_renders_grouped_summary_for_pods_and_containers() {
        let (temp_dir, config) = create_update_workspace("grouped");
        let mut context = Context::new();
        context.config = config;
        context.parameters = vec![String::from("@all")];

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        execute_update_with(
            &context,
            SummaryKind::LifecycleUpdate,
            &mut mock_mixed_pull,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        let output = String::from_utf8(stdout).unwrap();
        let err_output = String::from_utf8(stderr).unwrap();

        assert!(output.contains("Update pod 'Example Web Stack' ('example-stack') ...\n"));
        assert!(output.contains(
            "  Trying to pull docker.io/example/db-server:latest (example-stack-db): Up to date.\n"
        ));
        assert!(output.contains(
            "  Trying to pull docker.io/example/example-app:latest (example-stack-app): Updated.\n"
        ));
        assert!(output.contains("  Stop container 'example-stack-app': \n"));
        assert!(output.contains("  Start container 'example-stack-app': \n"));
        assert!(output.contains("\nUpdate pod 'Redis Cache' ('cache') ...\n"));
        assert!(output.contains("  Pod 'Redis Cache' ('cache') is up to date.\n"));
        assert!(output.contains("\nUpdate pod 'Reverse Proxy' ('proxy') ...\n"));
        assert!(output.contains("  Pod 'Reverse Proxy' ('proxy') update failed.\n"));
        assert!(!output.contains("Pod 'Reverse Proxy' ('proxy') is up to date."));
        assert!(err_output.contains("WARNING: Command exited with code '1'!"));
        assert!(output.ends_with(concat!(
            "\n",
            "Update Summary:\n",
            "  Reloaded (1):\n",
            "    - example-stack (Example Web Stack): example-stack-app (docker.io/example/example-app:latest)\n",
            "\n",
            "  Failed (1):\n",
            "    - proxy (Reverse Proxy): proxy-nginx (docker.io/library/nginx:stable-alpine)\n",
            "\n",
            "  Up to date (1):\n",
            "    - cache (Redis Cache)\n",
            "\n",
            "Checked 3 pods (4 containers): 1 reloaded, 1 up to date, 1 failed.\n",
        )));

        // Container-targeted update across multiple recipes.
        context.parameters = vec![
            String::from("stack/db"),
            String::from("stack/app"),
            String::from("proxy/1"),
        ];
        let mut cnt_stdout = Vec::new();
        let mut cnt_stderr = Vec::new();
        execute_update_with(
            &context,
            SummaryKind::LifecycleUpdate,
            &mut mock_mixed_pull,
            &mut cnt_stdout,
            &mut cnt_stderr,
        )
        .unwrap();
        let cnt_output = String::from_utf8(cnt_stdout).unwrap();
        assert!(cnt_output.contains("Container 'example-stack-db' is up to date.\n\n"));
        assert!(cnt_output.contains("Stop container 'example-stack-app': \n"));
        assert!(cnt_output.contains("Container 'proxy-nginx' update failed.\n"));
        assert!(cnt_output.ends_with(concat!(
            "\n",
            "Update Summary:\n",
            "  Reloaded (1):\n",
            "    - example-stack-app (docker.io/example/example-app:latest)\n",
            "\n",
            "  Failed (1):\n",
            "    - proxy-nginx (docker.io/library/nginx:stable-alpine)\n",
            "\n",
            "  Up to date (1):\n",
            "    - example-stack-db\n",
            "\n",
            "Checked 3 containers: 1 reloaded, 1 up to date, 1 failed.\n",
        )));

        // Live handle wrapper in simulate mode succeeds cleanly.
        context.parameters = vec![String::from("@all")];
        assert!(handle(&context).is_ok());

        // Missing and unknown target errors fail before producing output.
        context.parameters.clear();
        let mut err_stdout = Vec::new();
        let missing = execute_update_with(
            &context,
            SummaryKind::LifecycleUpdate,
            &mut mock_mixed_pull,
            &mut err_stdout,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(
            missing,
            PodError::Cli(error) if error.kind == CliErrorKind::MissingTarget
        ));
        assert!(err_stdout.is_empty());

        context.parameters = vec![String::from("unknown_recipe")];
        let unknown = execute_update_with(
            &context,
            SummaryKind::LifecycleUpdate,
            &mut mock_mixed_pull,
            &mut err_stdout,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(
            unknown,
            PodError::Config(error) if error.kind == ConfigErrorKind::RecipeNotFound
        ));
        assert!(err_stdout.is_empty());

        // Mixing pod and container targets fails before executing any pull.
        for mixed in [
            vec![String::from("stack"), String::from("stack/db")],
            vec![String::from("stack/db"), String::from("stack")],
        ] {
            context.parameters = mixed;
            let error = execute_update_with(
                &context,
                SummaryKind::LifecycleUpdate,
                &mut mock_mixed_pull,
                &mut err_stdout,
                &mut Vec::new(),
            )
            .unwrap_err();
            assert!(matches!(
                error,
                PodError::Cli(cli_error) if cli_error.kind == CliErrorKind::InvalidTarget
            ));
            assert!(err_stdout.is_empty());
        }

        // Invalid later recipe aborts upfront before pulling earlier recipes.
        fs::write_file(
            &context.config.recipes.path.join("broken.pod"),
            "invalid {{{ psl",
        )
        .unwrap();
        context.config.recipes.groups.insert(
            String::from("partial_bad"),
            vec![String::from("stack"), String::from("broken")],
        );
        context.parameters = vec![String::from("@partial_bad")];
        let recipe_err = execute_update_with(
            &context,
            SummaryKind::LifecycleUpdate,
            &mut mock_mixed_pull,
            &mut err_stdout,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(recipe_err, PodError::Psl(_) | PodError::Recipe(_)));
        assert!(err_stdout.is_empty());

        let _ = std_fs::remove_dir_all(&temp_dir);
    }
}
