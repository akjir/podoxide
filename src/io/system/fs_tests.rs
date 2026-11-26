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

//! Unit tests for host filesystem operations in [`super`].

use std::error::Error;
use std::fs;
use std::io::{self, Cursor};
use std::os::unix::fs::{self as unix_fs, PermissionsExt};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{PodError, PslErrorKind};
use crate::io::psl::MAX_FILE_BYTES;
use crate::utilities::path;
use super::{
    INITIAL_FILE_CAPACITY, MAX_DIRECTORY_ENTRIES, TestTempDir, collect_matching_entry,
    directory_exists, ensure_writable_target, file_exists, get_absolute_path, io_path_error,
    list_directory, read_bounded_utf8, read_file, write_file, write_file_exclusive,
    write_file_nofollow,
};

#[test]
fn directory_and_file_existence_and_path_forwarding() {
    let temp_dir = TestTempDir::new("fs_existence");
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
    assert!(!file_exists(Path::new("/dev/null")));

    let abs = get_absolute_path(&file_path);
    assert!(abs.is_absolute());
    assert!(abs.ends_with("sample.pod"));
}

#[test]
fn list_directory_skips_subdirectories_filters_extensions_and_enforces_bounds() {
    let temp_dir = TestTempDir::new("fs_list_dir");
    write_file(&temp_dir.join("b.pod"), "").unwrap();
    write_file(&temp_dir.join("a.pod"), "").unwrap();
    write_file(&temp_dir.join("notes.txt"), "").unwrap();
    write_file(&temp_dir.join(".pod"), "").unwrap();
    write_file(&temp_dir.join(".hidden.pod"), "").unwrap();
    fs::create_dir_all(temp_dir.join("dir_as_recipe.pod")).unwrap();
    fs::create_dir_all(temp_dir.join("plain_subdir")).unwrap();

    let all_files = list_directory(&temp_dir, None).unwrap();
    assert_eq!(
        all_files,
        vec![".hidden.pod", ".pod", "a.pod", "b.pod", "notes.txt"]
    );

    let pod_files = list_directory(&temp_dir, Some(".pod")).unwrap();
    assert_eq!(pod_files, vec![".hidden.pod", "a.pod", "b.pod"]);

    let symlink_dir = TestTempDir::new("fs_list_symlinks");
    let target_pod = symlink_dir.join("target.pod");
    let subdir = symlink_dir.join("subdir");
    write_file(&target_pod, "pod app {}\n").unwrap();
    fs::create_dir_all(&subdir).unwrap();
    unix_fs::symlink(&target_pod, symlink_dir.join("link.pod")).unwrap();
    unix_fs::symlink(&subdir, symlink_dir.join("dir_link.pod")).unwrap();
    unix_fs::symlink(
        symlink_dir.join("missing.pod"),
        symlink_dir.join("broken.pod"),
    )
    .unwrap();
    assert_eq!(
        list_directory(&symlink_dir, Some(".pod")).unwrap(),
        vec!["link.pod", "target.pod"],
        "list_directory must include regular-file symlinks and skip directory/dangling symlinks"
    );

    let missing_ext = list_directory(&temp_dir, Some(".missing")).unwrap();
    assert!(missing_ext.is_empty());

    let missing_dir = temp_dir.join("does_not_exist");
    let missing_dir_err = list_directory(&missing_dir, None).unwrap_err();
    assert!(
        missing_dir_err
            .to_string()
            .contains(missing_dir.to_str().unwrap())
    );

    let mid_iter_err = collect_matching_entry(
        &temp_dir,
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
        None,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        mid_iter_err
            .to_string()
            .contains(temp_dir.to_str().unwrap())
    );
    assert!(mid_iter_err.source().unwrap().source().is_some());

    assert!(path::matches_extension("app.pod", Some(".pod")));
    assert!(!path::matches_extension(".pod", Some(".pod")));

    let mut full_entries = (0..MAX_DIRECTORY_ENTRIES)
        .map(|index| format!("entry_{index}.pod"))
        .collect::<Vec<_>>();
    let first_entry = fs::read_dir(&temp_dir).unwrap().next().unwrap();
    let bound_err =
        collect_matching_entry(&temp_dir, first_entry, None, &mut full_entries).unwrap_err();
    assert!(
        bound_err
            .to_string()
            .contains("directory entry limit exceeded")
    );
}

