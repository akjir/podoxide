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

//! Regression tests for real Bash word-splitting, quoting, suppression, and PSL edge cases in `.pods-completion.bash`.

#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use self::common::TestWorkspace;

const QUOTED_PSL_CONFIG: &str = "\
recipes {
    path ./stacks

    groups {
        all \"api\" 'db'
    }
}
";

const QUOTED_PSL_RECIPE: &str = "\
pod \"Example Stack\" {
    name     \"Example Stack\"
    registry docker.io

    command migrate {
        container 1
        execute   \"run-migrations\"
    }
}

container \"*my db\" {
    image example/db:latest
}

container {
    image example/sidecar:latest
}
";

const SPACED_NAME_CONFIG: &str = "\
recipes {
    path .
    groups {
        custom svc
    }
}
";

const STAGING_CONFIG: &str = "\
recipes {
    path .
    groups {
        backend api db
        all     @backend
    }
}
";

fn completion_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".pods-completion.bash")
}

fn run_completion(workspace: &TestWorkspace, words: &[&str], cword: usize) -> Vec<String> {
    let mut script = String::from("set -u\nsource \"$COMPLETION_SCRIPT\"\nCOMP_WORDS=(");
    for (index, word) in words.iter().enumerate() {
        if index > 0 {
            script.push(' ');
        }
        script.push_str(&shell_quote(word));
    }
    script.push_str(")\n");
    script.push_str(&format!("COMP_CWORD={cword}\n"));
    script.push_str("_pods_completions\n");
    script.push_str("for item in \"${COMPREPLY[@]}\"; do printf '%s\\n' \"$item\"; done\n");

    let script_path = completion_script_path();
    let script_str = script_path.to_str().expect("UTF-8 completion script path");
    let output = workspace.run_binary_with_env(
        Path::new("bash"),
        &["--noprofile", "--norc", "-c", &script],
        b"",
        &[("COMPLETION_SCRIPT", script_str)],
    );
    assert!(
        output.success,
        "completion invocation failed for {words:?} (cword={cword}): stderr={}",
        output.stderr
    );
    assert!(
        output.stderr.is_empty(),
        "completion produced unexpected stderr for {words:?}: {}",
        output.stderr
    );
    output.stdout.lines().map(String::from).collect()
}

fn shell_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

#[test]
fn completion_appends_slash_for_glued_config_directory() {
    let workspace = TestWorkspace::new("comp_glued_dir");
    fs::create_dir_all(workspace.dir().join("sub")).unwrap();
    fs::write(workspace.dir().join("sub/prod.conf"), STAGING_CONFIG).unwrap();
    fs::write(workspace.dir().join("sub/readme.txt"), "ignored\n").unwrap();

    assert_eq!(
        run_completion(&workspace, &["pods", "--config=sub"], 1),
        vec![String::from("--config=sub/")],
        "glued --config=<dir> must append trailing slash"
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "--config=sub/"], 1),
        vec![String::from("--config=sub/prod.conf")],
        "glued --config=<dir>/ must complete .conf files inside directory"
    );
}

#[test]
fn completion_filter_and_sort_ignores_empty_items_without_error() {
    let workspace = TestWorkspace::new("comp_empty_item");
    let script = r#"
set -u
source "$COMPLETION_SCRIPT"
_pods_comp_filter_and_sort "" "" "foo" "" "bar"
for item in "${COMPREPLY[@]}"; do printf '%s\n' "$item"; done
"#;
    let script_path = completion_script_path();
    let script_str = script_path.to_str().expect("UTF-8 completion script path");
    let output = workspace.run_binary_with_env(
        Path::new("bash"),
        &["--noprofile", "--norc", "-c", script],
        b"",
        &[("COMPLETION_SCRIPT", script_str)],
    );
    assert!(output.success, "stderr: {}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "empty item must not emit bad array subscript: {}",
        output.stderr
    );
    let replies: Vec<String> = output.stdout.lines().map(String::from).collect();
    assert_eq!(replies, vec![String::from("bar"), String::from("foo")]);
}

