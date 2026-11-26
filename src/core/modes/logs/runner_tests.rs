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

//! Unit tests for `pods logs` target validation, flag validation, and execution in [`super`].

use crate::core::config::Config;
use crate::core::context::{Context, Flags};
use crate::error::{CliErrorKind, ConfigErrorKind, PodError, RecipeErrorKind};
use crate::io::exec::command::ExecOutcome;
use crate::io::system::fs::{self, TestTempDir};
use super::execute_logs_with;

const COMMANDS_RECIPE_PSL: &str = "\
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

const INVALID_RECIPE_PSL: &str = "\
pod invalid_pod {
    name \"Invalid Pod\"
}
";

const SIMPLE_RECIPE_PSL: &str = "\
pod simple_container {
    name \"Simple Container\"
    path /pods/simple_container
    registry docker.io
}
container {
    image alpine
    restart never
}
";

fn assert_invalid_flag_rejected(context: &Context, flags: Flags) {
    let mut local_context = context.clone();
    local_context.flags = flags;
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let error = execute_logs_with(&local_context, false, &mut stdout, &mut stderr).unwrap_err();
    match error {
        PodError::Cli(cli_error) => {
            assert_eq!(cli_error.kind, CliErrorKind::InvalidFlagValue);
        }
        other => panic!("expected PodError::Cli(InvalidFlagValue), got {other:?}"),
    }
}

fn build_test_context(suffix: &str) -> (TestTempDir, Context) {
    let directory = TestTempDir::new(&format!("mode_logs_{suffix}"));

    fs::write_file(
        &directory.join("recipe_011_simple_container.pod"),
        SIMPLE_RECIPE_PSL,
    )
    .unwrap();
    fs::write_file(
        &directory.join("recipe_006_commands.pod"),
        COMMANDS_RECIPE_PSL,
    )
    .unwrap();
    fs::write_file(
        &directory.join("recipe_002_invalid.pod"),
        INVALID_RECIPE_PSL,
    )
    .unwrap();
    fs::write_file(&directory.join("unregistered.pod"), SIMPLE_RECIPE_PSL).unwrap();

    let mut config = Config::new();
    config.simulate = true;
    config.recipes.path = directory.to_path_buf();
    config.recipes.groups.insert(
        String::from("all"),
        vec![
            String::from("recipe_011_simple_container"),
            String::from("recipe_006_commands"),
            String::from("recipe_002_invalid"),
            String::from("missing_recipe"),
        ],
    );

    let mut context = Context::new();
    context.config = config;
    (directory, context)
}

