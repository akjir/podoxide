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

use std::path::{Component, Path, PathBuf};

use crate::utilities::string::is_bidi_control;

/// Extracts the configuration file stem from a raw `.conf` path string if it is structurally valid.
#[must_use]
pub fn extract_config_stem(raw_value: &str) -> Option<&str> {
    if raw_value.is_empty()
        || raw_value != raw_value.trim()
        || raw_value.starts_with('-')
        || raw_value.ends_with('/')
        || raw_value
            .chars()
            .any(|character| character.is_control() || is_bidi_control(character))
    {
        return None;
    }
    let file_name = raw_value.rsplit('/').next().unwrap_or(raw_value);
    let stem = file_name.strip_suffix(".conf").unwrap_or(file_name);
    if file_name.is_empty()
        || file_name == "."
        || file_name == ".."
        || file_name.starts_with('-')
        || stem.is_empty()
        || stem == "."
        || stem == ".."
    {
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

/// Evaluates whether `path` is a structurally valid, UTF-8 target file path without `..` traversal.
#[must_use]
pub fn is_valid_target_path(path: &Path) -> bool {
    let Some(raw_path) = path.to_str() else {
        return false;
    };
    !has_parent_dir(path) && extract_config_stem(raw_path).is_some()
}

/// Evaluates whether `file_name` ends with `extension` and retains a non-empty stem before it.
#[must_use]
pub fn matches_extension(file_name: &str, extension: Option<&str>) -> bool {
    let Some(required_extension) = extension else {
        return true;
    };
    let Some(stem) = file_name.strip_suffix(required_extension) else {
        return false;
    };
    !stem.is_empty()
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
    relative.strip_prefix("./").unwrap_or(relative)
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::{Path, PathBuf};

    use super::{
        extract_config_stem, has_parent_dir, is_valid_target_path, matches_extension,
        normalize_lexical_path, parent_or_cur_dir, pop_relative_or_absolute,
        relative_display_path,
    };

    #[test]
    fn extract_config_stem_handles_valid_and_invalid_paths() {
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
            ".conf",
            "/tmp/.conf",
            "..conf",
            "/tmp/..conf",
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
        assert!(!matches_extension("app.txt", Some(".pod")));
        assert!(matches_extension("anything", None));

        assert_eq!(normalize_lexical_path(Path::new("")), PathBuf::new());
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

        assert!(is_valid_target_path(Path::new("./recipe.pod")));
        assert!(is_valid_target_path(Path::new("./pods.conf")));
        assert!(!is_valid_target_path(Path::new("")));
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
}
