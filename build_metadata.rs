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

//! Pure Git and Cargo build-script metadata formatting and rerun-directive helpers.

#![forbid(unsafe_code)]

use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

/// Git metadata paths tracked for incremental rebuilds when present on disk.
const GIT_RERUN_TARGETS: [&str; 4] = ["HEAD", "index", "packed-refs", "refs"];
/// Repository source and embedded template paths tracked inside Git checkouts so unstaged edits refresh `.dev` metadata.
const WORKTREE_RERUN_TARGETS: [&str; 4] = ["Cargo.toml", "pods.conf", "recipe.pod", "src"];

/// Cleanliness state of the Git working tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WorktreeState {
    /// Working tree has no uncommitted modifications or untracked files.
    Clean,
    /// Working tree has uncommitted modifications or untracked files.
    Dirty,
}

/// Formats the compile-time version string from package and Git metadata.
#[must_use]
pub(crate) fn format_full_version(
    package_version: &str,
    commit_count: Option<&str>,
    commit_hash: Option<&str>,
    worktree_state: WorktreeState,
) -> String {
    if commit_count.is_none() && commit_hash.is_none() {
        return package_version.to_owned();
    }
    let count = commit_count.unwrap_or("0");
    let hash = commit_hash.unwrap_or("unknown");
    let suffix = match worktree_state {
        WorktreeState::Clean => "",
        WorktreeState::Dirty => ".dev",
    };
    format!("{package_version}+{count}.{hash}{suffix}")
}

/// Validates and trims raw `git rev-list --count HEAD` output into an ASCII digit string.
#[must_use]
pub(crate) fn parse_git_commit_count(raw_output: &str) -> Option<&str> {
    let count = raw_output.trim();
    if count.is_empty() || !count.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(count)
}

/// Validates and trims raw `git rev-parse --short HEAD` output into an ASCII hexadecimal hash.
#[must_use]
pub(crate) fn parse_git_commit_hash(raw_output: &str) -> Option<&str> {
    let hash = raw_output.trim();
    if hash.is_empty() || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some(hash)
}

/// Strips trailing line endings from `git rev-parse --git-path` output and rejects control newlines.
#[must_use]
pub(crate) fn parse_git_path(raw_output: &str) -> Option<&str> {
    let path = raw_output.trim_end_matches(['\r', '\n']);
    if path.is_empty() || path.contains(['\n', '\r']) {
        return None;
    }
    Some(path)
}

/// Emits Edition 2024 `cargo::` version and rerun directives for `working_dir` to `writer`.
///
/// # Errors
///
/// Returns [`io::Error`] if writing Cargo build directives to `writer` fails.
pub(crate) fn write_version_metadata<W: Write>(
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
    writeln!(writer, "cargo::rerun-if-changed=build_metadata.rs")?;

    let git_paths = collect_git_rerun_paths(working_dir);
    if git_paths.is_empty() {
        return Ok(());
    }
    for git_path in git_paths {
        writeln!(writer, "cargo::rerun-if-changed={git_path}")?;
    }
    for source_target in WORKTREE_RERUN_TARGETS {
        if working_dir.join(source_target).exists() {
            writeln!(writer, "cargo::rerun-if-changed={source_target}")?;
        }
    }
    Ok(())
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
