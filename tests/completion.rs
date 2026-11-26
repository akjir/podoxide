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

//! Integration tests for `.pods-completion.bash` executed in isolated Bash subshells.

#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use self::common::TestWorkspace;

const CUSTOM_CONFIG: &str = "\
# Custom staging configuration
recipes {
    path ./stacks

    groups {
        # Commented line should be ignored
        backend db api
        all     @backend # trailing comment
    }
}
";

const CUSTOM_RECIPE: &str = "\
pod api-stack {
    name     \"API Stack\"
    registry docker.io

    # command fake_commented {
    command reload_cfg {
        container *web
        execute   \"reload\"
    }
    command status_check {
        container 3
        execute   \"check\"
    }
}

# container *ignored {
container *web {
    image example/web:latest
}
container custom-worker {
    image example/worker:latest
}
container {
    image example/sidecar:latest
}
";

const UNSAFE_CONF: &str = "\
recipes {
    path .
    groups {
        all $(touch pwned_conf) recipe
    }
}
";

struct FlagCase<'a> {
    cword: usize,
    expected: &'a [&'a str],
    forbidden: &'a [&'a str],
    words: &'a [&'a str],
}

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
fn completion_does_not_leak_shell_variables() {
    let workspace = TestWorkspace::new("completion_no_leak");
    let init = workspace.run_pods(&["init"], b"");
    assert!(init.success, "pods init failed: {}", init.stderr);

    let script = r#"
set -u
source "$COMPLETION_SCRIPT"
i=999; word=keep_word; mode=keep_mode; action=keep_action
opts=keep_opts; cur=keep_cur; prev=keep_prev; LC_ALL=en_US.UTF-8
COMP_WORDS=(pods up "")
COMP_CWORD=2
_pods_completions
printf 'i=%s word=%s mode=%s action=%s opts=%s cur=%s prev=%s lc=%s\n' \
    "$i" "$word" "$mode" "$action" "$opts" "$cur" "$prev" "$LC_ALL"
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
    assert_eq!(
        output.stdout,
        "i=999 word=keep_word mode=keep_mode action=keep_action opts=keep_opts cur=keep_cur prev=keep_prev lc=en_US.UTF-8\n",
        "completion helper leaked shell variables"
    );
}

#[test]
fn completion_handles_config_flag_equals_and_filters_conf_files() {
    let workspace = TestWorkspace::new("completion_config_flag");
    fs::write(workspace.dir().join("pods.conf"), CUSTOM_CONFIG).unwrap();
    fs::write(workspace.dir().join("staging.conf"), CUSTOM_CONFIG).unwrap();
    fs::write(workspace.dir().join("notes.txt"), "ignored\n").unwrap();
    fs::create_dir_all(workspace.dir().join("configs")).unwrap();
    fs::write(workspace.dir().join("configs/prod.conf"), CUSTOM_CONFIG).unwrap();
    fs::write(workspace.dir().join("configs/readme.md"), "ignored\n").unwrap();

    // Default COMP_WORDBREAKS splits '=' into its own token.
    let split_all = run_completion(&workspace, &["pods", "--config", "=", ""], 3);
    assert!(split_all.contains(&String::from("pods.conf")));
    assert!(split_all.contains(&String::from("staging.conf")));
    assert!(
        split_all
            .iter()
            .any(|item| item == "configs" || item == "configs/")
    );
    assert!(!split_all.contains(&String::from("notes.txt")));
    assert!(!split_all.contains(&String::from("logs")));

    let split_prefix = run_completion(&workspace, &["pods", "--config", "=", "st"], 3);
    assert_eq!(split_prefix, vec![String::from("staging.conf")]);

    let split_subdir = run_completion(&workspace, &["pods", "--config", "=", "configs/"], 3);
    assert_eq!(split_subdir, vec![String::from("configs/prod.conf")]);

    // Space-separated --config <file> filters non-.conf regular files too.
    let spaced = run_completion(&workspace, &["pods", "--config", ""], 2);
    assert!(spaced.contains(&String::from("pods.conf")));
    assert!(!spaced.contains(&String::from("notes.txt")));

    // Glued --config=<prefix> when '=' is not in COMP_WORDBREAKS.
    let glued = run_completion(&workspace, &["pods", "--config=st"], 1);
    assert_eq!(glued, vec![String::from("--config=staging.conf")]);
}

