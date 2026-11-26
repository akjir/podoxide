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

//! Host path canonicalization, working-directory anchoring, and bounded symlink resolution.

use std::borrow::Cow;
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::utilities::path;

const MAX_SYMLINK_DEPTH: usize = 40;

/// Resolves the normalized absolute path for `target_path` without requiring the target to exist on disk.
#[must_use]
pub(super) fn get_absolute_path(target_path: &Path) -> PathBuf {
    if target_path.as_os_str().is_empty() {
        return PathBuf::new();
    }
    let working_path = resolve_working_path(target_path);
    if let Ok(canonical) = fs::canonicalize(working_path.as_ref()) {
        return canonical;
    }
    if working_path.is_absolute() {
        return resolve_absolute_components(working_path.as_ref());
    }
    path::normalize_lexical_path(working_path.as_ref())
}

/// Anchors a relative `target_path` to the process current working directory (`env::current_dir()`),
/// or returns `target_path` borrowed as-is when it is already absolute or empty.
pub(super) fn resolve_working_path(target_path: &Path) -> Cow<'_, Path> {
    if target_path.as_os_str().is_empty() || target_path.is_absolute() {
        return Cow::Borrowed(target_path);
    }
    let Ok(current_dir) = env::current_dir() else {
        return Cow::Borrowed(target_path);
    };
    Cow::Owned(current_dir.join(target_path))
}

/// Appends and resolves all components of `target_path` onto `resolved` with bounded symlink expansion.
fn append_resolved_path(
    resolved: &mut PathBuf,
    target_path: &Path,
    remaining_symlinks: &mut usize,
) {
    for component in target_path.components() {
        apply_resolved_component(resolved, component, remaining_symlinks);
    }
}

/// Applies a path component to `resolved` and resolves existing or symlinked prefixes before `..`.
fn apply_resolved_component(
    resolved: &mut PathBuf,
    component: Component<'_>,
    remaining_symlinks: &mut usize,
) {
    match component {
        Component::Prefix(prefix) => resolved.push(prefix.as_os_str()),
        Component::RootDir => {
            resolved.clear();
            resolved.push(component.as_os_str());
        }
        Component::CurDir => {}
        Component::ParentDir => path::pop_relative_or_absolute(resolved),
        Component::Normal(part) => {
            resolved.push(part);
            resolve_normal_component(resolved, remaining_symlinks);
        }
    }
}

/// Resolves `target_path` component by component, canonicalizing existing prefixes and expanding symlinks along the way.
fn resolve_absolute_components(target_path: &Path) -> PathBuf {
    let mut resolved = PathBuf::with_capacity(target_path.as_os_str().len());
    let mut remaining_symlinks = MAX_SYMLINK_DEPTH;
    append_resolved_path(&mut resolved, target_path, &mut remaining_symlinks);
    resolved
}

/// Resolves the most recently appended normal component in `resolved` via canonicalization or bounded symlink expansion.
fn resolve_normal_component(resolved: &mut PathBuf, remaining_symlinks: &mut usize) {
    if let Ok(canonical) = fs::canonicalize(&*resolved) {
        *resolved = canonical;
        return;
    }
    if *remaining_symlinks == 0 {
        return;
    }
    let Ok(link_target) = fs::read_link(&*resolved) else {
        return;
    };
    *remaining_symlinks -= 1;
    let _ = resolved.pop();
    append_resolved_path(resolved, &link_target, remaining_symlinks);
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs as unix_fs;
    use std::path::{Path, PathBuf};

    use crate::io::system::fs::TestTempDir;
    use crate::utilities::path;
    use super::get_absolute_path;

    #[test]
    fn resolves_existing_missing_relative_and_symlinked_paths() {
        let temp_dir = TestTempDir::new("path_resolve");
        let file_path = temp_dir.join("sample.pod");
        fs::write(&file_path, "pod app {}\n").unwrap();

        assert_eq!(get_absolute_path(Path::new("")), PathBuf::new());
        let existing_abs = get_absolute_path(&file_path);
        assert!(existing_abs.is_absolute());
        assert!(existing_abs.ends_with("sample.pod"));

        let missing_abs = get_absolute_path(Path::new("/var/tmp/./nonexistent/../pods"));
        assert_eq!(missing_abs, PathBuf::from("/var/tmp/pods"));

        let missing_rel = get_absolute_path(Path::new("./nonexistent_dir/../pods.conf"));
        assert!(missing_rel.is_absolute());
        assert!(missing_rel.ends_with("pods.conf"));

        let real_nested = temp_dir.join("nested").join("real_dir");
        fs::create_dir_all(&real_nested).unwrap();
        let sym_dir = temp_dir.join("sym_dir");
        unix_fs::symlink(&real_nested, &sym_dir).unwrap();

        let canonical_nested = fs::canonicalize(temp_dir.join("nested")).unwrap();
        assert_eq!(
            get_absolute_path(&sym_dir.join("../missing.pod")),
            canonical_nested.join("missing.pod")
        );
        assert_eq!(
            path::normalize_lexical_path(Path::new("a/../../b")),
            PathBuf::from("../b")
        );
        assert_eq!(
            path::normalize_lexical_path(Path::new("./a/..")),
            PathBuf::from(".")
        );
        assert_eq!(
            path::normalize_lexical_path(Path::new("/../../etc")),
            PathBuf::from("/etc")
        );

        let dangling_target = temp_dir.join("dangling_target.conf");
        let dangling_link = temp_dir.join("dangling.conf");
        unix_fs::symlink(&dangling_target, &dangling_link).unwrap();
        let canonical_temp = fs::canonicalize(&temp_dir).unwrap();
        assert_eq!(
            get_absolute_path(&dangling_link),
            canonical_temp.join("dangling_target.conf")
        );

        let dangling_rel_dir = temp_dir.join("dangling_rel_dir");
        unix_fs::symlink(Path::new("nested/missing_sub"), &dangling_rel_dir).unwrap();
        assert_eq!(
            get_absolute_path(&dangling_rel_dir.join("../escaped.pod")),
            canonical_nested.join("escaped.pod")
        );

        let dangling_abs_dir = temp_dir.join("dangling_abs_dir");
        unix_fs::symlink(canonical_nested.join("missing_abs"), &dangling_abs_dir).unwrap();
        assert_eq!(
            get_absolute_path(&dangling_abs_dir.join("../abs_escaped.pod")),
            canonical_nested.join("abs_escaped.pod")
        );

        let loop_a = temp_dir.join("loop_a");
        let loop_b = temp_dir.join("loop_b");
        unix_fs::symlink(&loop_b, &loop_a).unwrap();
        unix_fs::symlink(&loop_a, &loop_b).unwrap();
        assert!(get_absolute_path(&loop_a.join("cycle.pod")).ends_with("cycle.pod"));
    }
}
