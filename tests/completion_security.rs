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

//! Security and shell-environment hardening integration tests for `.pods-completion.bash`.

#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use self::common::TestWorkspace;

const HOME_CONFIG: &str = "\
recipes {
    path .
    groups {
        homegrp homeapp
    }
}
";

fn completion_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".pods-completion.bash")
}

fn run_bash(workspace: &TestWorkspace, script: &str, env: &[(&str, &str)]) -> (String, String) {
    let script_path = completion_script_path();
    let script_str = script_path.to_str().expect("UTF-8 completion script path");
    let mut full_env = Vec::with_capacity(env.len() + 1);
    full_env.push(("COMPLETION_SCRIPT", script_str));
    full_env.extend_from_slice(env);
    let output = workspace.run_binary_with_env(
        Path::new("bash"),
        &["--noprofile", "--norc", "-c", script],
        b"",
        &full_env,
    );
    assert!(
        output.success,
        "bash failed: stdout={:?} stderr={:?}",
        output.stdout, output.stderr
    );
    (output.stdout, output.stderr)
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
fn completion_blocks_config_prefix_command_injection_and_handles_special_filenames() {
    let workspace = TestWorkspace::new("comp_sec_rce");
    fs::write(workspace.dir().join("normal.conf"), HOME_CONFIG).unwrap();
    fs::write(workspace.dir().join("line1\nline2.conf"), HOME_CONFIG).unwrap();
    fs::create_dir_all(workspace.dir().join("dir [1]")).unwrap();
    fs::write(workspace.dir().join("dir [1]/nested.conf"), HOME_CONFIG).unwrap();

    let marker_a = workspace.dir().join("rce_a");
    let marker_b = workspace.dir().join("rce_b");
    let marker_c = workspace.dir().join("rce_c");
    let marker_d = workspace.dir().join("rce_d");

    let payloads = [
        vec![
            String::from("pods"),
            String::from("--config"),
            format!("'${{HOME:+$(touch {})}}/'", marker_a.display()),
        ],
        vec![
            String::from("pods"),
            format!("--config=${{a[`touch {}`]}}/", marker_b.display()),
        ],
        vec![
            String::from("pods"),
            String::from("--config"),
            String::from("="),
            format!("$(touch {})/", marker_c.display()),
        ],
        vec![
            String::from("pods"),
            String::from("--config"),
            format!("`touch {}`/", marker_d.display()),
        ],
    ];

    for words in &payloads {
        let joined: Vec<String> = words.iter().map(|word| shell_quote(word)).collect();
        let cword = words.len() - 1;
        let script = format!(
            "set -eu\nsource \"$COMPLETION_SCRIPT\"\nCOMP_WORDS=({})\nCOMP_CWORD={cword}\n_pods_completions\nprintf '%s\\0' \"${{COMPREPLY[@]}}\"\n",
            joined.join(" ")
        );
        let (_stdout, stderr) = run_bash(&workspace, &script, &[]);
        assert!(
            stderr.is_empty(),
            "unexpected stderr for {words:?}: {stderr}"
        );
    }

    for marker in [&marker_a, &marker_b, &marker_c, &marker_d] {
        assert!(
            !marker.exists(),
            "command injection payload executed and created {}",
            marker.display()
        );
    }

    // Filenames with embedded newlines and bracket directories must stay intact.
    let nul_script = r#"
set -eu
source "$COMPLETION_SCRIPT"
COMP_WORDS=(pods --config "")
COMP_CWORD=2
_pods_completions
for item in "${COMPREPLY[@]}"; do printf '%s\0' "$item"; done
"#;
    let (stdout, stderr) = run_bash(&workspace, nul_script, &[]);
    assert!(stderr.is_empty(), "unexpected stderr: {stderr}");
    let entries: Vec<&str> = stdout
        .split('\0')
        .filter(|entry| !entry.is_empty())
        .collect();
    assert!(
        entries.contains(&"line1\nline2.conf"),
        "newline filename must remain a single COMPREPLY entry, got {entries:?}"
    );
    assert!(
        entries.contains(&"dir [1]"),
        "bracket directory must appear intact, got {entries:?}"
    );

    let nested_script = r#"
set -eu
source "$COMPLETION_SCRIPT"
COMP_WORDS=(pods --config "dir [1]/")
COMP_CWORD=2
_pods_completions
for item in "${COMPREPLY[@]}"; do printf '%s\n' "$item"; done
"#;
    let (nested_out, nested_err) = run_bash(&workspace, nested_script, &[]);
    assert!(nested_err.is_empty(), "unexpected stderr: {nested_err}");
    assert_eq!(nested_out, "dir [1]/nested.conf\n");
}

#[test]
fn completion_expands_tilde_config_paths_and_isolates_nocasematch_and_errexit() {
    let workspace = TestWorkspace::new("comp_sec_tilde_env");
    assert!(workspace.run_pods(&["init"], b"").success);

    let fake_home = workspace.dir().join("fake_home");
    let home_sub = fake_home.join("cfgs");
    fs::create_dir_all(&home_sub).unwrap();
    fs::write(home_sub.join("home.conf"), HOME_CONFIG).unwrap();
    fs::write(home_sub.join("ignored.txt"), "ignore\n").unwrap();
    let home_str = fake_home.to_str().expect("UTF-8 fake home");

    let tilde_script = r#"
set -eu
source "$COMPLETION_SCRIPT"
COMP_WORDS=(pods --config "~/cfgs/")
COMP_CWORD=2
_pods_completions
for item in "${COMPREPLY[@]}"; do printf 'A:%s\n' "$item"; done
COMP_WORDS=(pods --config "~/cfgs/home.conf" up "")
COMP_CWORD=4
_pods_completions
for item in "${COMPREPLY[@]}"; do printf 'B:%s\n' "$item"; done
COMP_WORDS=(pods --config "'~/cfgs/home.conf'" up "")
COMP_CWORD=4
_pods_completions
for item in "${COMPREPLY[@]}"; do printf 'C:%s\n' "$item"; done
"#;
    let (tilde_out, tilde_err) = run_bash(&workspace, tilde_script, &[("HOME", home_str)]);
    assert!(tilde_err.is_empty(), "unexpected stderr: {tilde_err}");
    assert!(
        tilde_out.contains("A:~/cfgs/home.conf\n"),
        "expected ~/ completion preserving tilde prefix, got {tilde_out:?}"
    );
    assert!(
        !tilde_out.contains("ignored.txt"),
        "non-.conf file must be excluded under ~/: {tilde_out:?}"
    );
    assert!(
        tilde_out.contains("B:@homegrp\n") && tilde_out.contains("B:homeapp\n"),
        "expected dynamic targets loaded from ~/cfgs/home.conf, got {tilde_out:?}"
    );
    assert!(
        !tilde_out.contains("C:"),
        "quoted '~/cfgs/home.conf' must not tilde-expand, got {tilde_out:?}"
    );

    // nocasematch must be isolated during completion and restored afterward; set -eu must not abort.
    let env_script = r#"
set -eu
shopt -s nocasematch
source "$COMPLETION_SCRIPT"
COMP_WORDS=(pods UP "")
COMP_CWORD=2
_pods_completions
printf 'UP_COUNT=%d\n' "${#COMPREPLY[@]}"
shopt -q nocasematch && printf 'RESTORED=on\n' || printf 'RESTORED=off\n'
COMP_WORDS=(pods --config nonexistent up "")
COMP_CWORD=4
_pods_completions
printf 'MISSING_CFG=%d\n' "${#COMPREPLY[@]}"
COMP_WORDS=(pods up recipe/)
COMP_CWORD=2
_pods_completions
for item in "${COMPREPLY[@]}"; do printf 'CNT:%s\n' "$item"; done
COMP_WORDS=(pods cmd exec missing_rec "")
COMP_CWORD=4
_pods_completions
printf 'MISSING_REC=%d\n' "${#COMPREPLY[@]}"
"#;
    let (env_out, env_err) = run_bash(&workspace, env_script, &[]);
    assert!(env_err.is_empty(), "unexpected stderr: {env_err}");
    assert!(
        env_out.contains("UP_COUNT=0\n"),
        "uppercase UP must not match under nocasematch, got {env_out:?}"
    );
    assert!(
        env_out.contains("RESTORED=on\n"),
        "nocasematch must be restored to its prior state, got {env_out:?}"
    );
    assert!(env_out.contains("MISSING_CFG=0\n"));
    assert!(env_out.contains("CNT:recipe/app\n") && env_out.contains("CNT:recipe/db\n"));
    assert!(env_out.contains("MISSING_REC=0\n"));
}