#[test]
fn completion_rejects_unknown_first_word_and_sorts_lists() {
    let workspace = TestWorkspace::new("completion_unknown");

    for (words, cword) in [
        (&["pods", "typo", ""][..], 2),
        (&["pods", "typo", "-"][..], 2),
        (&["pods", "--debug", "typo", ""][..], 3),
        (&["pods", "config", "typo", ""][..], 3),
        (&["pods", "init", "typo", ""][..], 3),
        (&["pods", "image", "typo", ""][..], 3),
    ] {
        assert!(
            run_completion(&workspace, words, cword).is_empty(),
            "expected empty completion for {words:?}"
        );
    }

    let top_level = run_completion(&workspace, &["pods", ""], 1);
    let mut sorted_top = top_level.clone();
    sorted_top.sort();
    assert_eq!(
        top_level, sorted_top,
        "top-level completions must be sorted"
    );
    assert!(top_level.contains(&String::from("command")));
    assert!(top_level.contains(&String::from("version")));
}

#[test]
fn completion_scopes_flags_by_mode_and_action() {
    let workspace = TestWorkspace::new("completion_flags");
    let init = workspace.run_pods(&["init"], b"");
    assert!(init.success, "pods init failed: {}", init.stderr);

    let cases = [
        FlagCase {
            words: &["pods", "up", "-"],
            cword: 2,
            expected: &["--config", "--debug", "--simulate"],
            forbidden: &["--all", "--full", "--force", "--preview", "--tail"],
        },
        FlagCase {
            words: &["pods", "status", "-"],
            cword: 2,
            expected: &["--all", "--config", "--debug", "--full", "--simulate"],
            forbidden: &["--force", "--preview", "--tail"],
        },
        FlagCase {
            words: &["pods", "status", "recipe", "-"],
            cword: 3,
            expected: &["--config", "--debug", "--full", "--simulate"],
            forbidden: &["--all"],
        },
        FlagCase {
            words: &["pods", "image", "update", "-"],
            cword: 3,
            expected: &["--config", "--debug", "--simulate"],
            forbidden: &["--all", "--force", "--preview"],
        },
        FlagCase {
            words: &["pods", "image", "prune", "-"],
            cword: 3,
            expected: &[
                "--all",
                "--config",
                "--debug",
                "--force",
                "--preview",
                "--simulate",
            ],
            forbidden: &["--full", "--tail"],
        },
        FlagCase {
            words: &["pods", "image", "prune", "--preview", "-"],
            cword: 4,
            expected: &["--all", "--config", "--debug", "--simulate"],
            forbidden: &["--force"],
        },
        FlagCase {
            words: &["pods", "image", "prune", "--force", "-"],
            cword: 4,
            expected: &["--all", "--config", "--debug", "--simulate"],
            forbidden: &["--preview"],
        },
        FlagCase {
            words: &["pods", "recipe", "list", "-"],
            cword: 3,
            expected: &["--all", "--config", "--debug", "--simulate"],
            forbidden: &["--full", "--force"],
        },
        FlagCase {
            words: &["pods", "recipe", "show", "-"],
            cword: 3,
            expected: &["--config", "--debug", "--simulate"],
            forbidden: &["--all"],
        },
        FlagCase {
            words: &["pods", "logs", "show", "-"],
            cword: 3,
            expected: &[
                "--config",
                "--debug",
                "--simulate",
                "--since",
                "--tail",
                "--timestamps",
                "--until",
            ],
            forbidden: &["--all", "--full"],
        },
        FlagCase {
            words: &["pods", "logs", "help", "-"],
            cword: 3,
            expected: &["--config", "--debug"],
            forbidden: &["--simulate", "--since", "--tail", "--timestamps", "--until"],
        },
        FlagCase {
            words: &["pods", "init", "create", "-"],
            cword: 3,
            expected: &["--debug", "--simulate"],
            forbidden: &["--config", "--all"],
        },
        FlagCase {
            words: &["pods", "init", "help", "-"],
            cword: 3,
            expected: &["--debug"],
            forbidden: &["--config", "--simulate"],
        },
        FlagCase {
            words: &["pods", "version", "-"],
            cword: 2,
            expected: &["--config", "--debug"],
            forbidden: &["--simulate", "--all", "--full"],
        },
        FlagCase {
            words: &["pods", "help", "-"],
            cword: 2,
            expected: &["--config", "--debug"],
            forbidden: &["--simulate", "--all", "--full"],
        },
    ];

    for case in &cases {
        let replies = run_completion(&workspace, case.words, case.cword);
        for flag in case.expected {
            assert!(
                replies.iter().any(|item| item == flag),
                "expected flag {flag} for {:?}, got {replies:?}",
                case.words
            );
        }
        for flag in case.forbidden {
            assert!(
                !replies.iter().any(|item| item == flag),
                "forbidden flag {flag} leaked for {:?}, got {replies:?}",
                case.words
            );
        }
    }
}

