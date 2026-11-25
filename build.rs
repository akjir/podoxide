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

//! Cargo build script generating compile-time version and Git build metadata.

#![forbid(unsafe_code)]

use std::env;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

/// Fallback package version used when `CARGO_PKG_VERSION` is unset.
const DEFAULT_PACKAGE_VERSION: &str = "0.0.0";

/// Git metadata paths tracked for incremental rebuilds when present on disk.
const GIT_RERUN_TARGETS: [&str; 4] = ["HEAD", "index", "packed-refs", "refs"];

/// Cleanliness state of the Git working tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorktreeState {
    /// Working tree has no uncommitted modifications or untracked files.
    Clean,
    /// Working tree has uncommitted modifications or untracked files.
    Dirty,
}

/// Resolves existing Git metadata paths relative to `working_dir` for Cargo rerun tracking.
fn collect_git_rerun_paths(working_dir: &Path) -> Vec<String> {
    let mut paths = Vec::with_capacity(GIT_RERUN_TARGETS.len());
    for git_target in GIT_RERUN_TARGETS {
        let Some(resolved) = resolve_git_path(working_dir, git_target) else {
            continue;
        };
        if working_dir.join(&resolved).exists() {
            paths.push(resolved);
        }
    }
    paths
}

/// Returns whether the Git working tree at `working_dir` has uncommitted or untracked changes.
fn detect_worktree_state(working_dir: &Path) -> WorktreeState {
    let is_dirty = run_git_stdout(working_dir, &["status", "--porcelain"])
        .is_some_and(|output| !output.trim().is_empty());
    if is_dirty {
        WorktreeState::Dirty
    } else {
        WorktreeState::Clean
    }
}

/// Formats the compile-time version string from package and Git metadata.
fn format_full_version(
    package_version: &str,
    commit_count: Option<&str>,
    commit_hash: Option<&str>,
    worktree_state: WorktreeState,
) -> String {
    if commit_count.is_none() && commit_hash.is_none() {
        return format!("v{package_version}");
    }
    let count = commit_count.unwrap_or("0");
    let hash = commit_hash.unwrap_or("unknown");
    let suffix = match worktree_state {
        WorktreeState::Clean => "",
        WorktreeState::Dirty => ".dev",
    };
    format!("v{package_version}+{count}.{hash}{suffix}")
}

/// Runs the Cargo build script to embed Git version metadata at compile time.
fn main() -> io::Result<()> {
    let package_version =
        env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| DEFAULT_PACKAGE_VERSION.to_owned());
    let mut stdout = io::stdout().lock();
    write_version_metadata(Path::new("."), &package_version, &mut stdout)
}

/// Validates and trims raw `git rev-list --count HEAD` output into an ASCII digit string.
fn parse_git_commit_count(raw_output: &str) -> Option<&str> {
    let count = raw_output.trim();
    if count.is_empty() || !count.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(count)
}

/// Validates and trims raw `git rev-parse --short HEAD` output into an ASCII hexadecimal hash.
fn parse_git_commit_hash(raw_output: &str) -> Option<&str> {
    let hash = raw_output.trim();
    if hash.is_empty() || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(hash)
}

/// Strips trailing line endings from `git rev-parse --git-path` output and rejects control newlines.
fn parse_git_path(raw_output: &str) -> Option<&str> {
    let path = raw_output.trim_end_matches(['\r', '\n']);
    if path.is_empty() || path.contains(['\n', '\r']) {
        return None;
    }
    Some(path)
}

/// Queries Git for the total commit count on `HEAD` in `working_dir`.
fn query_git_commit_count(working_dir: &Path) -> Option<String> {
    let output = run_git_stdout(working_dir, &["rev-list", "--count", "HEAD"])?;
    parse_git_commit_count(&output).map(str::to_owned)
}

/// Queries Git for the short commit hash of `HEAD` in `working_dir`.
fn query_git_commit_hash(working_dir: &Path) -> Option<String> {
    let output = run_git_stdout(working_dir, &["rev-parse", "--short", "HEAD"])?;
    parse_git_commit_hash(&output).map(str::to_owned)
}

