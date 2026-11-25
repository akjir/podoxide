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

//! Podman CLI inspection queries for versions, containers, pods, and images.

use std::collections::BTreeSet;
use std::io;

use crate::error::PodError;
use crate::utilities::string;

/// ASCII Unit Separator (`0x1F`) delimiting fields in Podman `--format` query records.
pub const FIELD_DELIMITER: char = '\x1f';
/// Format template passed to `podman images --format`.
const IMAGES_FORMAT: &str = r#"{{.ID}}{{"\x1f"}}{{.Id}}{{"\x1f"}}{{.Repository}}{{"\x1f"}}{{.Tag}}{{"\x1f"}}{{.Size}}{{"\x1f"}}{{.Created}}{{"\x1e"}}"#;
/// Maximum number of container or image records collected during in-use image queries.
const MAX_INSPECT_RECORDS: usize = 1024;
/// Maximum number of lines scanned in `podman --version` output.
const MAX_VERSION_LINES: usize = 32;
/// Maximum byte length accepted for `podman --version` output.
const MAX_VERSION_OUTPUT_BYTES: usize = 4096;
/// Minimum supported Podman major version.
const MIN_PODMAN_MAJOR: u32 = 5;
/// Minimum supported Podman minor version when major equals [`MIN_PODMAN_MAJOR`].
const MIN_PODMAN_MINOR: u32 = 8;
/// Format template passed to `podman ps -a --format`.
const PS_FORMAT: &str = r#"{{.ID}}{{"\x1f"}}{{.Image}}{{"\x1f"}}{{.Command}}{{"\x1f"}}{{.CreatedAt}}{{"\x1f"}}{{.Status}}{{"\x1f"}}{{.Ports}}{{"\x1f"}}{{.Names}}{{"\x1f"}}{{.PodName}}{{"\x1f"}}{{.Restarts}}{{"\x1e"}}"#;
/// ASCII Record Separator (`0x1E`) terminating records in Podman `--format` query output.
pub const RECORD_DELIMITER: char = '\x1e';

/// Executes `podman --version` and evaluates whether the installed Podman CLI is version 5.8.0 or higher.
#[must_use]
pub fn check_version() -> bool {
    let Some(output) = super::run_podman_capture(&["--version"]) else {
        return false;
    };
    is_supported_version(&output)
}

/// Executes `podman container inspect -f '{{.State.Status}}' <container_name>` and returns `true` when the container is running.
#[must_use]
pub fn container_running(container_name: &str) -> bool {
    container_running_with(container_name, &mut super::run_podman_capture)
}

/// Executes `podman pod exists <pod_name>` and returns `true` when the pod exists in local storage.
#[must_use]
pub fn pod_exists(pod_name: &str) -> bool {
    pod_exists_with(pod_name, &mut super::run_podman_capture)
}

/// Executes `podman container inspect -f '{{.Image}}' <container_name>` and returns the container's local image ID.
#[must_use]
pub fn query_container_image_id(container_name: &str) -> Option<String> {
    query_container_image_id_with(container_name, &mut super::run_podman_capture)
}

/// Executes `podman image inspect -f '{{.Id}}' <image_reference>` and returns the local image ID.
#[must_use]
pub fn query_image_id(image_reference: &str) -> Option<String> {
    query_image_id_with(image_reference, &mut super::run_podman_capture)
}

/// Executes `podman images [--filter dangling=true] --format ...` and returns raw control-delimited image records.
#[must_use]
pub fn query_images(all: bool) -> Option<String> {
    query_images_with(all, &mut super::run_podman_capture)
}

/// Executes `podman ps -a --no-trunc --format '{{.ImageID}}'` atomically to collect image IDs attached to existing containers.
///
/// # Errors
///
/// Returns [`PodError::Io`] if querying `podman ps` fails so callers never fail open with an empty set.
pub fn query_in_use_images() -> Result<BTreeSet<String>, PodError> {
    collect_in_use_images_with(&mut super::run_podman_capture)
}

/// Executes `podman ps -a --format ...` and returns raw control-delimited container status records.
#[must_use]
pub fn query_ps_status() -> Option<String> {
    query_ps_status_with(&mut super::run_podman_capture)
}

/// Queries `podman ps -a --no-trunc --format '{{.ImageID}}'` via `capture` to collect image IDs attached to existing containers.
fn collect_in_use_images_with<F>(capture: &mut F) -> Result<BTreeSet<String>, PodError>
where
    F: FnMut(&[&str]) -> Option<String>,
{
    let ps_output = capture(&["ps", "-a", "--no-trunc", "--format", "{{.ImageID}}"])
        .ok_or_else(|| PodError::from(io::Error::other("Failed to query in-use podman images.")))?;
    Ok(parse_in_use_output(&ps_output))
}

/// Evaluates whether `container_name` is running via `capture`, supporting unmanaged container names.
#[must_use]
fn container_running_with<F>(container_name: &str, capture: &mut F) -> bool
where
    F: FnMut(&[&str]) -> Option<String>,
{
    if !string::is_valid_unmanaged_container_name(container_name) {
        return false;
    }
    let Some(output) = capture(&[
        "container",
        "inspect",
        "-f",
        "{{.State.Status}}",
        container_name,
    ]) else {
        return false;
    };
    is_running_container_status(&output)
}

/// Extracts the raw semantic version token from a `"podman version <semver>"` output line.
fn extract_version_token(line: &str) -> Option<&str> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "podman" || tokens.next()? != "version" {
        return None;
    }
    let version_token = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }
    Some(version_token)
}