#[test]
fn completion_handles_real_bash_at_split_group_targets() {
    let workspace = TestWorkspace::new("comp_at_split");
    assert!(workspace.run_pods(&["init"], b"").success);
    fs::write(workspace.dir().join("staging.conf"), STAGING_CONFIG).unwrap();

    assert_eq!(
        run_completion(&workspace, &["pods", "up", "@", "a"], 3),
        vec![String::from("@all")],
        "real Bash @-split prefix must keep @ because readline replaces $2='@a'"
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "up", "@"], 2),
        vec![String::from("@all")],
        "bare @ token must complete full @group"
    );
    assert!(
        run_completion(&workspace, &["pods", "up", "@", "r"], 3).is_empty(),
        "real Bash @-split prefix must never suggest bare recipes"
    );
    assert_eq!(
        run_completion(
            &workspace,
            &[
                "pods", "--config", "=", "staging", "up", "@", "all", "@", "b"
            ],
            8,
        ),
        vec![String::from("@backend")],
        "second @-split group must complete remaining @group"
    );
    assert!(
        run_completion(
            &workspace,
            &[
                "pods", "--config", "=", "staging", "up", "@", "all", "@", "a"
            ],
            8,
        )
        .is_empty(),
        "already-present @all group must not be re-suggested"
    );
    assert!(
        run_completion(
            &workspace,
            &["pods", "--config", "=", "staging", "logs", "@", "a"],
            6,
        )
        .is_empty(),
        "logs mode does not accept groups and must not leak recipe matches after @"
    );
}

#[test]
fn completion_handles_real_bash_colon_split_timestamps() {
    let workspace = TestWorkspace::new("comp_colon_split");
    assert!(workspace.run_pods(&["init"], b"").success);

    let cases: &[(&[&str], usize)] = &[
        (
            &[
                "pods",
                "logs",
                "--since",
                "=",
                "2026-01-01T12",
                ":",
                "00",
                ":",
                "00",
                "",
            ],
            9,
        ),
        (
            &[
                "pods",
                "logs",
                "--since",
                "2026-01-01T12",
                ":",
                "00",
                ":",
                "00",
                "",
            ],
            8,
        ),
        (&["pods", "logs", "--until", "=", "12", ":", "30", ""], 7),
    ];

    for &(words, cword) in cases {
        let replies = run_completion(&workspace, words, cword);
        for expected in ["follow", "help", "show", "recipe"] {
            assert!(
                replies.iter().any(|item| item == expected),
                "expected {expected:?} for {words:?} (cword={cword}), got {replies:?}"
            );
        }
    }

    assert!(
        run_completion(
            &workspace,
            &["pods", "logs", "--since", "=", "2026-01-01T12", ":"],
            5,
        )
        .is_empty(),
        "cursor directly on timestamp colon must not suggest actions"
    );
    assert!(
        run_completion(
            &workspace,
            &["pods", "logs", "--since", "=", "2026-01-01T12", ":", "00"],
            6,
        )
        .is_empty(),
        "cursor inside timestamp minutes must not suggest actions"
    );
}

#[test]
fn completion_handles_three_word_config_equals() {
    let workspace = TestWorkspace::new("comp_three_word_eq");
    fs::write(workspace.dir().join("pods.conf"), STAGING_CONFIG).unwrap();
    fs::write(workspace.dir().join("staging.conf"), STAGING_CONFIG).unwrap();
    fs::write(workspace.dir().join("notes.txt"), "ignored\n").unwrap();
    fs::create_dir_all(workspace.dir().join("sub")).unwrap();

    // Real Bash 5.3 splits `pods --config=<Tab>` into 3 words with $2="" after '=',
    // so COMPREPLY must return bare filenames (with `-o filenames`) to avoid `--config==`.
    let replies = run_completion(&workspace, &["pods", "--config", "="], 2);
    assert_eq!(
        replies,
        vec![
            String::from("pods.conf"),
            String::from("staging.conf"),
            String::from("sub"),
        ],
        "real Bash 3-word `pods --config=<Tab>` must complete .conf files and directories without duplicating '='"
    );
}

