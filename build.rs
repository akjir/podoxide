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
use std::process::Command;

/// Generates the full application version string with Git build metadata and exports it to Cargo.
fn generate_version_metadata() {
    let package_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_owned());
    let commit_count = get_git_commit_count();
    let commit_hash = get_git_commit_hash();
    let suffix = if is_git_dirty() { ".dev" } else { "" };

    let full_version = format!("v{package_version}+{commit_count}.{commit_hash}{suffix}");
    println!("cargo:rustc-env=PODOXIDE_FULL_VERSION={full_version}");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    println!("cargo:rerun-if-changed=.git/refs");
}

/// Queries Git for the total commit count on `HEAD`, returning `"0"` on failure.
fn get_git_commit_count() -> String {
    let Ok(output) = Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .output()
    else {
        return "0".to_owned();
    };
    if !output.status.success() {
        return "0".to_owned();
    }
    let count = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if count.is_empty() || !count.bytes().all(|byte| byte.is_ascii_digit()) {
        return "0".to_owned();
    }
    count
}

/// Queries Git for the short commit hash of `HEAD`, returning `"unknown"` on failure.
fn get_git_commit_hash() -> String {
    let Ok(output) = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    else {
        return "unknown".to_owned();
    };
    if !output.status.success() {
        return "unknown".to_owned();
    }
    let hash = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if hash.is_empty() {
        return "unknown".to_owned();
    }
    hash
}

/// Returns `true` if the Git working tree has uncommitted modifications or untracked files.
fn is_git_dirty() -> bool {
    let Ok(output) = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    !String::from_utf8_lossy(&output.stdout).trim().is_empty()
}

/// Runs the Cargo build script to embed Git version metadata at compile time.
fn main() {
    generate_version_metadata();
}