/// Resolves a Git metadata path such as `HEAD` or `packed-refs` via `git rev-parse --git-path`.
fn resolve_git_path(working_dir: &Path, git_target: &str) -> Option<String> {
    let output = run_git_stdout(working_dir, &["rev-parse", "--git-path", git_target])?;
    parse_git_path(&output).map(str::to_owned)
}

/// Executes `git` with `arguments` in `working_dir` and returns UTF-8 stdout on zero exit status.
fn run_git_stdout(working_dir: &Path, arguments: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .current_dir(working_dir)
        .args(arguments)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Emits Edition 2024 `cargo::` version and rerun directives for `working_dir` to `writer`.
fn write_version_metadata<W: Write>(
    working_dir: &Path,
    package_version: &str,
    writer: &mut W,
) -> io::Result<()> {
    let commit_count = query_git_commit_count(working_dir);
    let commit_hash = query_git_commit_hash(working_dir);
    let worktree_state = if commit_count.is_some() || commit_hash.is_some() {
        detect_worktree_state(working_dir)
    } else {
        WorktreeState::Clean
    };

    let full_version = format_full_version(
        package_version,
        commit_count.as_deref(),
        commit_hash.as_deref(),
        worktree_state,
    );
    writeln!(
        writer,
        "cargo::rustc-env=PODOXIDE_FULL_VERSION={full_version}"
    )?;
    writeln!(writer, "cargo::rerun-if-changed=build.rs")?;

    for git_path in collect_git_rerun_paths(working_dir) {
        writeln!(writer, "cargo::rerun-if-changed={git_path}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{self, Command};

    use super::{
        WorktreeState, format_full_version, parse_git_commit_count, parse_git_commit_hash,
        parse_git_path, write_version_metadata,
    };

    struct TempDirGuard {
        path: PathBuf,
    }

    impl TempDirGuard {
        fn new(label: &str) -> Self {
            let path =
                env::temp_dir().join(format!("podoxide_build_test_{}_{label}", process::id()));
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
            format_full_version("0.1.0", Some("48"), Some("d5f2403"), WorktreeState::Clean),
            "v0.1.0+48.d5f2403"
        );
        assert_eq!(
            format_full_version("0.1.0", Some("48"), Some("d5f2403"), WorktreeState::Dirty),
            "v0.1.0+48.d5f2403.dev"
        );
        assert_eq!(
            format_full_version("0.1.0", None, None, WorktreeState::Clean),
            "v0.1.0"
        );
        assert_eq!(
            format_full_version("0.1.0", None, None, WorktreeState::Dirty),
            "v0.1.0"
        );
        assert_eq!(
            format_full_version("0.1.0", Some("5"), None, WorktreeState::Clean),
            "v0.1.0+5.unknown"
        );
        assert_eq!(
            format_full_version("0.1.0", None, Some("abc1234"), WorktreeState::Dirty),
            "v0.1.0+0.abc1234.dev"
        );
    }

    #[test]
    fn parsers_validate_git_outputs_and_reject_malformed_values() {
        assert_eq!(parse_git_commit_count("48\n"), Some("48"));
        assert_eq!(parse_git_commit_count("  0 \r\n"), Some("0"));
        assert_eq!(parse_git_commit_count(""), None);
        assert_eq!(parse_git_commit_count("-1"), None);
        assert_eq!(parse_git_commit_count("12a"), None);

        assert_eq!(parse_git_commit_hash("d5f2403\n"), Some("d5f2403"));
        assert_eq!(parse_git_commit_hash("ABCDEF0\r\n"), Some("ABCDEF0"));
        assert_eq!(parse_git_commit_hash(""), None);
        assert_eq!(parse_git_commit_hash("unknown"), None);
        assert_eq!(parse_git_commit_hash("abc\ndef"), None);

        assert_eq!(parse_git_path(".git/HEAD\n"), Some(".git/HEAD"));
        assert_eq!(
            parse_git_path("dir with space/.git/HEAD\r\n"),
            Some("dir with space/.git/HEAD")
        );
        assert_eq!(parse_git_path("\n"), None);
        assert_eq!(parse_git_path(".git/HEAD\ncargo::warning=bad\n"), None);
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
        let mut output = Vec::new();
        write_version_metadata(&temp_dir.path, "0.1.0", &mut output).unwrap();
        let directives = String::from_utf8(output).unwrap();

        assert_eq!(
            directives,
            "cargo::rustc-env=PODOXIDE_FULL_VERSION=v0.1.0\ncargo::rerun-if-changed=build.rs\n"
        );
    }

    #[test]
    fn write_version_metadata_handles_unborn_repo_packed_refs_dirty_and_worktree() {
        let temp_dir = TempDirGuard::new("git_scenarios");
        let main_repo = temp_dir.path.join("main");
        let worktree = temp_dir.path.join("worktree");
        fs::create_dir_all(&main_repo).unwrap();

        run_git_ok(&main_repo, &["init", "-q"]);
        run_git_ok(&main_repo, &["config", "user.email", "test@example.com"]);
        run_git_ok(&main_repo, &["config", "user.name", "Test User"]);

        // Unborn repo (no commits yet) must emit clean version and only existing paths.
        let mut unborn_output = Vec::new();
        write_version_metadata(&main_repo, "0.1.0", &mut unborn_output).unwrap();
        let unborn_text = String::from_utf8(unborn_output).unwrap();
        assert!(unborn_text.contains("cargo::rustc-env=PODOXIDE_FULL_VERSION=v0.1.0\n"));
        assert!(!unborn_text.contains("packed-refs"));
        assert!(!unborn_text.contains("cargo:rustc-env"));
        assert!(!unborn_text.contains("cargo:rerun-if-changed"));

        // Commit a file and verify clean repo metadata before packed-refs exists.
        fs::write(main_repo.join("sample.txt"), "hello\n").unwrap();
        run_git_ok(&main_repo, &["add", "sample.txt"]);
        run_git_ok(&main_repo, &["commit", "-q", "-m", "initial commit"]);

        let mut clean_output = Vec::new();
        write_version_metadata(&main_repo, "0.1.0", &mut clean_output).unwrap();
        let clean_text = String::from_utf8(clean_output).unwrap();
        assert!(clean_text.contains("cargo::rustc-env=PODOXIDE_FULL_VERSION=v0.1.0+1."));
        assert!(!clean_text.contains(".dev\n"));
        assert!(clean_text.contains("cargo::rerun-if-changed=.git/HEAD\n"));
        assert!(clean_text.contains("cargo::rerun-if-changed=.git/index\n"));
        assert!(clean_text.contains("cargo::rerun-if-changed=.git/refs\n"));
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
        write_version_metadata(&worktree, "0.1.0", &mut worktree_output).unwrap();
        let worktree_text = String::from_utf8(worktree_output).unwrap();
        assert!(worktree_text.contains("cargo::rustc-env=PODOXIDE_FULL_VERSION=v0.1.0+1."));
        assert!(worktree_text.contains("worktrees/worktree/HEAD\n"));
        assert!(worktree_text.contains("worktrees/worktree/index\n"));
        assert!(worktree_text.contains("packed-refs\n"));
        for line in worktree_text.lines() {
            if let Some(path) = line.strip_prefix("cargo::rerun-if-changed=")
                && path != "build.rs"
            {
                assert!(
                    worktree.join(path).exists(),
                    "emitted rerun path must exist on disk: {path}"
                );
            }
        }

        // Dirty the worktree and verify .dev suffix.
        fs::write(worktree.join("untracked.txt"), "dirty\n").unwrap();
        let mut dirty_output = Vec::new();
        write_version_metadata(&worktree, "0.1.0", &mut dirty_output).unwrap();
        let dirty_text = String::from_utf8(dirty_output).unwrap();
        assert!(
            dirty_text.lines().next().unwrap().ends_with(".dev"),
            "expected .dev suffix in dirty worktree: {dirty_text:?}"
        );
    }
}