#[test]
fn completion_skips_equals_flag_values_when_detecting_mode_and_action() {
    let workspace = TestWorkspace::new("completion_equals_scan");
    let init = workspace.run_pods(&["init"], b"");
    assert!(init.success, "pods init failed: {}", init.stderr);

    for (words, cword) in [
        (&["pods", "logs", "--tail", "=", "50", ""][..], 5),
        (&["pods", "logs", "--tail=50", ""][..], 3),
    ] {
        let replies = run_completion(&workspace, words, cword);
        for action in ["follow", "help", "show"] {
            assert!(
                replies.iter().any(|item| item == action),
                "expected logs sub-action {action} for {words:?}, got {replies:?}"
            );
        }
    }

    let after_config = run_completion(&workspace, &["pods", "--config", "=", "pods.conf", ""], 4);
    assert!(
        after_config.contains(&String::from("up")),
        "expected default action after --config=pods.conf, got {after_config:?}"
    );
}

#[test]
fn completion_suggests_dynamic_targets_containers_and_commands() {
    let workspace = TestWorkspace::new("completion_dynamic");
    let init = workspace.run_pods(&["init"], b"");
    assert!(init.success, "pods init failed: {}", init.stderr);

    assert_eq!(
        run_completion(&workspace, &["pods", "up", ""], 2),
        vec![String::from("@all"), String::from("recipe")]
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "up", "recipe/"], 2),
        vec![String::from("recipe/app"), String::from("recipe/db")]
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "up", "recipe/*"], 2),
        vec![String::from("recipe/*app"), String::from("recipe/*db")]
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "connect", "recipe/"], 2),
        vec![String::from("recipe/app"), String::from("recipe/db")]
    );

    let expected_cmds = vec![
        String::from("backup"),
        String::from("clear_cache"),
        String::from("migrate"),
    ];
    assert_eq!(
        run_completion(&workspace, &["pods", "cmd", "exec", "recipe", ""], 4),
        expected_cmds
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "command", "recipe", ""], 3),
        expected_cmds
    );
    assert_eq!(
        run_completion(&workspace, &["pods", "recipe", "show", ""], 3),
        vec![String::from("recipe")]
    );

    // Custom --config with custom recipes.path, named/unnamed containers, and comments
    fs::write(workspace.dir().join("staging.conf"), CUSTOM_CONFIG).unwrap();
    fs::create_dir_all(workspace.dir().join("stacks")).unwrap();
    fs::write(workspace.dir().join("stacks/api.pod"), CUSTOM_RECIPE).unwrap();

    assert_eq!(
        run_completion(
            &workspace,
            &["pods", "--config", "=", "staging", "image", "update", ""],
            6,
        ),
        vec![
            String::from("@all"),
            String::from("@backend"),
            String::from("api"),
            String::from("db"),
        ]
    );
    assert_eq!(
        run_completion(
            &workspace,
            &["pods", "--config=staging.conf", "logs", "show", "api/"],
            4,
        ),
        vec![
            String::from("api/3"),
            String::from("api/custom-worker"),
            String::from("api/web"),
        ]
    );
    assert_eq!(
        run_completion(
            &workspace,
            &["pods", "--config", "staging", "cmd", "exec", "api", ""],
            6,
        ),
        vec![String::from("reload_cfg"), String::from("status_check")]
    );

    // Path traversal and command substitution inside configs must be safely rejected
    assert!(
        run_completion(&workspace, &["pods", "--config=../etc/passwd", "up", ""], 3).is_empty()
    );
    fs::write(workspace.dir().join("unsafe.conf"), UNSAFE_CONF).unwrap();
    let _ = run_completion(&workspace, &["pods", "--config=unsafe", "up", ""], 3);
    assert!(!workspace.dir().join("pwned_conf").exists());
}
