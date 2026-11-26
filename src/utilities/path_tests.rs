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

//! Unit tests for path normalization, extension matching, and validation helpers in [`super`].

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use crate::error::MAX_ERROR_TOKEN_BYTES;
use super::{
    build_full_path, has_parent_dir, has_regular_target_name, is_disallowed_editor_value,
    is_valid_directory, is_valid_directory_path, is_valid_editor_value, is_valid_target_path,
    is_valid_volume_options, matches_extension, normalize_lexical_path, parent_or_cur_dir,
    pop_relative_or_absolute, relative_display_path, sanitize_path_token,
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
    assert_eq!(
        build_full_path(Path::new("./recipes"), "/etc/passwd", ".pod"),
        PathBuf::from("./recipes/etc/passwd.pod")
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
    assert!(!is_valid_editor_value("./evil"));
    assert!(!is_valid_editor_value("sub/vim"));
    assert!(!is_valid_editor_value(""));
    assert!(!is_valid_editor_value("-vim"));
    assert!(!is_valid_editor_value("/usr/bin/-vim"));
    assert!(!is_valid_editor_value("../vim"));
    assert!(!is_valid_editor_value("/usr/bin/"));
    assert!(!is_valid_editor_value("vim -u NONE"));
    assert!(!is_valid_editor_value("podman"));
    assert!(!is_valid_editor_value("/usr/bin/podman"));
    assert!(!is_valid_editor_value("/opt/v1..2/podman"));
    assert!(is_disallowed_editor_value("podman"));
    assert!(is_disallowed_editor_value("/usr/bin/podman"));
    assert!(is_disallowed_editor_value("/opt/v1..2/podman"));
    assert!(!is_disallowed_editor_value("../podman"));
    assert!(!is_disallowed_editor_value("vim"));

    assert!(is_valid_volume_options("ro,Z"));
    assert!(!is_valid_volume_options(""));
    assert!(!is_valid_volume_options(","));
    assert!(!is_valid_volume_options("ro,"));
    assert!(!is_valid_volume_options(",ro"));
    assert!(!is_valid_volume_options("ro,,Z"));
    assert!(!is_valid_volume_options("-ro"));
    assert!(!is_valid_volume_options("ro:z"));
    assert!(!is_valid_volume_options("ro/rw"));
    assert!(!is_valid_volume_options(".."));
    assert!(!is_valid_volume_options("ro z"));

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
    assert_eq!(
        normalize_lexical_path(Path::new("./a/..")),
        PathBuf::from(".")
    );
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
    assert!(is_valid_target_path(Path::new(".pod.conf")));
    assert!(!is_valid_target_path(Path::new("+!id.conf")));
    assert!(!is_valid_target_path(Path::new("+q.pod")));
    assert!(!is_valid_target_path(Path::new("./+q.pod")));
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
    assert_eq!(
        relative_display_path(Path::new("./a/b"), Path::new("a")),
        Path::new("b")
    );
    assert_eq!(
        relative_display_path(Path::new("a/b"), Path::new("./a")),
        Path::new("b")
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