#[test]
fn completion_loads_quoted_and_escaped_config_paths() {
    let workspace = TestWorkspace::new("comp_quoted_cfg");
    assert!(workspace.run_pods(&["init"], b"").success);
    fs::write(workspace.dir().join("staging.conf"), STAGING_CONFIG).unwrap();
    fs::write(workspace.dir().join("my config.conf"), SPACED_NAME_CONFIG).unwrap();

    let default_targets = vec![String::from("@all"), String::from("recipe")];
    assert_eq!(
        run_completion(
            &workspace,
            &["pods", "--config", "=", "\"pods.conf\"", "up", ""],
            5,
        ),
        default_targets,
        "double-quoted split --config must load dynamic targets"
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "--config=\"pods.conf\"", "up", ""], 3),
        default_targets,
        "double-quoted glued --config must load dynamic targets"
    );

    let staging_targets = vec![
        String::from("@all"),
        String::from("@backend"),
        String::from("api"),
        String::from("db"),
    ];
    assert_eq!(
        run_completion(
            &workspace,
            &["pods", "--config", "=", "'staging.conf'", "up", ""],
            5,
        ),
        staging_targets,
        "single-quoted split --config must load dynamic targets"
    );
    assert_eq!(
        run_completion(
            &workspace,
            &["pods", "--config='staging.conf'", "up", ""],
            3,
        ),
        staging_targets,
        "single-quoted glued --config must load dynamic targets"
    );

    let spaced_targets = vec![String::from("@custom"), String::from("svc")];
    assert_eq!(
        run_completion(
            &workspace,
            &["pods", "--config", "my\\ config.conf", "up", ""],
            4,
        ),
        spaced_targets,
        "backslash-escaped spaced --config must load dynamic targets"
    );
    assert_eq!(
        run_completion(
            &workspace,
            &["pods", "--config", "=", "my\\ config.conf", "up", ""],
            5,
        ),
        spaced_targets,
        "backslash-escaped split --config= must load dynamic targets"
    );
    assert!(
        run_completion(
            &workspace,
            &["pods", "--config", "=", "\"../etc/passwd\"", "up", ""],
            5,
        )
        .is_empty(),
        "quoted path traversal must still be rejected"
    );
}

#[test]
fn completion_parses_quoted_psl_tokens_and_headers() {
    let workspace = TestWorkspace::new("comp_quoted_psl");
    fs::write(workspace.dir().join("pods.conf"), QUOTED_PSL_CONFIG).unwrap();
    fs::create_dir_all(workspace.dir().join("stacks")).unwrap();
    fs::write(workspace.dir().join("stacks/api.pod"), QUOTED_PSL_RECIPE).unwrap();

    assert_eq!(
        run_completion(&workspace, &["pods", "up", ""], 2),
        vec![
            String::from("@all"),
            String::from("api"),
            String::from("db"),
        ],
        "quoted recipe names in groups must complete"
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "up", "api/"], 2),
        vec![String::from("api/2"), String::from("api/my_db")],
        "quoted container header with spaces must normalize and preserve subsequent unnamed indices"
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "up", "api/*"], 2),
        vec![String::from("api/*my_db")],
        "quoted relative container header must complete with * prefix"
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "command", "exec", "api", ""], 4),
        vec![String::from("migrate")],
        "quoted pod header with spaces must still expose nested command blocks"
    );
}

#[test]
fn completion_suppresses_cli_rejected_combinations_and_duplicates() {
    let workspace = TestWorkspace::new("comp_suppress");
    assert!(workspace.run_pods(&["init"], b"").success);

    assert!(
        run_completion(&workspace, &["pods", "status", "--all", ""], 3).is_empty(),
        "status --all must not suggest targets"
    );
    assert!(
        run_completion(&workspace, &["pods", "up", "@all", "recipe/"], 3).is_empty(),
        "mixed container target after glued @group must be suppressed"
    );
    assert!(
        run_completion(&workspace, &["pods", "up", "@", "all", "recipe/"], 4).is_empty(),
        "mixed container target after Bash-split @group must be suppressed"
    );

    let after_container = run_completion(&workspace, &["pods", "up", "recipe/db", ""], 3);
    assert!(
        !after_container.contains(&String::from("@all"))
            && !after_container.contains(&String::from("recipe")),
        "pod/group targets must not be suggested after container target, got {after_container:?}"
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "up", "recipe/db", "recipe/"], 3),
        vec![String::from("recipe/app")],
        "subsequent container completion must exclude already-given container target"
    );

    let after_recipe = run_completion(&workspace, &["pods", "up", "recipe", ""], 3);
    assert!(
        !after_recipe.contains(&String::from("recipe")),
        "already-given recipe must not be re-suggested, got {after_recipe:?}"
    );
    assert_eq!(after_recipe, vec![String::from("@all")]);

    assert_eq!(
        run_completion(&workspace, &["pods", "recipe", "--all", ""], 3),
        vec![String::from("list")],
        "recipe --all must only suggest list"
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "image", "--preview", ""], 3),
        vec![String::from("prune")],
        "image --preview must only suggest prune"
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "image", "--force", ""], 3),
        vec![String::from("prune")],
        "image --force must only suggest prune"
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "image", "--all", ""], 3),
        vec![String::from("prune")],
        "image --all must only suggest prune"
    );
}
