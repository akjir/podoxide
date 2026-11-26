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

//! Integration tests for compile-time version and Git metadata generation in `build_metadata.rs`.

#![forbid(unsafe_code)]

#[path = "../build_metadata.rs"]
mod build_metadata;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{self, Command};

use self::build_metadata::WorktreeState;

struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    fn new(label: &str) -> Self {
        let path = env::temp_dir().join(format!("podoxide_build_test_{}_{label}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn format_full_version_handles_git_partial_and_non_git_states() {
    assert_eq!(
        build_metadata::format_full_version(
            "0.1.0",
            Some("48"),
            Some("d5f2403"),
            WorktreeState::Clean
        ),
        "v0.1.0+48.d5f2403"
    );
    assert_eq!(
        build_metadata::format_full_version(
            "0.1.0",
            Some("48"),
            Some("d5f2403"),
            WorktreeState::Dirty
        ),
        "v0.1.0+48.d5f2403.dev"
    );
    assert_eq!(
        build_metadata::format_full_version("0.1.0", None, None, WorktreeState::Clean),
        "v0.1.0"
    );
    assert_eq!(
        build_metadata::format_full_version("0.1.0", None, None, WorktreeState::Dirty),
        "v0.1.0"
    );
    assert_eq!(
        build_metadata::format_full_version("0.1.0", Some("5"), None, WorktreeState::Clean),
        "v0.1.0+5.unknown"
    );
    assert_eq!(
        build_metadata::format_full_version("0.1.0", None, Some("abc1234"), WorktreeState::Dirty),
        "v0.1.0+0.abc1234.dev"
    );
}

#[test]
fn parsers_validate_git_outputs_and_reject_malformed_values() {
    assert_eq!(build_metadata::parse_git_commit_count("48\n"), Some("48"));
    assert_eq!(
        build_metadata::parse_git_commit_count("  0 \r\n"),
        Some("0")
    );
    assert_eq!(build_metadata::parse_git_commit_count(""), None);
    assert_eq!(build_metadata::parse_git_commit_count("-1"), None);
    assert_eq!(build_metadata::parse_git_commit_count("12a"), None);

    assert_eq!(
        build_metadata::parse_git_commit_hash("d5f2403\n"),
        Some("d5f2403")
    );
    assert_eq!(
        build_metadata::parse_git_commit_hash("ABCDEF0\r\n"),
        Some("ABCDEF0")
    );
    assert_eq!(build_metadata::parse_git_commit_hash(""), None);
    assert_eq!(build_metadata::parse_git_commit_hash("unknown"), None);
    assert_eq!(build_metadata::parse_git_commit_hash("abc\ndef"), None);

    assert_eq!(
        build_metadata::parse_git_path(".git/HEAD\n"),
        Some(".git/HEAD")
    );
    assert_eq!(
        build_metadata::parse_git_path("dir with space/.git/HEAD\r\n"),
        Some("dir with space/.git/HEAD")
    );
    assert_eq!(build_metadata::parse_git_path("\n"), None);
    assert_eq!(
        build_metadata::parse_git_path(".git/HEAD\ncargo::warning=bad\n"),
        None
    );
}

fn run_git_ok(working_dir: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .current_dir(working_dir)
        .args(arguments)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "git {arguments:?} failed in {}",
        working_dir.display()
    );
}

#[test]
fn write_version_metadata_emits_clean_directives_outside_git() {
    let temp_dir = TempDirGuard::new("non_git");
    fs::create_dir_all(temp_dir.path.join("src")).unwrap();
    fs::write(temp_dir.path.join("Cargo.toml"), "[package]\n").unwrap();

    let mut output = Vec::new();
    build_metadata::write_version_metadata(&temp_dir.path, "0.1.0", &mut output).unwrap();
    let directives = String::from_utf8(output).unwrap();

    assert_eq!(
        directives,
        "cargo::rustc-env=PODOXIDE_FULL_VERSION=v0.1.0\ncargo::rerun-if-changed=build.rs\ncargo::rerun-if-changed=build_metadata.rs\n"
    );
}

#[test]
fn write_version_metadata_handles_unborn_repo_packed_refs_dirty_and_worktree() {
    let temp_dir = TempDirGuard::new("git_scenarios");
    let main_repo = temp_dir.path.join("main");
    let worktree = temp_dir.path.join("worktree");
    fs::create_dir_all(main_repo.join("src")).unwrap();

    run_git_ok(&main_repo, &["init", "-q"]);
    run_git_ok(&main_repo, &["config", "user.email", "test@example.com"]);
    run_git_ok(&main_repo, &["config", "user.name", "Test User"]);

    // Unborn repo (no commits yet) must emit clean version and only existing paths.
    let mut unborn_output = Vec::new();
    build_metadata::write_version_metadata(&main_repo, "0.1.0", &mut unborn_output).unwrap();
    let unborn_text = String::from_utf8(unborn_output).unwrap();
    assert!(unborn_text.contains("cargo::rustc-env=PODOXIDE_FULL_VERSION=v0.1.0\n"));
    assert!(unborn_text.contains("cargo::rerun-if-changed=build_metadata.rs\n"));
    assert!(!unborn_text.contains("packed-refs"));
    assert!(!unborn_text.contains("cargo:rustc-env"));
    assert!(!unborn_text.contains("cargo:rerun-if-changed"));

    // Commit source files and verify clean repo metadata before packed-refs exists.
    fs::write(main_repo.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
    fs::write(main_repo.join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
    run_git_ok(&main_repo, &["add", "Cargo.toml", "src/lib.rs"]);
    run_git_ok(&main_repo, &["commit", "-q", "-m", "initial commit"]);

    let mut clean_output = Vec::new();
    build_metadata::write_version_metadata(&main_repo, "0.1.0", &mut clean_output).unwrap();
    let clean_text = String::from_utf8(clean_output).unwrap();
    assert!(clean_text.contains("cargo::rustc-env=PODOXIDE_FULL_VERSION=v0.1.0+1."));
    assert!(!clean_text.contains(".dev\n"));
    assert!(clean_text.contains("cargo::rerun-if-changed=build.rs\n"));
    assert!(clean_text.contains("cargo::rerun-if-changed=build_metadata.rs\n"));
    assert!(clean_text.contains("cargo::rerun-if-changed=.git/HEAD\n"));
    assert!(clean_text.contains("cargo::rerun-if-changed=.git/index\n"));
    assert!(clean_text.contains("cargo::rerun-if-changed=.git/refs\n"));
    assert!(clean_text.contains("cargo::rerun-if-changed=Cargo.toml\n"));
    assert!(clean_text.contains("cargo::rerun-if-changed=src\n"));
    assert!(!clean_text.contains("packed-refs"));

    // Pack refs and add a worktree; verify worktree path resolution and packed-refs tracking.
    run_git_ok(&main_repo, &["pack-refs", "--all"]);
    run_git_ok(
        &main_repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "wt-branch",
            worktree.to_str().unwrap(),
        ],
    );

    let mut worktree_output = Vec::new();
    build_metadata::write_version_metadata(&worktree, "0.1.0", &mut worktree_output).unwrap();
    let worktree_text = String::from_utf8(worktree_output).unwrap();
    assert!(worktree_text.contains("cargo::rustc-env=PODOXIDE_FULL_VERSION=v0.1.0+1."));
    assert!(worktree_text.contains("worktrees/worktree/HEAD\n"));
    assert!(worktree_text.contains("worktrees/worktree/index\n"));
    assert!(worktree_text.contains("packed-refs\n"));
    assert!(worktree_text.contains("cargo::rerun-if-changed=Cargo.toml\n"));
    assert!(worktree_text.contains("cargo::rerun-if-changed=src\n"));
    for line in worktree_text.lines() {
        if let Some(path) = line.strip_prefix("cargo::rerun-if-changed=")
            && path != "build.rs"
            && path != "build_metadata.rs"
        {
            assert!(
                worktree.join(path).exists(),
                "emitted rerun path must exist on disk: {path}"
            );
        }
    }

    // Make an unstaged source edit in the worktree and verify .dev suffix.
    fs::write(
        worktree.join("src/lib.rs"),
        "pub fn demo() { let _ = 1; }\n",
    )
    .unwrap();
    let mut dirty_output = Vec::new();
    build_metadata::write_version_metadata(&worktree, "0.1.0", &mut dirty_output).unwrap();
    let dirty_text = String::from_utf8(dirty_output).unwrap();
    assert!(
        dirty_text.lines().next().unwrap().ends_with(".dev"),
        "expected .dev suffix after unstaged edit in worktree: {dirty_text:?}"
    );
}
