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

//! Unit tests for `pods command exec` and command argument tokenization in [`super`].

use std::env;
use std::fs as std_fs;
use std::path::PathBuf;
use std::process;

use crate::core::config::Config;
use crate::core::container::Container;
use crate::core::context::Context;
use crate::core::modes::command::tokenizer;
use crate::core::pod::PodCommand;
use crate::core::recipe::Recipe;
use crate::error::{CliErrorKind, PodError, RecipeErrorKind};
use crate::io::system::fs;
use super::{exec_command, execute_with, handle};

const COMMANDS_RECIPE_PSL: &str = "\
pod cmd_pod {
    name \"Command Pod\"
    description \"A pod with commands.\"
    path /pods/cmd_pod
    registry docker.io

    command add_index {
        description \"Adds missing database indices.\"
        container *db
        user 33
        execute \"script.sh add-missing-indices\"
    }
    command run_absolute {
        container absolute_db
        execute \"script.sh run-absolute\"
    }
    command run_int_user {
        container absolute_db
        user 1000
        execute \"script.sh run-int-user\"
    }
    command run_int_container {
        container 1
        execute \"script.sh run-int-container\"
    }
    command run_int_container_2 {
        container 2
        execute \"script.sh run-int-container-2\"
    }
    command run_quoted {
        container *db
        execute \"sh -c 'echo hello'\"
    }
    command missing_target_container {
        container *nonexistent
        execute \"script.sh\"
    }
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

fn build_test_context(suffix: &str) -> (PathBuf, Context) {
    let mut directory = env::temp_dir();
    directory.push(format!("podoxide_cmd_exec_{}_{}", process::id(), suffix));
    let _ = std_fs::remove_dir_all(&directory);
    std_fs::create_dir_all(&directory).unwrap();

    let recipe_path = directory.join("recipe_006_commands.pod");
    fs::write_file(&recipe_path, COMMANDS_RECIPE_PSL).unwrap();

    let mut config = Config::new();
    config.simulate = true;
    config.recipes.path = directory.clone();
    config.recipes.groups.insert(
        String::from("all"),
        vec![String::from("recipe_006_commands")],
    );

    let mut context = Context::new();
    context.config = config;
    (directory, context)
}

#[test]
fn exec_command_rejects_flag_injection_and_reports_command_line() {
    let mut recipe = Recipe::new();
    recipe.has_pod = true;
    recipe.pod.id = String::from("cmd_pod");
    recipe.pod.line = 1;

    let mut flag_exec = PodCommand::new();
    flag_exec.name = String::from("flag_exec");
    flag_exec.container = String::from("*db");
    flag_exec.execute = String::from("--privileged sh");
    flag_exec.line = 12;
    recipe
        .pod
        .commands
        .insert(String::from("flag_exec"), flag_exec);

    let mut quoted_flag_exec = PodCommand::new();
    quoted_flag_exec.name = String::from("quoted_flag_exec");
    quoted_flag_exec.container = String::from("*db");
    quoted_flag_exec.execute = String::from("'--privileged' sh");
    recipe
        .pod
        .commands
        .insert(String::from("quoted_flag_exec"), quoted_flag_exec);

    let mut unterminated_exec = PodCommand::new();
    unterminated_exec.name = String::from("unterminated_exec");
    unterminated_exec.container = String::from("*db");
    unterminated_exec.execute = String::from("sh -c 'echo hello");
    recipe
        .pod
        .commands
        .insert(String::from("unterminated_exec"), unterminated_exec);

    let mut flag_user = PodCommand::new();
    flag_user.name = String::from("flag_user");
    flag_user.container = String::from("*db");
    flag_user.execute = String::from("sh");
    flag_user.user = Some(String::from("-u=0"));
    recipe
        .pod
        .commands
        .insert(String::from("flag_user"), flag_user);

    let mut empty_program = PodCommand::new();
    empty_program.name = String::from("empty_program");
    empty_program.container = String::from("*db");
    empty_program.execute = String::from("'' arg");
    recipe
        .pod
        .commands
        .insert(String::from("empty_program"), empty_program);

    let mut trailing_newline = PodCommand::new();
    trailing_newline.name = String::from("trailing_newline");
    trailing_newline.container = String::from("*db");
    trailing_newline.execute = String::from("script.sh\n");
    recipe
        .pod
        .commands
        .insert(String::from("trailing_newline"), trailing_newline);

    let mut trailing_escaped_space = PodCommand::new();
    trailing_escaped_space.name = String::from("trailing_escaped_space");
    trailing_escaped_space.container = String::from("*db");
    trailing_escaped_space.execute = String::from("script.sh arg\\ ");
    recipe.pod.commands.insert(
        String::from("trailing_escaped_space"),
        trailing_escaped_space,
    );

    let mut container = Container::new();
    container.name = String::from("*db");
    container.image = String::from("postgres");
    recipe.containers.push(container);

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    // Command with explicit line=12 reports Some(12).
    let block_error =
        exec_command(&recipe, "flag_exec", true, &mut stdout, &mut stderr).unwrap_err();
    match block_error {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidCommand);
            assert_eq!(recipe_error.line, Some(12));
        }
        other => panic!("expected PodError::Recipe(InvalidCommand), got {other:?}"),
    }

    // Commands with line=0 and missing CLI specs report line=None (never blaming pod line 1).
    for command_spec in [
        "quoted_flag_exec",
        "unterminated_exec",
        "flag_user",
        "empty_program",
        "trailing_newline",
        "missing",
    ] {
        let error =
            exec_command(&recipe, command_spec, true, &mut stdout, &mut stderr).unwrap_err();
        match error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidCommand);
                assert_eq!(
                    recipe_error.line, None,
                    "expected line=None for {command_spec}"
                );
            }
            other => panic!("expected PodError::Recipe(InvalidCommand), got {other:?}"),
        }
    }

    exec_command(
        &recipe,
        "trailing_escaped_space",
        true,
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert!(String::from_utf8(stdout).unwrap().contains("'arg '"));
}

