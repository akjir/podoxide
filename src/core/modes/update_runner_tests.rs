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

//! Unit tests for shared pod and container update orchestration in [`super`].

use std::fs as std_fs;
use std::io::Write;
use std::str::FromStr;

use crate::core::config::Config;
use crate::core::context::Context;
use crate::core::log;
use crate::core::update::{PullCache, SummaryKind, UpdateRunners};
use crate::error::{CliErrorKind, ConfigErrorKind, PodError};
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::PullOutcome;
use crate::io::system::fs::{self, TestTempDir};
use super::{execute_update_with, execute_update_with_inspectors};

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

fn create_update_workspace(suffix: &str) -> (TestTempDir, Config) {
    let temp_dir = TestTempDir::new(&format!("update_test_{suffix}"));
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
fn execute_update_with_handles_numeric_container_names() {
    let temp_dir = TestTempDir::new("update_numeric");
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&recipes_dir).unwrap();

    fs::write_file(
        &recipes_dir.join("num_two.pod"),
        "pod num_two_pod {\n    name \"Numeric Two\"\n    path /pods/num_two\n    registry docker.io\n}\ncontainer 2 {\n    image example/first:latest\n}\ncontainer *db {\n    image example/second:latest\n}\n",
    )
    .unwrap();
    fs::write_file(
        &recipes_dir.join("num_five.pod"),
        "pod num_five_pod {\n    name \"Numeric Five\"\n    path /pods/num_five\n    registry docker.io\n}\ncontainer 5 {\n    image example/only:latest\n}\n",
    )
    .unwrap();

    let mut config = Config::new();
    config.simulate = true;
    config.pods.path = temp_dir.join("pods");
    config.recipes.path = recipes_dir;
    config.recipes.groups.insert(
        String::from("all"),
        vec![String::from("num_two"), String::from("num_five")],
    );

    let mut context = Context::new();
    context.config = config;
    context.parameters = vec![String::from("num_two/1"), String::from("num_five/1")];

    let mut pulled_images = Vec::new();
    let mut record_pull = |image_ref: &str,
                           prefix: &str,
                           _: bool,
                           stdout: &mut Vec<u8>,
                           _: &mut Vec<u8>|
     -> Result<PullOutcome, PodError> {
        pulled_images.push(image_ref.to_owned());
        log::write_print(stdout, &format!("{prefix}Simulated."));
        Ok(PullOutcome::Simulated)
    };

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let outcome = execute_update_with(
        &context,
        SummaryKind::LifecycleUpdate,
        &mut record_pull,
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(outcome, ExecOutcome::Succeeded);
    assert_eq!(
        pulled_images,
        vec![
            String::from("docker.io/example/first:latest"),
            String::from("docker.io/example/only:latest"),
        ]
    );
    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("- 2 (docker.io/example/first:latest)"));
    assert!(output.contains("- 5 (docker.io/example/only:latest)"));
    assert!(!output.contains("num_two_pod-db"));
}

#[test]
fn execute_update_with_renders_grouped_summary_for_pods_and_containers() {
    let (_temp_dir, config) = create_update_workspace("grouped");
    let mut context = Context::new();
    context.config = config;
    context.parameters = vec![String::from("@all")];

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let pod_outcome = execute_update_with(
        &context,
        SummaryKind::LifecycleUpdate,
        &mut mock_mixed_pull,
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(pod_outcome, ExecOutcome::Failed);
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

    context.parameters = vec![
        String::from("stack/db"),
        String::from("stack/app"),
        String::from("proxy/1"),
    ];
    let mut cnt_stdout = Vec::new();
    let mut cnt_stderr = Vec::new();
    let cnt_outcome = execute_update_with(
        &context,
        SummaryKind::LifecycleUpdate,
        &mut mock_mixed_pull,
        &mut cnt_stdout,
        &mut cnt_stderr,
    )
    .unwrap();
    assert_eq!(cnt_outcome, ExecOutcome::Failed);
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
            PodError::Cli(cli_error) if cli_error.kind == CliErrorKind::MixedTargets
        ));
        assert!(err_stdout.is_empty());
    }

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
}

#[test]
fn tdd_cross_pod_duplicate_image_pull_deduplication() {
    let temp_dir = TestTempDir::new("tdd_dedup");
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&recipes_dir).unwrap();

    let pod_one = "\
pod one {
    name \"Pod One\"
    path /pods/one
    registry docker.io
}
container *db {
    image postgres:16
}
";
    let pod_two = "\
pod two {
    name \"Pod Two\"
    path /pods/two
    registry docker.io
}
container *db {
    image postgres:16
}
";
    fs::write_file(&recipes_dir.join("one.pod"), pod_one).unwrap();
    fs::write_file(&recipes_dir.join("two.pod"), pod_two).unwrap();

    let mut config =
        Config::from_str("recipes {\n    groups {\n        all one two\n    }\n}\n").unwrap();
    config.recipes.path = recipes_dir;
    config.simulate = true;

    let mut context = Context::new();
    context.config = config;
    context.parameters = vec![String::from("@all")];

    let mut pull_calls = 0_usize;
    let mut counting_pull = |_: &str,
                             prefix: &str,
                             _: bool,
                             stdout: &mut Vec<u8>,
                             _: &mut Vec<u8>|
     -> Result<PullOutcome, PodError> {
        pull_calls += 1;
        log::write_print(stdout, &format!("{prefix}Updated."));
        Ok(PullOutcome::Updated)
    };

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let outcome = execute_update_with(
        &context,
        SummaryKind::LifecycleUpdate,
        &mut counting_pull,
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(outcome, ExecOutcome::Succeeded);
    assert_eq!(
        pull_calls, 1,
        "shared image across pods must only invoke pull_image once"
    );

    let mut stale_pull_calls = 0_usize;
    let mut up_to_date_pull = |_: &str,
                               prefix: &str,
                               _: bool,
                               out: &mut Vec<u8>,
                               _: &mut Vec<u8>|
     -> Result<PullOutcome, PodError> {
        stale_pull_calls += 1;
        log::write_print(out, &format!("{prefix}Up to date."));
        Ok(PullOutcome::UpToDate)
    };
    let mut query_container = |container_name: &str| {
        if container_name == "two-db" {
            Some(String::from("sha256:old"))
        } else {
            Some(String::from("sha256:new"))
        }
    };
    let mut query_image = |_: &str| Some(String::from("sha256:new"));
    let mut cache = PullCache::new();
    let mut runners = UpdateRunners {
        cache: &mut cache,
        pull_image: &mut up_to_date_pull,
        query_container_image: &mut query_container,
        query_image: &mut query_image,
    };
    let mut stale_stdout = Vec::new();
    let mut stale_stderr = Vec::new();
    let stale_outcome = execute_update_with_inspectors(
        &context,
        SummaryKind::LifecycleUpdate,
        &mut runners,
        &mut stale_stdout,
        &mut stale_stderr,
    )
    .unwrap();
    assert_eq!(stale_outcome, ExecOutcome::Succeeded);
    assert_eq!(stale_pull_calls, 1);
    let stale_text = String::from_utf8(stale_stdout).unwrap();
    assert!(stale_text.contains("Pod 'Pod One' ('one') is up to date."));
    assert!(stale_text.contains("Stop container 'two-db': "));
    assert!(stale_text.contains("Start container 'two-db': "));
    assert!(
        stale_text.contains("Checked 2 pods (2 containers): 1 reloaded, 1 up to date, 0 failed.")
    );
}