#[test]
fn read_write_exclusive_nofollow_nonblock_and_error_source_preservation() {
    let temp_dir = TestTempDir::new("fs_read_write_sec");
    let file_path = temp_dir.join("config.conf");
    let content = "general {\n    editor vim\n}\n";

    write_file(&file_path, content).unwrap();
    assert_eq!(read_file(&file_path).unwrap(), content);

    let missing_file = temp_dir.join("missing.conf");
    let missing_err = read_file(&missing_file).unwrap_err();
    assert!(
        missing_err
            .to_string()
            .contains(missing_file.to_str().unwrap())
    );
    let io_err = missing_err.source().expect("PodError::Io source");
    assert!(io_err.source().is_some(), "PathIoError must chain source");

    let direct_wrapped = io_path_error(
        &missing_file,
        io::Error::new(io::ErrorKind::PermissionDenied, "blocked"),
    );
    assert!(matches!(
        direct_wrapped,
        PodError::Io(ref error)
            if error.kind() == io::ErrorKind::PermissionDenied && error.source().is_some()
    ));

    let exclusive_path = temp_dir.join("exclusive.pod");
    write_file_exclusive(&exclusive_path, "first").unwrap();
    assert_eq!(read_file(&exclusive_path).unwrap(), "first");
    let exists_err = write_file_exclusive(&exclusive_path, "second").unwrap_err();
    assert!(matches!(
        exists_err,
        PodError::Io(ref error) if error.kind() == io::ErrorKind::AlreadyExists
    ));
    assert_eq!(read_file(&exclusive_path).unwrap(), "first");

    let dangling_target = temp_dir.join("dangling_target.conf");
    let dangling_link = temp_dir.join("dangling.conf");
    unix_fs::symlink(&dangling_target, &dangling_link).unwrap();
    let dangling_excl_err = write_file_exclusive(&dangling_link, "evil").unwrap_err();
    assert!(matches!(
        dangling_excl_err,
        PodError::Io(ref error) if error.kind() == io::ErrorKind::AlreadyExists
    ));
    assert!(!file_exists(&dangling_target));
    assert!(write_file(&dangling_link, "overwrite").is_err());
    assert!(write_file_nofollow(&dangling_link, "overwrite").is_err());
    assert!(!file_exists(&dangling_target));

    let protected_target = temp_dir.join("protected.conf");
    write_file(&protected_target, "safe").unwrap();
    let active_link = temp_dir.join("active_link.conf");
    unix_fs::symlink(&protected_target, &active_link).unwrap();
    assert!(ensure_writable_target(&active_link).is_err());
    assert!(write_file(&active_link, "overwrite").is_err());
    assert!(write_file_nofollow(&active_link, "overwrite").is_err());
    assert!(matches!(
        write_file_exclusive(&active_link, "overwrite"),
        Err(PodError::Io(ref error)) if error.kind() == io::ErrorKind::AlreadyExists
    ));
    assert_eq!(read_file(&protected_target).unwrap(), "safe");

    for invalid in [
        Path::new(""),
        Path::new("."),
        Path::new(".."),
        &temp_dir,
        &temp_dir.join("missing_parent/pods.conf"),
        &temp_dir.join("new_target.conf/"),
        &temp_dir.join("new_target.conf/."),
        &temp_dir.join("new_target.conf/.."),
    ] {
        assert!(ensure_writable_target(invalid).is_err());
    }
    assert!(write_file_exclusive(Path::new(""), "x").is_err());
    assert!(ensure_writable_target(&temp_dir.join("new_target.conf")).is_ok());

    let write_only = temp_dir.join("write_only.conf");
    write_file(&write_only, "secret").unwrap();
    fs::set_permissions(&write_only, fs::Permissions::from_mode(0o200)).unwrap();
    assert!(file_exists(&write_only));
    fs::set_permissions(&write_only, fs::Permissions::from_mode(0o600)).unwrap();

    let fifo_path = temp_dir.join("nonblocking.fifo");
    let status = Command::new("mkfifo").arg(&fifo_path).status().unwrap();
    assert!(status.success(), "mkfifo must succeed in temp dir");
    let fifo_err = read_file(&fifo_path).unwrap_err();
    assert!(matches!(
        fifo_err,
        PodError::Io(ref error) if error.kind() == io::ErrorKind::InvalidInput
    ));

    assert!(read_file(Path::new("/dev/null")).is_err());
    assert!(read_file(Path::new("/dev/zero")).is_err());
    assert!(read_file(&temp_dir).is_err());

    let oversized_path = temp_dir.join("oversized.conf");
    write_file(&oversized_path, &"a".repeat(MAX_FILE_BYTES + 1)).unwrap();
    assert!(matches!(
        read_file(&oversized_path),
        Err(PodError::Psl(ref error)) if error.kind == PslErrorKind::LimitExceeded
    ));

    let mut straddling_utf8 = vec![b'a'; MAX_FILE_BYTES];
    straddling_utf8.extend_from_slice("ä".as_bytes());
    assert!(matches!(
        read_bounded_utf8(
            Path::new("straddling.pod"),
            Cursor::new(straddling_utf8),
            INITIAL_FILE_CAPACITY
        ),
        Err(PodError::Psl(ref error)) if error.kind == PslErrorKind::LimitExceeded
    ));
    let bounded_invalid_err = read_bounded_utf8(
        Path::new("invalid.pod"),
        Cursor::new(vec![0xFF, 0xFE]),
        INITIAL_FILE_CAPACITY,
    )
    .unwrap_err();
    assert!(matches!(
        bounded_invalid_err,
        PodError::Io(ref error) if error.kind() == io::ErrorKind::InvalidData
    ));
    assert!(bounded_invalid_err.to_string().contains("invalid.pod"));

    let invalid_utf8_path = temp_dir.join("invalid_utf8.pod");
    fs::write(&invalid_utf8_path, [0xFF, 0xFE]).unwrap();
    let invalid_utf8_err = read_file(&invalid_utf8_path).unwrap_err();
    assert!(matches!(
        invalid_utf8_err,
        PodError::Io(ref error) if error.kind() == io::ErrorKind::InvalidData
    ));
    assert!(
        invalid_utf8_err
            .to_string()
            .contains(invalid_utf8_path.to_str().unwrap()),
        "read_file invalid UTF-8 error must include file path, got: {invalid_utf8_err}"
    );
}

#[test]
fn test_temp_dir_cleans_up_on_drop_and_panic() {
    let mut leaked_path = PathBuf::new();
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let dir = TestTempDir::new("fs_panic_cleanup");
        leaked_path = dir.to_path_buf();
        write_file(&dir.join("sample.txt"), "data").unwrap();
        assert!(directory_exists(&dir));
        panic!("intentional panic to verify RAII cleanup");
    }));
    assert!(result.is_err());
    assert!(!leaked_path.exists());
}