/// Evaluates whether `podman container inspect -f '{{.State.Status}}'` output indicates a running container.
#[must_use]
fn is_running_container_status(output: &str) -> bool {
    let mut lines = output.lines();
    lines.next() == Some("running") && lines.next().is_none()
}

/// Evaluates whether a `podman --version` output string satisfies version 5.8.0 or higher.
#[must_use]
fn is_supported_version(output: &str) -> bool {
    let Some((major, minor)) = parse_podman_major_minor(output) else {
        return false;
    };
    major > MIN_PODMAN_MAJOR || (major == MIN_PODMAN_MAJOR && minor >= MIN_PODMAN_MINOR)
}

/// Evaluates whether `patch_remainder` starts with valid patch digits followed by an optional SemVer suffix.
fn is_valid_patch_segment(patch_remainder: &str) -> bool {
    let Some((patch_digits, metadata)) = patch_remainder.split_once(['-', '+']) else {
        return string::parse_ascii_u32(patch_remainder).is_some();
    };
    string::parse_ascii_u32(patch_digits).is_some() && is_valid_version_metadata(metadata)
}

/// Evaluates whether `metadata` is a non-empty SemVer pre-release or build metadata string.
fn is_valid_version_metadata(metadata: &str) -> bool {
    !metadata.is_empty()
        && metadata.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' || byte == b'+'
        })
}

/// Extracts a single valid trimmed image ID token from `podman image inspect -f '{{.Id}}'` output.
#[must_use]
fn parse_image_id_output(output: &str) -> Option<String> {
    let mut lines = output.lines();
    let first = lines.next()?.trim();
    if lines.next().is_some() || !string::is_valid_token(first) {
        return None;
    }
    Some(String::from(first))
}

/// Collects valid trimmed image IDs from `podman ps -a --no-trunc --format '{{.ImageID}}'` output up to [`MAX_INSPECT_RECORDS`].
#[must_use]
fn parse_in_use_output(ps_output: &str) -> BTreeSet<String> {
    let mut in_use = BTreeSet::new();
    for (index, line) in ps_output.lines().enumerate() {
        if index >= MAX_INSPECT_RECORDS || in_use.len() >= MAX_INSPECT_RECORDS {
            break;
        }
        let trimmed = line.trim();
        if string::is_valid_token(trimmed) && !in_use.contains(trimmed) {
            in_use.insert(String::from(trimmed));
        }
    }
    in_use
}

/// Parses a single `"podman version <major>.<minor>.<patch>"` line into its `(major, minor)` version tuple.
fn parse_line_major_minor(line: &str) -> Option<(u32, u32)> {
    let version_token = extract_version_token(line)?;
    let (major_str, remainder) = version_token.split_once('.')?;
    let (minor_str, patch_remainder) = remainder.split_once('.')?;
    if !is_valid_patch_segment(patch_remainder) {
        return None;
    }
    let major = string::parse_ascii_u32(major_str)?;
    let minor = string::parse_ascii_u32(minor_str)?;
    Some((major, minor))
}

/// Scans bounded `podman --version` output lines and returns the first valid `(major, minor)` version tuple.
fn parse_podman_major_minor(output: &str) -> Option<(u32, u32)> {
    if output.len() > MAX_VERSION_OUTPUT_BYTES {
        return None;
    }
    for (index, line) in output.lines().enumerate() {
        if index >= MAX_VERSION_LINES {
            return None;
        }
        if let Some(version) = parse_line_major_minor(line) {
            return Some(version);
        }
    }
    None
}

/// Evaluates whether `pod_name` exists via `capture`.
#[must_use]
fn pod_exists_with<F>(pod_name: &str, capture: &mut F) -> bool
where
    F: FnMut(&[&str]) -> Option<String>,
{
    if !string::is_valid_pod_or_container_name(pod_name) {
        return false;
    }
    capture(&["pod", "exists", pod_name]).is_some()
}

/// Queries `podman container inspect -f '{{.Image}}' <container_name>` via `capture` and returns the validated image ID.
#[must_use]
fn query_container_image_id_with<F>(container_name: &str, capture: &mut F) -> Option<String>
where
    F: FnMut(&[&str]) -> Option<String>,
{
    if !string::is_valid_unmanaged_container_name(container_name) {
        return None;
    }
    let output = capture(&["container", "inspect", "-f", "{{.Image}}", container_name])?;
    parse_image_id_output(&output)
}

/// Queries `podman image inspect -f '{{.Id}}' <image_reference>` via `capture` and returns the validated image ID.
#[must_use]
fn query_image_id_with<F>(image_reference: &str, capture: &mut F) -> Option<String>
where
    F: FnMut(&[&str]) -> Option<String>,
{
    if !string::is_valid_token(image_reference) {
        return None;
    }
    let output = capture(&["image", "inspect", "-f", "{{.Id}}", image_reference])?;
    parse_image_id_output(&output)
}

/// Queries `podman images` (with `--filter dangling=true` when `all` is `false`) via `capture`.
#[must_use]
fn query_images_with<F>(all: bool, capture: &mut F) -> Option<String>
where
    F: FnMut(&[&str]) -> Option<String>,
{
    if all {
        capture(&["images", "--format", IMAGES_FORMAT])
    } else {
        capture(&[
            "images",
            "--filter",
            "dangling=true",
            "--format",
            IMAGES_FORMAT,
        ])
    }
}

/// Queries `podman ps -a --format` via `capture`.
#[must_use]
fn query_ps_status_with<F>(capture: &mut F) -> Option<String>
where
    F: FnMut(&[&str]) -> Option<String>,
{
    capture(&["ps", "-a", "--format", PS_FORMAT])
}

#[cfg(test)]
#[path = "query_tests.rs"]
mod tests;
