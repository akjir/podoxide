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

//! Host filesystem operations, bounded file reading, and path resolution.

use std::borrow::Cow;
use std::env;
use std::fs::{self, DirEntry, File, Metadata};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use crate::error::{PodError, PslError, PslErrorKind};
use crate::io::psl::MAX_FILE_BYTES;
use crate::utilities::path::{
    matches_extension, normalize_lexical_path, parent_or_cur_dir, pop_relative_or_absolute,
};

const INITIAL_DIRECTORY_CAPACITY: usize = 16;
const INITIAL_FILE_CAPACITY: usize = 256;
/// Maximum number of entries collected from a single directory listing.
pub const MAX_DIRECTORY_ENTRIES: usize = 4096;
const MAX_SYMLINK_DEPTH: usize = 40;

/// Evaluates whether a directory exists at `path`.
#[must_use]
pub fn directory_exists(path: &Path) -> bool {
    if path.as_os_str().is_empty() {
        return false;
    }
    let working_path = resolve_working_path(path);
    fs::metadata(working_path.as_ref()).is_ok_and(|metadata| metadata.is_dir())
}

/// Verifies that `path` either does not exist yet (within an existing parent directory) or is an
/// existing regular file (and not a symlink or directory).
///
/// # Errors
///
/// Returns [`PodError::Io`] if `path` is a symlink, directory, or special file, or if its parent
/// directory does not exist.
pub fn ensure_writable_target(path: &Path) -> Result<(), PodError> {
    if !has_regular_target_name(path) {
        return Err(invalid_target_error(path));
    }
    let working_path = resolve_working_path(path);
    let metadata = match fs::symlink_metadata(working_path.as_ref()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return ensure_parent_directory(working_path.as_ref());
        }
        Err(error) => return Err(io_path_error(path, error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid_target_error(path));
    }
    Ok(())
}

/// Evaluates whether a regular file exists at `path`.
#[must_use]
pub fn file_exists(path: &Path) -> bool {
    if path.as_os_str().is_empty() {
        return false;
    }
    let working_path = resolve_working_path(path);
    fs::metadata(working_path.as_ref()).is_ok_and(|metadata| metadata.is_file())
}

/// Resolves the normalized absolute path for `path` without requiring the target to exist on disk.
#[must_use]
pub fn get_absolute_path(path: &Path) -> PathBuf {
    if path.as_os_str().is_empty() {
        return PathBuf::new();
    }
    let working_path = resolve_working_path(path);
    if let Ok(canonical) = fs::canonicalize(working_path.as_ref()) {
        return canonical;
    }
    if working_path.is_absolute() {
        return resolve_absolute_components(working_path.as_ref());
    }
    normalize_lexical_path(working_path.as_ref())
}

/// Lists entry names in `path`, optionally filtered by `extension` (such as `Some(".pod")`),
/// sorted alphabetically.
///
/// # Errors
///
/// Returns [`PodError`] if `path` cannot be read or if the number of scanned or matching entries
/// exceeds [`MAX_DIRECTORY_ENTRIES`].
pub fn list_directory(path: &Path, extension: Option<&str>) -> Result<Vec<String>, PodError> {
    let working_path = resolve_working_path(path);
    let read_dir = fs::read_dir(working_path.as_ref()).map_err(|error| io_path_error(path, error))?;
    let mut entries = Vec::with_capacity(INITIAL_DIRECTORY_CAPACITY.min(MAX_DIRECTORY_ENTRIES));
    for (scanned_count, entry_result) in read_dir.enumerate() {
        if scanned_count >= MAX_DIRECTORY_ENTRIES {
            return Err(directory_limit_error());
        }
        collect_matching_entry(entry_result, extension, &mut entries)?;
    }
    entries.sort_unstable();
    Ok(entries)
}

/// Reads the entire UTF-8 text content of `path`, enforcing [`MAX_FILE_BYTES`] before and
/// during allocation.
///
/// # Errors
///
/// Returns [`PodError::Io`] if opening, verifying, or reading `path` fails, or [`PodError::Psl`]
/// with [`PslErrorKind::LimitExceeded`] if the file size exceeds [`MAX_FILE_BYTES`].
pub fn read_file(path: &Path) -> Result<String, PodError> {
    let working_path = resolve_working_path(path);
    let path_metadata =
        fs::metadata(working_path.as_ref()).map_err(|error| io_path_error(path, error))?;
    ensure_regular_file(path, &path_metadata)?;
    let file = File::open(working_path.as_ref()).map_err(|error| io_path_error(path, error))?;
    let metadata = file.metadata().map_err(|error| io_path_error(path, error))?;
    ensure_regular_file(path, &metadata)?;
    if metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(PodError::from(PslError::new(
            PslErrorKind::LimitExceeded,
            1,
        )));
    }
    let initial_capacity = usize::try_from(metadata.len()).unwrap_or(INITIAL_FILE_CAPACITY);
    read_bounded_utf8(file, initial_capacity)
}

