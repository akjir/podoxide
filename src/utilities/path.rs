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

//! File path lexical normalization, extension matching, and configuration stem extraction helpers.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use crate::core::config::{DEFAULT_CONFIG_NAME, DEFAULT_CONFIG_PATH};
use crate::error::{ConfigError, ConfigErrorKind, MAX_ERROR_TOKEN_BYTES, PodError};
use crate::utilities::string::{
    contains_control_or_bidi, contains_control_or_whitespace, sanitize_text,
};

/// Constructs a full file or directory path from `base_dir`, `file_name`, and `file_extension`.
#[must_use]
pub fn build_full_path(base_dir: &Path, file_name: &str, file_extension: &str) -> PathBuf {
    let base_bytes = base_dir.as_os_str().as_encoded_bytes();
    let leaf = format_leaf_name(file_name, file_extension);
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

/// Extracts the configuration file stem from a raw `.conf` path string if it is structurally valid.
#[must_use]
pub fn extract_config_stem(raw_value: &str) -> Option<&str> {
    if raw_value.is_empty()
        || raw_value != raw_value.trim()
        || raw_value.starts_with('-')
        || raw_value.ends_with('/')
        || contains_control_or_bidi(raw_value)
    {
        return None;
    }
    let file_name = raw_value.rsplit('/').next().unwrap_or(raw_value);
    if file_name.starts_with('-') {
        return None;
    }
    let stem = file_name.strip_suffix(".conf").unwrap_or(file_name);
    if !has_valid_stem(stem) {
        return None;
    }
    Some(stem)
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

/// Evaluates whether `path` is a valid UTF-8 directory path without `..` traversal, colons, or control characters.
#[must_use]
pub fn is_valid_directory(path: &Path) -> bool {
    path.to_str().is_some_and(is_valid_directory_path)
}

/// Evaluates whether `value` is a safe directory path without `..` traversal, colons, or control characters.
#[must_use]
pub fn is_valid_directory_path(value: &str) -> bool {
    if value.is_empty()
        || value != value.trim()
        || value.starts_with('-')
        || value.contains(':')
        || contains_control_or_bidi(value)
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

/// Evaluates whether `value` is a safe executable name or path without `..` traversal, leading dashes, or control/whitespace characters.
#[must_use]
pub fn is_valid_editor_value(value: &str) -> bool {
    if value.is_empty()
        || value.starts_with('-')
        || contains_control_or_whitespace(value)
        || has_parent_dir(Path::new(value))
    {
        return false;
    }
    let Some(file_name) = value.rsplit('/').next() else {
        return false;
    };
    !file_name.is_empty()
        && file_name != "."
        && file_name != ".."
        && !file_name.starts_with('-')
}

/// Evaluates whether `path` is a structurally valid, UTF-8 target file path without `..` traversal.
#[must_use]
pub fn is_valid_target_path(path: &Path) -> bool {
    let Some(raw_path) = path.to_str() else {
        return false;
    };
    if has_parent_dir(path) {
        return false;
    }
    let Some(config_stem) = extract_config_stem(raw_path) else {
        return false;
    };
    let target_stem = config_stem.strip_suffix(".pod").unwrap_or(config_stem);
    has_valid_stem(target_stem)
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
    let relative = if base_dir != Path::new(".") {
        path.strip_prefix(base_dir).unwrap_or(path)
    } else {
        path
    };
    let stripped = relative.strip_prefix("./").unwrap_or(relative);
    if stripped.as_os_str().is_empty() && !path.as_os_str().is_empty() {
        return Path::new(".");
    }
    stripped
}

/// Resolves a configuration file argument into its configuration name (file stem).
#[must_use]
pub fn resolve_config_name(config: Option<&Path>) -> String {
    config
        .and_then(Path::to_str)
        .and_then(extract_config_stem)
        .unwrap_or(DEFAULT_CONFIG_NAME)
        .to_owned()
}

/// Resolves a configuration file argument into a normalized `.conf` path.
#[must_use]
pub fn resolve_config_path(config: Option<&Path>) -> PathBuf {
    let Some(path) = config else {
        return PathBuf::from(DEFAULT_CONFIG_PATH);
    };
    let Some(raw) = path.to_str() else {
        return PathBuf::from(DEFAULT_CONFIG_PATH);
    };
    if extract_config_stem(raw).is_none() {
        return PathBuf::from(DEFAULT_CONFIG_PATH);
    }
    let has_explicit_prefix =
        raw.starts_with('/') || raw.starts_with("./") || raw.starts_with("../");
    let has_conf_extension = raw.ends_with(".conf");

    let mut resolved = String::with_capacity(raw.len() + 7);
    if !has_explicit_prefix {
        resolved.push_str("./");
    }
    resolved.push_str(raw);
    if !has_conf_extension {
        resolved.push_str(".conf");
    }
    PathBuf::from(resolved)
}

/// Clamps and sanitizes a filesystem `path` into a bounded [`MAX_ERROR_TOKEN_BYTES`] error token.
#[must_use]
pub fn sanitize_path_token(path: &Path) -> String {
    if let Some(valid_str) = path.to_str() {
        return sanitize_text(valid_str, MAX_ERROR_TOKEN_BYTES);
    }
    let raw_bytes = path.as_os_str().as_encoded_bytes();
    let max_scan = MAX_ERROR_TOKEN_BYTES.saturating_add(3);
    let prefix_bytes = raw_bytes.get(..max_scan).unwrap_or(raw_bytes);
    let lossy = String::from_utf8_lossy(prefix_bytes);
    sanitize_text(&lossy, MAX_ERROR_TOKEN_BYTES)
}

/// Verifies that `editor` is non-empty and structurally valid for direct process execution.
///
/// # Errors
///
/// Returns [`PodError`] with [`ConfigErrorKind::MissingEditor`] when `editor` is empty or
/// whitespace-only, or [`ConfigErrorKind::InvalidPath`] when `editor` is not a valid executable
/// name or path.
pub fn validate_editor(editor: &str) -> Result<&str, PodError> {
    if editor.trim().is_empty() {
        return Err(PodError::from(ConfigError::new(
            ConfigErrorKind::MissingEditor,
            None,
        )));
    }
    if !is_valid_editor_value(editor) {
        return Err(PodError::from(ConfigError::without_line(
            ConfigErrorKind::InvalidPath,
            editor,
        )));
    }
    Ok(editor)
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

/// Evaluates whether `stem` is non-empty and contains at least one non-dot byte.
fn has_valid_stem(stem: &str) -> bool {
    !stem.is_empty() && !stem.bytes().all(|byte| byte == b'.')
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::{Path, PathBuf};

    use crate::error::{ConfigErrorKind, MAX_ERROR_TOKEN_BYTES, PodError};
    use super::{
        build_full_path, extract_config_stem, has_parent_dir, has_regular_target_name,
        is_valid_directory, is_valid_directory_path, is_valid_editor_value, is_valid_target_path,
        matches_extension, normalize_lexical_path, parent_or_cur_dir, pop_relative_or_absolute,
        relative_display_path, resolve_config_name, resolve_config_path, sanitize_path_token,
        validate_editor,
    };

    #[test]
    fn build_full_path_and_directory_validation_and_sanitization() {
        assert_eq!(
            build_full_path(Path::new("."), "recipe", ".pod"),
            PathBuf::from("./recipe.pod")
        );
        assert_eq!(
            build_full_path(Path::new("./recipes"), "recipe.pod", ".pod"),
            PathBuf::from("./recipes/recipe.pod")
        );
        assert_eq!(
            build_full_path(Path::new("pods"), "si_po", ""),
            PathBuf::from("./pods/si_po")
        );
        assert_eq!(
            build_full_path(Path::new(".pods"), "si_po", ""),
            PathBuf::from("./.pods/si_po")
        );
        assert_eq!(
            build_full_path(Path::new(".recipes"), "recipe", ".pod"),
            PathBuf::from("./.recipes/recipe.pod")
        );
        assert_eq!(
            build_full_path(Path::new("/pods"), "nopathpod", ""),
            PathBuf::from("/pods/nopathpod")
        );
        assert_eq!(
            build_full_path(Path::new(""), "recipe", ".pod"),
            PathBuf::from("./recipe.pod")
        );

        assert!(is_valid_directory(Path::new(".")));
        assert!(is_valid_directory(Path::new("/var/lib/pods")));
        assert!(!is_valid_directory(Path::new("../pods")));
        let non_utf8_dir = PathBuf::from(OsString::from_vec(vec![b'/', 0xFF]));
        assert!(!is_valid_directory(&non_utf8_dir));

        assert!(is_valid_directory_path("."));
        assert!(is_valid_directory_path("./pods"));
        assert!(is_valid_directory_path("/var/lib/pods"));
        assert!(!is_valid_directory_path(""));
        assert!(!is_valid_directory_path("/"));
        assert!(!is_valid_directory_path("-pods"));
        assert!(!is_valid_directory_path("../pods"));
        assert!(!is_valid_directory_path("/var:pods"));
        assert!(!is_valid_directory_path("/var/pods\n"));

        assert!(is_valid_editor_value("vim"));
        assert!(is_valid_editor_value("/usr/bin/nvim"));
        assert!(!is_valid_editor_value(""));
        assert!(!is_valid_editor_value("-vim"));
        assert!(!is_valid_editor_value("/usr/bin/-vim"));
        assert!(!is_valid_editor_value("../vim"));
        assert!(!is_valid_editor_value("/usr/bin/"));
        assert!(!is_valid_editor_value("vim -u NONE"));
        assert_eq!(validate_editor("vim").unwrap(), "vim");
        assert!(matches!(
            validate_editor("   "),
            Err(PodError::Config(error)) if error.kind == ConfigErrorKind::MissingEditor
        ));
        assert!(matches!(
            validate_editor("-vim"),
            Err(PodError::Config(error)) if error.kind == ConfigErrorKind::InvalidPath
        ));

        assert!(has_regular_target_name(Path::new("./pods.conf")));
        assert!(!has_regular_target_name(Path::new("")));
        assert!(!has_regular_target_name(Path::new(".")));
        assert!(!has_regular_target_name(Path::new("..")));
        assert!(!has_regular_target_name(Path::new("./pods.conf/")));
        assert!(!has_regular_target_name(Path::new("./pods.conf/.")));

        assert_eq!(
            sanitize_path_token(Path::new("/tmp/evil\x1b[31m.pod")),
            "/tmp/evil [31m.pod"
        );
    }

    #[test]
    fn extract_config_stem_and_resolve_config_helpers() {
        assert_eq!(extract_config_stem("pods"), Some("pods"));
        assert_eq!(extract_config_stem("./pods.conf"), Some("pods"));
        assert_eq!(extract_config_stem(".hidden"), Some(".hidden"));
        assert_eq!(extract_config_stem("./.hidden.conf"), Some(".hidden"));
        assert_eq!(
            extract_config_stem("../my.multi.ext.conf"),
            Some("my.multi.ext")
        );
        assert_eq!(
            extract_config_stem("/etc/pods/staging.conf"),
            Some("staging")
        );

        assert_eq!(resolve_config_name(None), "pods");
        assert_eq!(resolve_config_name(Some(Path::new("staging"))), "staging");
        assert_eq!(resolve_config_name(Some(Path::new("."))), "pods");
        assert_eq!(resolve_config_path(None), PathBuf::from("./pods.conf"));
        assert_eq!(
            resolve_config_path(Some(Path::new("staging"))),
            PathBuf::from("./staging.conf")
        );
        assert_eq!(
            resolve_config_path(Some(Path::new("/etc/pods/staging.conf"))),
            PathBuf::from("/etc/pods/staging.conf")
        );
        assert_eq!(
            resolve_config_path(Some(Path::new("."))),
            PathBuf::from("./pods.conf")
        );

        for invalid in [
            "",
            " ",
            " pods.conf",
            "pods.conf ",
            "pods.conf\n",
            "-pods.conf",
            "/tmp/-pods.conf",
            "/",
            "./",
            "../",
            "configs/",
            ".",
            "..",
            "...",
            ".conf",
            "/tmp/.conf",
            "..conf",
            "...conf",
            "/tmp/..conf",
            "/tmp/...conf",
            "pods\u{202E}.conf",
            "/tmp/\u{2066}pods.conf",
        ] {
            assert_eq!(
                extract_config_stem(invalid),
                None,
                "expected None for {invalid:?}"
            );
        }
    }

    #[test]
    fn parent_dir_extension_and_lexical_normalization() {
        assert!(has_parent_dir(Path::new("..")));
        assert!(has_parent_dir(Path::new("../pods")));
        assert!(has_parent_dir(Path::new("/var/pods/../etc")));
        assert!(!has_parent_dir(Path::new("./pods")));
        assert!(!has_parent_dir(Path::new("/var/lib/pods")));

        assert!(matches_extension("app.pod", Some(".pod")));
        assert!(matches_extension(".hidden.pod", Some(".pod")));
        assert!(!matches_extension(".pod", Some(".pod")));
        assert!(!matches_extension("..pod", Some(".pod")));
        assert!(!matches_extension("...pod", Some(".pod")));
        assert!(!matches_extension("app.txt", Some(".pod")));
        assert!(matches_extension("anything", None));
        assert!(!matches_extension("", None));
        assert!(!matches_extension(".", None));
        assert!(!matches_extension("..", None));

        assert_eq!(normalize_lexical_path(Path::new("")), PathBuf::new());
        assert_eq!(normalize_lexical_path(Path::new("/..")), PathBuf::from("/"));
        assert_eq!(normalize_lexical_path(Path::new("..")), PathBuf::from(".."));
        assert_eq!(normalize_lexical_path(Path::new("./a/..")), PathBuf::from("."));
        assert_eq!(
            normalize_lexical_path(Path::new("a/../../b")),
            PathBuf::from("../b")
        );
        assert_eq!(
            normalize_lexical_path(Path::new("/../../etc/./pods")),
            PathBuf::from("/etc/pods")
        );

        let mut dot_path = PathBuf::from(".");
        pop_relative_or_absolute(&mut dot_path);
        assert_eq!(dot_path, PathBuf::from(".."));
        pop_relative_or_absolute(&mut dot_path);
        assert_eq!(dot_path, PathBuf::from("../.."));

        let mut empty_path = PathBuf::new();
        pop_relative_or_absolute(&mut empty_path);
        assert_eq!(empty_path, PathBuf::from(".."));

        let mut root_path = PathBuf::from("/");
        pop_relative_or_absolute(&mut root_path);
        assert_eq!(root_path, PathBuf::from("/"));

        assert!(is_valid_target_path(Path::new("./recipe.pod")));
        assert!(is_valid_target_path(Path::new("./pods.conf")));
        assert!(is_valid_target_path(Path::new("./.hidden.pod")));
        assert!(!is_valid_target_path(Path::new("")));
        assert!(!is_valid_target_path(Path::new(".pod")));
        assert!(!is_valid_target_path(Path::new("./.pod")));
        assert!(!is_valid_target_path(Path::new("/tmp/.pod")));
        assert!(!is_valid_target_path(Path::new("..pod")));
        assert!(!is_valid_target_path(Path::new("./...pod")));
        assert!(!is_valid_target_path(Path::new("..conf")));
        assert!(!is_valid_target_path(Path::new("...conf")));
        assert!(!is_valid_target_path(Path::new("../pods.conf")));
        assert!(!is_valid_target_path(Path::new("/tmp/../pods.conf")));
        let non_utf8_path = PathBuf::from(OsString::from_vec(vec![b'.', b'/', 0xFF, b'.', b'c']));
        assert!(!is_valid_target_path(&non_utf8_path));

        assert_eq!(parent_or_cur_dir(Path::new("./recipe.pod")), Path::new("."));
        assert_eq!(parent_or_cur_dir(Path::new("recipe.pod")), Path::new("."));
        assert_eq!(
            parent_or_cur_dir(Path::new("/tmp/workspace/recipe.pod")),
            Path::new("/tmp/workspace")
        );

        assert_eq!(
            relative_display_path(Path::new("."), Path::new(".")),
            Path::new(".")
        );
        assert_eq!(
            relative_display_path(Path::new("/tmp/workspace"), Path::new("/tmp/workspace")),
            Path::new(".")
        );
        assert_eq!(
            relative_display_path(Path::new("./recipe.pod"), Path::new(".")),
            Path::new("recipe.pod")
        );
        assert_eq!(
            relative_display_path(Path::new("./pods.conf"), Path::new(".")),
            Path::new("pods.conf")
        );
        assert_eq!(
            relative_display_path(
                Path::new("/tmp/workspace/recipe.pod"),
                Path::new("/tmp/workspace")
            ),
            Path::new("recipe.pod")
        );
        assert_eq!(
            relative_display_path(Path::new("/etc/pods/staging.conf"), Path::new(".")),
            Path::new("/etc/pods/staging.conf")
        );
    }

    #[test]
    fn sanitize_path_token_respects_utf8_boundaries_and_non_utf8_osstr() {
        let exact = "a".repeat(MAX_ERROR_TOKEN_BYTES);
        assert_eq!(
            sanitize_path_token(Path::new(&exact)).len(),
            MAX_ERROR_TOKEN_BYTES
        );

        let over = "a".repeat(MAX_ERROR_TOKEN_BYTES + 1);
        assert_eq!(
            sanitize_path_token(Path::new(&over)).len(),
            MAX_ERROR_TOKEN_BYTES
        );

        let mut boundary_str = "a".repeat(MAX_ERROR_TOKEN_BYTES - 3);
        boundary_str.push('\u{1F980}');
        let sanitized_boundary = sanitize_path_token(Path::new(&boundary_str));
        assert_eq!(sanitized_boundary.len(), MAX_ERROR_TOKEN_BYTES - 3);
        assert!(!sanitized_boundary.contains('\u{FFFD}'));

        let mut raw_non_utf8 = vec![b'a'; MAX_ERROR_TOKEN_BYTES - 3];
        raw_non_utf8.extend_from_slice("\u{1F980}".as_bytes());
        raw_non_utf8[0] = 0xFF;
        let non_utf8_path = PathBuf::from(OsString::from_vec(raw_non_utf8));
        let sanitized_non_utf8 = sanitize_path_token(&non_utf8_path);
        assert!(sanitized_non_utf8.len() <= MAX_ERROR_TOKEN_BYTES);
        assert!(sanitized_non_utf8.starts_with('\u{FFFD}'));
        assert!(!sanitized_non_utf8.ends_with('\u{FFFD}'));
    }
}
