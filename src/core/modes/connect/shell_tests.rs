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

//! Unit tests for `pods connect [shell] <target>` in [`super`].

use std::env;
use std::fs as std_fs;
use std::path::PathBuf;
use std::process;

use crate::core::config::Config;
use crate::core::context::Context;
use crate::error::{CliErrorKind, ConfigErrorKind, PodError, RecipeErrorKind};
use crate::io::system::fs;
use super::{execute_with, handle};

const INVALID_RECIPE_PSL: &str = "\
pod invalid_pod {
    name \"Invalid Pod\"
}
";

const MULTI_CONTAINER_RECIPE_PSL: &str = "\
pod cmd_pod {
    name \"Command Pod\"
    description \"A pod with multiple containers.\"
    path /pods/cmd_pod
    registry docker.io
}
container {
    image alpine
    restart never
}
container *db {
    image postgres
    restart never
}
container absolute_db {
    image mariadb
    restart never
}
";

const SINGLE_CONTAINER_RECIPE_PSL: &str = "\
pod si_po {
    name \"Single Pod\"
    path /pods/si_po
    registry docker.io
}
container {
    image alpine
    restart never
}
";

fn build_test_context(suffix: &str) -> (PathBuf, Context) {
    let mut directory = env::temp_dir();
    directory.push(format!(
        "podoxide_connect_shell_{}_{}",
        process::id(),
        suffix
    ));
    let _ = std_fs::remove_dir_all(&directory);
    std_fs::create_dir_all(&directory).unwrap();

    fs::write_file(
        &directory.join("recipe_001_valid.pod"),
        SINGLE_CONTAINER_RECIPE_PSL,
    )
    .unwrap();
    fs::write_file(
        &directory.join("recipe_006_commands.pod"),
        MULTI_CONTAINER_RECIPE_PSL,
    )
    .unwrap();
    fs::write_file(
        &directory.join("recipe_002_invalid.pod"),
        INVALID_RECIPE_PSL,
    )
    .unwrap();
    // Unreferenced .pod file on disk NOT in config.recipes.groups.
    fs::write_file(&directory.join("unmanaged_app.pod"), "invalid {{{ psl").unwrap();

    let mut config = Config::new();
    config.simulate = true;
    config.recipes.path = directory.clone();
    config.recipes.groups.insert(
        String::from("all"),
        vec![
            String::from("recipe_001_valid"),
            String::from("recipe_006_commands"),
            String::from("recipe_002_invalid"),
            String::from("registered_missing"),
        ],
    );

    let mut context = Context::new();
    context.config = config;
    (directory, context)
}

#[test]
fn execute_with_rejects_invalid_parameters_and_targets() {
    let (directory, mut context) = build_test_context("invalid_params");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let missing_error =
        execute_with(&context, |_| true, |_| true, &mut stdout, &mut stderr).unwrap_err();
    match missing_error {
        PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
        other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
    }

    context.parameters = vec![String::from("   ")];
    let blank_error =
        execute_with(&context, |_| true, |_| true, &mut stdout, &mut stderr).unwrap_err();
    match blank_error {
        PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::InvalidTarget),
        other => panic!("expected PodError::Cli(InvalidTarget), got {other:?}"),
    }

    context.parameters = vec![
        String::from("recipe_001_valid"),
        String::from("extra_parameter"),
    ];
    let extra_error =
        execute_with(&context, |_| true, |_| true, &mut stdout, &mut stderr).unwrap_err();
    match extra_error {
        PodError::Cli(cli_error) => {
            assert_eq!(cli_error.kind, CliErrorKind::TooManyArguments);
        }
        other => panic!("expected PodError::Cli(TooManyArguments), got {other:?}"),
    }

    for invalid_target in [
        "recipe_001_valid/",
        "/db",
        "recipe_001_valid/db/extra",
        "recipe_001_valid/-flag",
        "Invalid Container Name!",
        "-flag",
        "_leading_underscore",
        "bad..name",
    ] {
        context.parameters = vec![String::from(invalid_target)];
        let error =
            execute_with(&context, |_| true, |_| true, &mut stdout, &mut stderr).unwrap_err();
        match error {
            PodError::Cli(cli_error) => {
                assert_eq!(cli_error.kind, CliErrorKind::InvalidTarget);
            }
            other => panic!(
                "expected PodError::Cli(InvalidTarget) for {invalid_target:?}, got {other:?}"
            ),
        }
    }

    let _ = std_fs::remove_dir_all(&directory);
}