/// Writes `content` to `path`, creating or truncating the target regular file.
///
/// # Errors
///
/// Returns [`PodError::Io`] if `path` is a symlink or non-regular file, or if writing fails.
pub fn write_file(path: &Path, content: &str) -> Result<(), PodError> {
    ensure_writable_target(path)?;
    let working_path = resolve_working_path(path);
    fs::write(working_path.as_ref(), content).map_err(|error| io_path_error(path, error))?;
    Ok(())
}

/// Appends and resolves all components of `path` onto `resolved` with bounded symlink expansion.
fn append_resolved_path(resolved: &mut PathBuf, path: &Path, remaining_symlinks: &mut usize) {
    for component in path.components() {
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
        Component::ParentDir => pop_relative_or_absolute(resolved),
        Component::Normal(part) => {
            resolved.push(part);
            resolve_normal_component(resolved, remaining_symlinks);
        }
    }
}

/// Appends a directory entry name to `entries` if it is valid UTF-8, matches `extension`, and stays within [`MAX_DIRECTORY_ENTRIES`].
fn collect_matching_entry(
    entry_result: io::Result<DirEntry>,
    extension: Option<&str>,
    entries: &mut Vec<String>,
) -> Result<(), PodError> {
    if entries.len() >= MAX_DIRECTORY_ENTRIES {
        return Err(directory_limit_error());
    }
    let entry = entry_result?;
    let file_name = entry.file_name();
    let Some(name_str) = file_name.to_str() else {
        return Ok(());
    };
    if !matches_extension(name_str, extension) {
        return Ok(());
    }
    entries.push(name_str.to_owned());
    Ok(())
}

/// Creates a [`PodError::Io`] indicating that [`MAX_DIRECTORY_ENTRIES`] was exceeded.
fn directory_limit_error() -> PodError {
    PodError::from(io::Error::new(
        io::ErrorKind::InvalidData,
        "directory entry limit exceeded",
    ))
}

/// Verifies that the parent directory of `path` exists on disk.
fn ensure_parent_directory(path: &Path) -> Result<(), PodError> {
    let parent = parent_or_cur_dir(path);
    if !directory_exists(parent) {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Parent directory '{}' does not exist!", parent.display()),
        )));
    }
    Ok(())
}

/// Verifies that `metadata` describes a regular file, rejecting directories, FIFOs, and device nodes.
fn ensure_regular_file(path: &Path, metadata: &Metadata) -> Result<(), PodError> {
    if !metadata.is_file() {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("'{}' is not a regular file", path.display()),
        )));
    }
    Ok(())
}

/// Evaluates whether `path` names a terminal file entry without a trailing slash or dot segment.
fn has_regular_target_name(path: &Path) -> bool {
    let raw_bytes = path.as_os_str().as_encoded_bytes();
    path.file_name().is_some() && !raw_bytes.ends_with(b"/") && !raw_bytes.ends_with(b"/.")
}

/// Creates a [`PodError::Io`] indicating that `path` is not a valid regular-file write target.
fn invalid_target_error(path: &Path) -> PodError {
    PodError::from(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "target path '{}' must be a regular file and not a symlink",
            path.display()
        ),
    ))
}

/// Annotates an [`io::Error`] with the offending `path` while preserving its [`io::ErrorKind`].
fn io_path_error(path: &Path, error: io::Error) -> PodError {
    PodError::from(io::Error::new(
        error.kind(),
        format!("'{}': {error}", path.display()),
    ))
}

/// Reads at most [`MAX_FILE_BYTES`] bytes from `reader` and decodes the buffer as UTF-8.
fn read_bounded_utf8<R: Read>(reader: R, initial_capacity: usize) -> Result<String, PodError> {
    let mut bytes = Vec::with_capacity(initial_capacity.min(MAX_FILE_BYTES));
    let mut bounded = reader.take((MAX_FILE_BYTES + 1) as u64);
    bounded.read_to_end(&mut bytes)?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(PodError::from(PslError::new(
            PslErrorKind::LimitExceeded,
            1,
        )));
    }
    String::from_utf8(bytes)
        .map_err(|error| PodError::from(io::Error::new(io::ErrorKind::InvalidData, error)))
}

