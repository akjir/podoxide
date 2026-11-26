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

//! File path lexical normalization, extension matching, and path validation helpers.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use crate::error::MAX_ERROR_TOKEN_BYTES;
use crate::utilities::string;

/// Constructs a full file or directory path from `base_dir`, `file_name`, and `file_extension`.
#[must_use]
pub fn build_full_path(base_dir: &Path, file_name: &str, file_extension: &str) -> PathBuf {
    let base_bytes = base_dir.as_os_str().as_encoded_bytes();
    let relative_name = file_name.trim_start_matches('/');
    let leaf = format_leaf_name(relative_name, file_extension);
    let capacity = base_bytes
        .len()
        .saturating_add(leaf.len())
        .saturating_add(4);
    let mut full = PathBuf::with_capacity(capacity);
    if !base_bytes.starts_with(b"/") && !has_dot_segment_prefix(base_bytes) {
        full.push(".");
    }
    if !base_bytes.is_empty() {
        full.push(base_dir);
    }
    if !leaf.is_empty() {
        full.push(leaf.as_ref());
    }
    full
}

/// Evaluates whether `path` contains any parent directory (`..`) traversal component.
#[must_use]
pub fn has_parent_dir(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

/// Evaluates whether `path` names a terminal file entry without a trailing slash or dot segment.
#[must_use]
pub fn has_regular_target_name(path: &Path) -> bool {
    let raw_bytes = path.as_os_str().as_encoded_bytes();
    path.file_name().is_some() && !raw_bytes.ends_with(b"/") && !raw_bytes.ends_with(b"/.")
}

/// Evaluates whether `value` is a structurally valid editor path whose executable name is disallowed (`podman`).
#[must_use]
pub fn is_disallowed_editor_value(value: &str) -> bool {
    valid_editor_file_name(value).is_some_and(|file_name| file_name.eq_ignore_ascii_case("podman"))
}

/// Evaluates whether `path` is a valid UTF-8 directory path without `..` traversal, colons, or control characters.
#[must_use]
pub fn is_valid_directory(path: &Path) -> bool {
    path.to_str().is_some_and(is_valid_directory_path)
}

/// Evaluates whether `value` is a safe directory path without `..` traversal, colons, or control characters.
#[must_use]
pub fn is_valid_directory_path(value: &str) -> bool {
    let trimmed_slashes = value.trim_end_matches('/');
    if value.is_empty()
        || value != value.trim()
        || trimmed_slashes != trimmed_slashes.trim_end()
        || value.starts_with('-')
        || value.contains(':')
        || string::contains_control_or_bidi(value)
    {
        return false;
    }
    let path = Path::new(value);
    if has_parent_dir(path) {
        return false;
    }
    path.components()
        .any(|component| matches!(component, Component::CurDir | Component::Normal(_)))
}

/// Evaluates whether `value` is a safe bare executable name or absolute path without `..` traversal, leading dashes, or control/whitespace characters.
#[must_use]
pub fn is_valid_editor_value(value: &str) -> bool {
    valid_editor_file_name(value).is_some_and(|file_name| !file_name.eq_ignore_ascii_case("podman"))
}

/// Evaluates whether `path` is a structurally valid, UTF-8 target file path without `..` traversal or leading `-`/`+`.
#[must_use]
pub fn is_valid_target_path(path: &Path) -> bool {
    let Some(raw_path) = path.to_str() else {
        return false;
    };
    if has_parent_dir(path) {
        return false;
    }
    let Some(file_name) = validate_raw_target_file_name(raw_path) else {
        return false;
    };
    let stem = file_name
        .strip_suffix(".conf")
        .or_else(|| file_name.strip_suffix(".pod"))
        .unwrap_or(file_name);
    has_valid_stem(stem)
}

/// Evaluates whether comma-separated volume mount `options` are non-empty, free of empty segments, colons, slashes, `..`, leading hyphens, and control/whitespace characters.
#[must_use]
pub fn is_valid_volume_options(options: &str) -> bool {
    if options.is_empty()
        || options.contains(':')
        || options.contains('/')
        || options.contains("..")
        || string::contains_control_or_whitespace(options)
    {
        return false;
    }
    options
        .split(',')
        .all(|segment| !segment.is_empty() && !segment.starts_with('-'))
}

/// Evaluates whether `file_name` ends with `extension` and retains a non-empty, non-dot-only stem.
#[must_use]
pub fn matches_extension(file_name: &str, extension: Option<&str>) -> bool {
    let stem = if let Some(required_extension) = extension {
        let Some(stripped) = file_name.strip_suffix(required_extension) else {
            return false;
        };
        stripped
    } else {
        file_name
    };
    has_valid_stem(stem)
}

/// Normalizes `path` lexically by resolving `.` and `..` components without accessing the filesystem.
#[must_use]
pub fn normalize_lexical_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::with_capacity(path.as_os_str().len());
    for component in path.components() {
        apply_lexical_component(&mut normalized, component);
    }
    if normalized.as_os_str().is_empty() && !path.as_os_str().is_empty() {
        normalized.push(".");
    }
    normalized
}