#[test]
fn execute_with_rejects_invalid_commands_and_missing_parameters() {
    let (directory, mut context) = build_test_context("errors");

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let empty_error = execute_with(&context, &mut stdout, &mut stderr).unwrap_err();
    match empty_error {
        PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
        other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
    }

    context.parameters = vec![String::from("recipe_006_commands")];
    let one_parameter_error = execute_with(&context, &mut stdout, &mut stderr).unwrap_err();
    match one_parameter_error {
        PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
        other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
    }

    // Missing CLI command specs must not blame line 1 of the recipe file.
    for invalid_spec in ["unknown", "typo", "0", "99"] {
        context.parameters = vec![
            String::from("recipe_006_commands"),
            String::from(invalid_spec),
        ];
        let error = execute_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidCommand);
                assert_eq!(
                    recipe_error.line, None,
                    "missing CLI command {invalid_spec:?} must not blame line 1"
                );
            }
            other => panic!(
                "expected PodError::Recipe(InvalidCommand) for {invalid_spec}, got {other:?}"
            ),
        }
    }

    fs::write_file(
        &directory.join("missing_container.pod"),
        "pod cmd_pod {\n    name \"Command Pod\"\n    path /pods/cmd_pod\n    registry docker.io\n    command missing_container {\n        execute \"script.sh no-container\"\n    }\n}\ncontainer *db {\n    image postgres\n}\n",
    )
    .unwrap();
    fs::write_file(
        &directory.join("missing_execute.pod"),
        "pod cmd_pod {\n    name \"Command Pod\"\n    path /pods/cmd_pod\n    registry docker.io\n    command missing_execute {\n        container *db\n    }\n}\ncontainer *db {\n    image postgres\n}\n",
    )
    .unwrap();
    context.config.recipes.groups.insert(
        String::from("incomplete"),
        vec![
            String::from("missing_container"),
            String::from("missing_execute"),
        ],
    );
    for incomplete_recipe in ["missing_container", "missing_execute"] {
        context.parameters = vec![
            String::from(incomplete_recipe),
            String::from(incomplete_recipe),
        ];
        let error = execute_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidCommand);
                assert_eq!(recipe_error.line, Some(5));
            }
            other => panic!(
                "expected PodError::Recipe(InvalidCommand) at line 5 for {incomplete_recipe}, got {other:?}"
            ),
        }
    }

    context.parameters = vec![
        String::from("recipe_006_commands"),
        String::from("missing_target_container"),
    ];
    let container_error = execute_with(&context, &mut stdout, &mut stderr).unwrap_err();
    match container_error {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotFound);
        }
        other => panic!("expected PodError::Recipe(ContainerNotFound), got {other:?}"),
    }

    let _ = std_fs::remove_dir_all(&directory);
}