/// Resolves `path` component by component, canonicalizing existing prefixes and expanding symlinks along the way.
fn resolve_absolute_components(path: &Path) -> PathBuf {
    let mut resolved = PathBuf::with_capacity(path.as_os_str().len());
    let mut remaining_symlinks = MAX_SYMLINK_DEPTH;
    append_resolved_path(&mut resolved, path, &mut remaining_symlinks);
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

/// Anchors a relative `path` to the process current working directory (`env::current_dir()`),
/// or returns `path` borrowed as-is when `path` is already absolute or empty.
fn resolve_working_path(path: &Path) -> Cow<'_, Path> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Cow::Borrowed(path);
    }
    let Ok(current_dir) = env::current_dir() else {
        return Cow::Borrowed(path);
    };
    Cow::Owned(current_dir.join(path))
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::io::Cursor;
    use std::os::unix::fs::{self as unix_fs, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process;

    use crate::error::{PodError, PslErrorKind};
    use crate::io::psl::MAX_FILE_BYTES;
    use crate::utilities::path::{matches_extension, normalize_lexical_path};
    use super::{
        INITIAL_FILE_CAPACITY, MAX_DIRECTORY_ENTRIES, collect_matching_entry, directory_exists,
        ensure_writable_target, file_exists, get_absolute_path, list_directory, read_bounded_utf8,
        read_file, write_file,
    };

    fn create_temp_test_dir(suffix: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("podoxide_fs_test_{}_{suffix}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn directory_and_file_existence_and_path_resolution() {
        let temp_dir = create_temp_test_dir("existence");
        let file_path = temp_dir.join("sample.pod");
        write_file(&file_path, "pod app {}\n").unwrap();

        assert!(directory_exists(&temp_dir));
        assert!(!directory_exists(&file_path));
        assert!(!directory_exists(Path::new("")));
        assert!(!directory_exists(&temp_dir.join("missing_dir")));

        assert!(file_exists(&file_path));
        assert!(!file_exists(&temp_dir));
        assert!(!file_exists(Path::new("")));
        assert!(!file_exists(&temp_dir.join("missing.pod")));

        assert_eq!(get_absolute_path(Path::new("")), PathBuf::new());
        let existing_abs = get_absolute_path(&file_path);
        assert!(existing_abs.is_absolute());
        assert!(existing_abs.ends_with("sample.pod"));

        let missing_abs = get_absolute_path(Path::new("/var/tmp/./nonexistent/../pods"));
        assert_eq!(missing_abs, PathBuf::from("/var/tmp/pods"));

        let missing_rel = get_absolute_path(Path::new("./nonexistent_dir/../pods.conf"));
        assert!(missing_rel.is_absolute());
        assert!(missing_rel.ends_with("pods.conf"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn list_directory_filters_extensions_and_enforces_bounds() {
        let temp_dir = create_temp_test_dir("list_dir");
        write_file(&temp_dir.join("b.pod"), "").unwrap();
        write_file(&temp_dir.join("a.pod"), "").unwrap();
        write_file(&temp_dir.join("notes.txt"), "").unwrap();
        write_file(&temp_dir.join(".pod"), "").unwrap();
        write_file(&temp_dir.join(".hidden.pod"), "").unwrap();

        let all_files = list_directory(&temp_dir, None).unwrap();
        assert_eq!(
            all_files,
            vec![".hidden.pod", ".pod", "a.pod", "b.pod", "notes.txt"]
        );

        let pod_files = list_directory(&temp_dir, Some(".pod")).unwrap();
        assert_eq!(pod_files, vec![".hidden.pod", "a.pod", "b.pod"]);

        let missing_ext = list_directory(&temp_dir, Some(".missing")).unwrap();
        assert!(missing_ext.is_empty());

        let missing_dir = temp_dir.join("does_not_exist");
        let missing_dir_err = list_directory(&missing_dir, None).unwrap_err();
        assert!(
            missing_dir_err
                .to_string()
                .contains(missing_dir.to_str().unwrap())
        );

        assert!(matches_extension("app.pod", Some(".pod")));
        assert!(!matches_extension(".pod", Some(".pod")));
        assert!(!matches_extension("app.txt", Some(".pod")));
        assert!(matches_extension("anything", None));

        let mut full_entries = (0..MAX_DIRECTORY_ENTRIES)
            .map(|index| format!("entry_{index}.pod"))
            .collect::<Vec<_>>();
        let first_entry = fs::read_dir(&temp_dir).unwrap().next().unwrap();
        let bound_err = collect_matching_entry(first_entry, None, &mut full_entries).unwrap_err();
        assert!(
            bound_err
                .to_string()
                .contains("directory entry limit exceeded")
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn read_and_write_file_and_max_file_bytes_bound() {
        let temp_dir = create_temp_test_dir("read_write");
        let file_path = temp_dir.join("config.conf");
        let content = "general {\n    editor vim\n}\n";

        write_file(&file_path, content).unwrap();
        let loaded = read_file(&file_path).unwrap();
        assert_eq!(loaded, content);

        let missing_file = temp_dir.join("missing.conf");
        let missing_file_err = read_file(&missing_file).unwrap_err();
        assert!(
            missing_file_err
                .to_string()
                .contains(missing_file.to_str().unwrap())
        );

        let oversized_path = temp_dir.join("oversized.conf");
        let oversized_content = "a".repeat(MAX_FILE_BYTES + 1);
        write_file(&oversized_path, &oversized_content).unwrap();
        let error = read_file(&oversized_path).unwrap_err();
        match error {
            PodError::Psl(psl_error) => assert_eq!(psl_error.kind, PslErrorKind::LimitExceeded),
            other => panic!("expected PslErrorKind::LimitExceeded, got {other:?}"),
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn red_team_symlink_traversal_write_protection_and_bounds() {
        let temp_dir = create_temp_test_dir("red_team");

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
            normalize_lexical_path(Path::new("a/../../b")),
            PathBuf::from("../b")
        );
        assert_eq!(
            normalize_lexical_path(Path::new("./a/..")),
            PathBuf::from(".")
        );
        assert_eq!(
            normalize_lexical_path(Path::new("/../../etc")),
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

        assert!(!file_exists(&dangling_link));
        assert!(ensure_writable_target(&dangling_link).is_err());
        assert!(ensure_writable_target(&temp_dir.join("dangling.conf/")).is_err());
        assert!(ensure_writable_target(&temp_dir.join("dangling.conf/.")).is_err());
        assert!(write_file(&dangling_link, "overwrite").is_err());
        assert!(!file_exists(&dangling_target));

        let protected_target = temp_dir.join("protected.conf");
        write_file(&protected_target, "safe").unwrap();
        let active_link = temp_dir.join("active_link.conf");
        unix_fs::symlink(&protected_target, &active_link).unwrap();
        assert!(ensure_writable_target(&active_link).is_err());
        assert!(write_file(&active_link, "overwrite").is_err());
        assert_eq!(read_file(&protected_target).unwrap(), "safe");
        assert!(ensure_writable_target(Path::new("")).is_err());
        assert!(ensure_writable_target(Path::new(".")).is_err());
        assert!(ensure_writable_target(Path::new("..")).is_err());
        assert!(ensure_writable_target(&temp_dir).is_err());
        assert!(write_file(&temp_dir, "overwrite_dir").is_err());
        assert!(ensure_writable_target(&temp_dir.join("missing_parent/pods.conf")).is_err());
        assert!(ensure_writable_target(&temp_dir.join("new_target.conf/")).is_err());
        assert!(ensure_writable_target(&temp_dir.join("new_target.conf/.")).is_err());
        assert!(ensure_writable_target(&temp_dir.join("new_target.conf/..")).is_err());
        assert!(ensure_writable_target(&temp_dir.join("new_target.conf")).is_ok());
        assert!(ensure_writable_target(Path::new("nonexistent_relative_target.conf")).is_ok());

        let write_only_file = temp_dir.join("write_only.conf");
        write_file(&write_only_file, "secret").unwrap();
        fs::set_permissions(&write_only_file, fs::Permissions::from_mode(0o200)).unwrap();
        assert!(file_exists(&write_only_file));
        fs::set_permissions(&write_only_file, fs::Permissions::from_mode(0o600)).unwrap();

        assert!(read_file(Path::new("/dev/null")).is_err());
        assert!(read_file(Path::new("/dev/zero")).is_err());
        assert!(read_file(&temp_dir).is_err());
        assert!(!file_exists(Path::new("/dev/null")));

        let mut straddling_utf8 = vec![b'a'; MAX_FILE_BYTES];
        straddling_utf8.extend_from_slice("ä".as_bytes());
        let utf8_bound_err =
            read_bounded_utf8(Cursor::new(straddling_utf8), INITIAL_FILE_CAPACITY).unwrap_err();
        match utf8_bound_err {
            PodError::Psl(psl_error) => assert_eq!(psl_error.kind, PslErrorKind::LimitExceeded),
            other => panic!("expected PslErrorKind::LimitExceeded, got {other:?}"),
        }

        let invalid_utf8 = Cursor::new(vec![0xFF, 0xFE]);
        assert!(matches!(
            read_bounded_utf8(invalid_utf8, INITIAL_FILE_CAPACITY),
            Err(PodError::Io(_))
        ));

        let mut full_entries = (0..MAX_DIRECTORY_ENTRIES)
            .map(|index| format!("entry_{index}.pod"))
            .collect::<Vec<_>>();
        let first_entry = fs::read_dir(&temp_dir).unwrap().next().unwrap();
        let non_matching_bound_err =
            collect_matching_entry(first_entry, Some(".never_matches"), &mut full_entries)
                .unwrap_err();
        assert!(
            non_matching_bound_err
                .to_string()
                .contains("directory entry limit exceeded")
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