#[test]
fn execute_logs_with_rejects_invalid_targets_flags_unregistered_and_invalid_recipes() {
    let (_directory, mut context) = build_test_context("errors");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let missing_error = execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
    match missing_error {
        PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
        other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
    }

    context.parameters = vec![
        String::from("recipe_011_simple_container"),
        String::from("recipe_006_commands"),
    ];
    let extra_error = execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
    match extra_error {
        PodError::Cli(cli_error) => {
            assert_eq!(cli_error.kind, CliErrorKind::TooManyArguments);
        }
        other => panic!("expected PodError::Cli(TooManyArguments), got {other:?}"),
    }

    for invalid_target in [
        "",
        "   ",
        "@all",
        "@backend/db",
        "-flag",
        "recipe_006_commands/",
        "/db",
        "recipe_006_commands/db/extra",
        "recipe_006_commands/-f",
        "recipe_006_commands/@db",
        "recipe_006_commands/..",
        "../recipe_006_commands",
    ] {
        context.parameters = vec![String::from(invalid_target)];
        let error = execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
        match error {
            PodError::Cli(cli_error) => {
                assert_eq!(cli_error.kind, CliErrorKind::InvalidTarget);
            }
            other => panic!(
                "expected PodError::Cli(InvalidTarget) for {invalid_target:?}, got {other:?}"
            ),
        }
    }

    // Unregistered recipe on disk is rejected with ConfigErrorKind::RecipeNotFound.
    context.parameters = vec![String::from("unregistered")];
    let unregistered_err =
        execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
    match unregistered_err {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
        }
        other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
    }

    // Registered recipe missing its `.pod` file fails with RecipeErrorKind::NotFound.
    context.parameters = vec![String::from("missing_recipe")];
    let not_found_recipe =
        execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
    match not_found_recipe {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::NotFound);
        }
        other => panic!("expected PodError::Recipe(NotFound), got {other:?}"),
    }

    // Invalid log flags are rejected BEFORE loading or validating any `.pod` file on disk.
    context.parameters = vec![String::from("missing_recipe")];
    for bad_time in ["-f", " -f", "", "   ", "10\n20"] {
        let mut since_flags = Flags::new();
        since_flags.since = Some(String::from(bad_time));
        assert_invalid_flag_rejected(&context, since_flags);

        let mut until_flags = Flags::new();
        until_flags.until = Some(String::from(bad_time));
        assert_invalid_flag_rejected(&context, until_flags);
    }
    for bad_tail in ["-f", "", "10\n20", "two words", "abc"] {
        let mut tail_flags = Flags::new();
        tail_flags.tail = Some(String::from(bad_tail));
        assert_invalid_flag_rejected(&context, tail_flags);
    }

    context.parameters = vec![String::from("recipe_002_invalid")];
    let invalid_recipe = execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
    match invalid_recipe {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::MissingPodRegistry);
        }
        other => panic!("expected PodError::Recipe(MissingPodRegistry), got {other:?}"),
    }

    context.parameters = vec![String::from("recipe_006_commands/missing")];
    let not_found_container =
        execute_logs_with(&context, false, &mut stdout, &mut stderr).unwrap_err();
    match not_found_container {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotFound);
        }
        other => panic!("expected PodError::Recipe(ContainerNotFound), got {other:?}"),
    }
}

#[test]
fn execute_logs_with_simulates_show_follow_options_and_container_filters() {
    let (_directory, mut context) = build_test_context("simulate");

    let mut option_flags = Flags::new();
    option_flags.since = Some(String::from("2026-01-01 12:00:00"));
    option_flags.tail = Some(String::from("all"));
    option_flags.until = Some(String::from("2026-01-01 13:00:00"));
    option_flags.set_timestamps(true);

    let cases = [
        (
            false,
            Flags::new(),
            "recipe_011_simple_container",
            "Execute log command: \npodman pod logs -n --color simple_container\n",
        ),
        (
            true,
            Flags::new(),
            "recipe_011_simple_container",
            "Execute log command: \npodman pod logs -n --color -f simple_container\n",
        ),
        (
            false,
            option_flags,
            "recipe_011_simple_container",
            "Execute log command: \npodman pod logs -n --color --since '2026-01-01 12:00:00' --until '2026-01-01 13:00:00' --tail -1 --timestamps simple_container\n",
        ),
        (
            false,
            Flags::new(),
            "recipe_006_commands/1",
            "Execute log command: \npodman pod logs -n --color -c cmd_pod-1 cmd_pod\n",
        ),
        (
            false,
            Flags::new(),
            "recipe_006_commands/*db",
            "Execute log command: \npodman pod logs -n --color -c cmd_pod-db cmd_pod\n",
        ),
        (
            false,
            Flags::new(),
            "recipe_006_commands/db",
            "Execute log command: \npodman pod logs -n --color -c cmd_pod-db cmd_pod\n",
        ),
        (
            false,
            Flags::new(),
            "recipe_006_commands/cmd_pod-db",
            "Execute log command: \npodman pod logs -n --color -c cmd_pod-db cmd_pod\n",
        ),
        (
            true,
            Flags::new(),
            "recipe_006_commands/absolute_db",
            "Execute log command: \npodman pod logs -n --color -f -c absolute_db cmd_pod\n",
        ),
    ];

    for (follow, flags, target, expected_output) in cases {
        context.flags = flags;
        context.parameters = vec![String::from(target)];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            execute_logs_with(&context, follow, &mut stdout, &mut stderr).unwrap(),
            ExecOutcome::Succeeded
        );
        assert_eq!(String::from_utf8(stdout).unwrap(), expected_output);
        assert!(stderr.is_empty());
    }
}