#[test]
fn execute_with_simulates_named_and_indexed_commands() {
    let (directory, mut context) = build_test_context("simulate");

    let cases = [
        (
            "add_index",
            "Execute command 'script.sh add-missing-indices' in container 'cmd_pod-db': \npodman exec -it -u 33 cmd_pod-db script.sh add-missing-indices\n",
        ),
        (
            "run_absolute",
            "Execute command 'script.sh run-absolute' in container 'absolute_db': \npodman exec -it absolute_db script.sh run-absolute\n",
        ),
        (
            "run_int_user",
            "Execute command 'script.sh run-int-user' in container 'absolute_db': \npodman exec -it -u 1000 absolute_db script.sh run-int-user\n",
        ),
        (
            "run_int_container",
            "Execute command 'script.sh run-int-container' in container 'cmd_pod-1': \npodman exec -it cmd_pod-1 script.sh run-int-container\n",
        ),
        (
            "run_int_container_2",
            "Execute command 'script.sh run-int-container-2' in container 'cmd_pod-db': \npodman exec -it cmd_pod-db script.sh run-int-container-2\n",
        ),
        (
            "run_quoted",
            "Execute command 'sh -c 'echo hello'' in container 'cmd_pod-db': \npodman exec -it cmd_pod-db sh -c 'echo hello'\n",
        ),
        (
            "1",
            "Execute command 'script.sh add-missing-indices' in container 'cmd_pod-db': \npodman exec -it -u 33 cmd_pod-db script.sh add-missing-indices\n",
        ),
        (
            "3",
            "Execute command 'script.sh run-absolute' in container 'absolute_db': \npodman exec -it absolute_db script.sh run-absolute\n",
        ),
    ];

    for (command_spec, expected_output) in cases {
        context.parameters = vec![
            String::from("recipe_006_commands"),
            String::from(command_spec),
        ];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        execute_with(&context, &mut stdout, &mut stderr).unwrap();
        assert_eq!(String::from_utf8(stdout).unwrap(), expected_output);
        assert!(stderr.is_empty());
    }

    context.parameters = vec![
        String::from("recipe_006_commands"),
        String::from("add_index"),
    ];
    assert!(handle(&context).is_ok());

    let _ = std_fs::remove_dir_all(&directory);
}

#[test]
fn split_execute_tokens_preserves_quoted_arguments() {
    assert_eq!(
        tokenizer::split_execute_tokens("sh -c 'echo hello world'").unwrap(),
        vec!["sh", "-c", "echo hello world"]
    );
    assert_eq!(
        tokenizer::split_execute_tokens("psql -U db -c \"SELECT 'a b';\"").unwrap(),
        vec!["psql", "-U", "db", "-c", "SELECT 'a b';"]
    );
    assert_eq!(
        tokenizer::split_execute_tokens("printf \"%s\\n\" \"a\\\"b\\\\c\" arg\\ with\\ space")
            .unwrap(),
        vec!["printf", "%s\\n", "a\"b\\c", "arg with space"]
    );
    assert_eq!(
        tokenizer::split_execute_tokens("cmd '' \"\"").unwrap(),
        vec!["cmd", "", ""]
    );
    assert!(tokenizer::split_execute_tokens("sh -c 'unterminated").is_none());
    assert!(tokenizer::split_execute_tokens("sh -c \"unterminated").is_none());
    assert!(tokenizer::split_execute_tokens("sh -c \"escape\\").is_none());
    assert!(tokenizer::split_execute_tokens("sh -c \\").is_none());

    let max_tokens_input = vec!["arg"; 64].join(" ");
    assert_eq!(
        tokenizer::split_execute_tokens(&max_tokens_input)
            .unwrap()
            .len(),
        64
    );

    let overflow_tokens_input = vec!["arg"; 65].join(" ");
    assert!(tokenizer::split_execute_tokens(&overflow_tokens_input).is_none());

    let mid_stream_overflow = "arg ".repeat(65);
    assert!(tokenizer::split_execute_tokens(&mid_stream_overflow).is_none());
}