/// Returns the non-empty parent directory of `path`, or `"."` when `path` has no non-empty parent.
#[must_use]
pub fn parent_or_cur_dir(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Pops the last normal component from `path` or appends `..` when `path` is relative and cannot be popped.
pub fn pop_relative_or_absolute(path: &mut PathBuf) {
    match path.components().next_back() {
        Some(Component::Prefix(_) | Component::RootDir) => {}
        Some(Component::CurDir) => {
            let _ = path.pop();
            path.push(Component::ParentDir.as_os_str());
        }
        Some(Component::ParentDir) => {
            path.push(Component::ParentDir.as_os_str());
        }
        Some(Component::Normal(_)) => {
            let _ = path.pop();
        }
        None => {
            path.push(Component::ParentDir.as_os_str());
        }
    }
}

/// Formats `path` relative to `base_dir` without a leading `./` prefix for display and log output.
#[must_use]
pub fn relative_display_path<'a>(path: &'a Path, base_dir: &Path) -> &'a Path {
    let clean_path = strip_leading_cur_dir(path);
    let clean_base = strip_leading_cur_dir(base_dir);
    let relative = if clean_base.as_os_str().is_empty() {
        clean_path
    } else {
        clean_path.strip_prefix(clean_base).unwrap_or(clean_path)
    };
    let stripped = strip_leading_cur_dir(relative);
    if stripped.as_os_str().is_empty() && !path.as_os_str().is_empty() {
        return Path::new(".");
    }
    stripped
}

/// Clamps and sanitizes a filesystem `path` into a bounded [`MAX_ERROR_TOKEN_BYTES`] error token.
#[must_use]
pub fn sanitize_path_token(path: &Path) -> String {
    if let Some(valid_str) = path.to_str() {
        return string::sanitize_text(valid_str, MAX_ERROR_TOKEN_BYTES);
    }
    let raw_bytes = path.as_os_str().as_encoded_bytes();
    let max_scan = MAX_ERROR_TOKEN_BYTES.saturating_add(3);
    let prefix_bytes = raw_bytes.get(..max_scan).unwrap_or(raw_bytes);
    let lossy = String::from_utf8_lossy(prefix_bytes);
    string::sanitize_text(&lossy, MAX_ERROR_TOKEN_BYTES)
}

/// Evaluates whether `stem` is non-empty after trimming and contains at least one non-dot byte.
#[must_use]
pub(crate) fn has_valid_stem(stem: &str) -> bool {
    !stem.trim().is_empty() && !stem.bytes().all(|byte| byte == b'.')
}

/// Validates structural invariants of a raw target path string and returns its terminal file name.
#[must_use]
pub(crate) fn validate_raw_target_file_name(raw_value: &str) -> Option<&str> {
    if raw_value.is_empty()
        || raw_value != raw_value.trim()
        || raw_value.starts_with(['-', '+'])
        || raw_value.ends_with('/')
        || string::contains_control_or_bidi(raw_value)
    {
        return None;
    }
    let file_name = raw_value.rsplit('/').next().unwrap_or(raw_value);
    (!file_name.is_empty() && file_name == file_name.trim() && !file_name.starts_with(['-', '+']))
        .then_some(file_name)
}

/// Applies a single `component` to `normalized` during lexical path normalization.
fn apply_lexical_component(normalized: &mut PathBuf, component: Component<'_>) {
    match component {
        Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
        Component::RootDir => normalized.push(component.as_os_str()),
        Component::CurDir => {}
        Component::ParentDir => pop_relative_or_absolute(normalized),
        Component::Normal(part) => normalized.push(part),
    }
}

/// Appends `file_extension` to `file_name` when `file_extension` is non-empty and not already present.
fn format_leaf_name<'a>(file_name: &'a str, file_extension: &str) -> Cow<'a, str> {
    if file_name.is_empty() || file_extension.is_empty() || file_name.ends_with(file_extension) {
        return Cow::Borrowed(file_name);
    }
    let mut combined = String::with_capacity(file_name.len().saturating_add(file_extension.len()));
    combined.push_str(file_name);
    combined.push_str(file_extension);
    Cow::Owned(combined)
}

/// Evaluates whether `base_bytes` starts with an explicit `.` or `..` relative path segment.
fn has_dot_segment_prefix(base_bytes: &[u8]) -> bool {
    base_bytes == b"."
        || base_bytes == b".."
        || base_bytes.starts_with(b"./")
        || base_bytes.starts_with(b"../")
}

/// Strips any leading `.` (`./`) current-directory prefix components from `path`.
#[must_use]
fn strip_leading_cur_dir(mut path: &Path) -> &Path {
    while let Ok(stripped) = path.strip_prefix(".") {
        path = stripped;
    }
    path
}

/// Extracts the validated terminal executable name from a bare or absolute editor path.
#[must_use]
fn valid_editor_file_name(value: &str) -> Option<&str> {
    if value.is_empty()
        || value.starts_with('-')
        || (value.contains('/') && !value.starts_with('/'))
        || string::contains_control_or_whitespace(value)
        || has_parent_dir(Path::new(value))
    {
        return None;
    }
    let file_name = value.rsplit('/').next()?;
    if file_name.is_empty() || file_name == "." || file_name == ".." || file_name.starts_with('-') {
        return None;
    }
    Some(file_name)
}

#[cfg(test)]
#[path = "path_tests.rs"]
mod tests;
