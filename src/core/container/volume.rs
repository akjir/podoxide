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

//! Container volume mount validation and host path formatting helpers.

use std::borrow::Cow;
use std::path::Path;

use crate::error::{RecipeError, RecipeErrorKind};
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::path;
use super::VolumeMount;

/// Formats `volume` against `pod_path` into a Podman `--volume` specification string.
///
/// # Errors
///
/// Returns [`RecipeError`] with [`RecipeErrorKind::InvalidVolume`] when `volume` is invalid.
pub fn format_volume_mount(
    volume: &VolumeMount,
    pod_path: &Path,
    line: usize,
    label: &str,
) -> Result<String, RecipeError> {
    if !is_valid_volume_mount(volume) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidVolume,
            line,
            label,
        ));
    }
    let host_path = if volume.host_dir.is_empty() {
        Cow::Borrowed("")
    } else {
        resolve_host_volume_path(&volume.host_dir, pod_path, line, label)?
    };
    let capacity = host_path
        .len()
        .saturating_add(volume.container_dir.len())
        .saturating_add(volume.options.len())
        .saturating_add(2);
    let mut formatted = String::with_capacity(capacity);
    if !host_path.is_empty() {
        formatted.push_str(&host_path);
        formatted.push(':');
    }
    formatted.push_str(&volume.container_dir);
    if !volume.options.is_empty() {
        formatted.push(':');
        formatted.push_str(&volume.options);
    }
    Ok(formatted)
}

/// Evaluates whether `volume` has a valid absolute `container_dir` and safe `host_dir` and `options`.
#[must_use]
pub fn is_valid_volume_mount(volume: &VolumeMount) -> bool {
    if !volume.container_dir.starts_with('/')
        || !path::is_valid_directory_path(&volume.container_dir)
    {
        return false;
    }
    if volume.host_dir.is_empty() {
        return volume.options.is_empty();
    }
    path::is_valid_directory_path(&volume.host_dir)
        && (volume.options.is_empty() || path::is_valid_volume_options(&volume.options))
}

/// Formats all `volumes` against `pod_path` into Podman `--volume` specification strings.
pub(super) fn format_volume_mounts(
    volumes: &[VolumeMount],
    pod_path: &Path,
    line: usize,
    label: &str,
) -> Result<Vec<String>, RecipeError> {
    let capacity = collection::bounded_capacity(volumes.len(), MAX_COLLECTION_PREALLOC);
    let mut mounts = Vec::with_capacity(capacity);
    for volume in volumes {
        mounts.push(format_volume_mount(volume, pod_path, line, label)?);
    }
    Ok(mounts)
}

/// Evaluates whether `volumes` contains any mount with a relative host directory or volume name.
#[must_use]
pub(super) fn has_relative_host_volume(volumes: &[VolumeMount]) -> bool {
    volumes
        .iter()
        .any(|volume| !volume.host_dir.is_empty() && !volume.host_dir.starts_with('/'))
}

/// Resolves a non-empty `host_dir` against `pod_path` when `host_dir` is relative.
pub(super) fn resolve_host_volume_path<'a>(
    host_dir: &'a str,
    pod_path: &Path,
    line: usize,
    label: &str,
) -> Result<Cow<'a, str>, RecipeError> {
    if host_dir.starts_with('/') {
        return Ok(Cow::Borrowed(host_dir));
    }
    let Some(pod_dir) = pod_path
        .to_str()
        .filter(|dir| path::is_valid_directory_path(dir))
    else {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidVolume,
            line,
            label,
        ));
    };
    let mut rest = host_dir.trim_end_matches('/');
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped.trim_start_matches('/');
    }
    let clean_host = if rest == "." { "" } else { rest };
    Ok(Cow::Owned(join_pod_volume_path(pod_dir, clean_host)))
}

/// Joins a validated UTF-8 `pod_dir` and normalized relative `clean_host` into a volume host path.
#[must_use]
fn join_pod_volume_path(pod_dir: &str, clean_host: &str) -> String {
    let trimmed_pod = pod_dir.trim_end_matches('/');
    let clean_pod = if trimmed_pod.is_empty() {
        pod_dir
    } else {
        trimmed_pod
    };
    let needs_dot_prefix =
        !clean_pod.starts_with('/') && clean_pod != "." && !clean_pod.starts_with("./");
    let capacity = clean_pod
        .len()
        .saturating_add(clean_host.len())
        .saturating_add(3);
    let mut resolved = String::with_capacity(capacity);
    if needs_dot_prefix {
        resolved.push_str("./");
    }
    resolved.push_str(clean_pod);
    if !clean_host.is_empty() {
        resolved.push('/');
        resolved.push_str(clean_host);
    }
    resolved
}