#[test]
fn execute_with_rejects_missing_or_invalid_recipes_and_distinguishes_stopped_unmanaged_containers()
{
    let (directory, mut context) = build_test_context("recipe_errors");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    context.parameters = vec![String::from("recipe_006_commands")];
    let multi_error =
        execute_with(&context, |_| true, |_| true, &mut stdout, &mut stderr).unwrap_err();
    match multi_error {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::MultipleContainers);
        }
        other => panic!("expected PodError::Recipe(MultipleContainers), got {other:?}"),
    }

    context.parameters = vec![String::from("recipe_006_commands/nonexistent")];
    let not_found_container =
        execute_with(&context, |_| true, |_| true, &mut stdout, &mut stderr).unwrap_err();
    match not_found_container {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotFound);
        }
        other => panic!("expected PodError::Recipe(ContainerNotFound), got {other:?}"),
    }

    // Unregistered `<recipe>/<container>` target fails with ConfigErrorKind::RecipeNotFound.
    context.parameters = vec![String::from("nonexistent_recipe/1")];
    let unregistered_recipe =
        execute_with(&context, |_| true, |_| true, &mut stdout, &mut stderr).unwrap_err();
    match unregistered_recipe {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
        }
        other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
    }

    // Registered recipe missing its `.pod` file fails with RecipeErrorKind::NotFound.
    context.parameters = vec![String::from("registered_missing/1")];
    let missing_file =
        execute_with(&context, |_| true, |_| true, &mut stdout, &mut stderr).unwrap_err();
    match missing_file {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::NotFound);
        }
        other => panic!("expected PodError::Recipe(NotFound), got {other:?}"),
    }

    context.parameters = vec![String::from("recipe_002_invalid")];
    let invalid_recipe =
        execute_with(&context, |_| true, |_| true, &mut stdout, &mut stderr).unwrap_err();
    match invalid_recipe {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::MissingPodRegistry);
        }
        other => panic!("expected PodError::Recipe(MissingPodRegistry), got {other:?}"),
    }

    context.config.simulate = false;
    context.parameters = vec![String::from("recipe_001_valid")];
    let not_running_recipe = execute_with(
        &context,
        |_| false,
        |_| panic!("container_exists must not be called for managed recipes"),
        &mut stdout,
        &mut stderr,
    )
    .unwrap_err();
    match not_running_recipe {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotRunning);
            assert_eq!(recipe_error.token.as_deref(), Some("si_po-1"));
        }
        other => panic!("expected PodError::Recipe(ContainerNotRunning), got {other:?}"),
    }

    // Stopped unmanaged container exists in Podman -> CliErrorKind::ContainerNotRunning.
    context.parameters = vec![String::from("my_stopped_container")];
    let stopped_direct =
        execute_with(&context, |_| false, |_| true, &mut stdout, &mut stderr).unwrap_err();
    match stopped_direct {
        PodError::Cli(cli_error) => {
            assert_eq!(cli_error.kind, CliErrorKind::ContainerNotRunning);
            assert_eq!(cli_error.argument.as_deref(), Some("my_stopped_container"));
        }
        other => panic!("expected PodError::Cli(ContainerNotRunning), got {other:?}"),
    }

    // Nonexistent unmanaged container -> CliErrorKind::TargetNotFound.
    context.parameters = vec![String::from("my_custom_container")];
    let missing_direct =
        execute_with(&context, |_| false, |_| false, &mut stdout, &mut stderr).unwrap_err();
    match missing_direct {
        PodError::Cli(cli_error) => {
            assert_eq!(cli_error.kind, CliErrorKind::TargetNotFound);
            assert_eq!(cli_error.argument.as_deref(), Some("my_custom_container"));
        }
        other => panic!("expected PodError::Cli(TargetNotFound), got {other:?}"),
    }

    let _ = std_fs::remove_dir_all(&directory);
}

#[test]
fn execute_with_simulates_single_multi_direct_and_unreferenced_pod_targets() {
    let (directory, mut context) = build_test_context("simulate");

    let cases = [
        (
            "recipe_001_valid",
            "Execute connect command: \npodman exec -it si_po-1 sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
        (
            "recipe_006_commands/1",
            "Execute connect command: \npodman exec -it cmd_pod-1 sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
        (
            "recipe_006_commands/2",
            "Execute connect command: \npodman exec -it cmd_pod-db sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
        (
            "recipe_006_commands/*db",
            "Execute connect command: \npodman exec -it cmd_pod-db sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
        (
            "recipe_006_commands/db",
            "Execute connect command: \npodman exec -it cmd_pod-db sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
        (
            "recipe_006_commands/absolute_db",
            "Execute connect command: \npodman exec -it absolute_db sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
        (
            "my_custom_container",
            "Execute connect command: \npodman exec -it my_custom_container sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
        (
            "MyApp_1",
            "Execute connect command: \npodman exec -it MyApp_1 sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
        // Unreferenced `unmanaged_app.pod` on disk must NOT hijack direct container name resolution.
        (
            "unmanaged_app",
            "Execute connect command: \npodman exec -it unmanaged_app sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
    ];

    for (target, expected_output) in cases {
        context.parameters = vec![String::from(target)];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        execute_with(&context, |_| false, |_| false, &mut stdout, &mut stderr).unwrap();
        assert_eq!(String::from_utf8(stdout).unwrap(), expected_output);
        assert!(stderr.is_empty());
    }

    context.parameters = vec![String::from("recipe_001_valid")];
    assert!(handle(&context).is_ok());

    context.config.recipes.path = PathBuf::from("../invalid_dir");
    context.parameters = vec![String::from("my_custom_container")];
    let mut fallback_stdout = Vec::new();
    let mut fallback_stderr = Vec::new();
    execute_with(
        &context,
        |_| false,
        |_| false,
        &mut fallback_stdout,
        &mut fallback_stderr,
    )
    .unwrap();
    assert!(
        String::from_utf8(fallback_stdout)
            .unwrap()
            .contains("podman exec -it my_custom_container")
    );

    let _ = std_fs::remove_dir_all(&directory);
}
