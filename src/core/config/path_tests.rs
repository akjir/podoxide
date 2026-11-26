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

//! Unit tests for configuration file loading, path resolution, and editor validation in [`super`].

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use crate::error::{ConfigErrorKind, PodError};
use crate::io::system::fs::{self as system_fs, TestTempDir};
use super::{
    Config, DEFAULT_CONFIG_NAME, DEFAULT_CONFIG_PATH, DEFAULT_RECIPES_PATH, extract_config_stem,
    resolve_config_name, resolve_config_path, validate_editor,
};

#[test]
fn extract_config_stem_resolve_config_and_validate_editor() {
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
    assert!(extract_config_stem(".pod.conf").is_some());

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

    assert_eq!(validate_editor("vim").unwrap(), "vim");
    assert!(matches!(
        validate_editor("   "),
        Err(PodError::Config(error)) if error.kind == ConfigErrorKind::MissingEditor
    ));
    assert!(matches!(
        validate_editor("-vim"),
        Err(PodError::Config(error)) if error.kind == ConfigErrorKind::InvalidPath
    ));
    assert!(matches!(
        validate_editor("podman"),
        Err(PodError::Config(error)) if error.kind == ConfigErrorKind::InvalidPath
    ));

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
fn load_and_read_file_handle_root_temp_missing_and_invalid_paths() {
    let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();
    assert_eq!(root_config.name, DEFAULT_CONFIG_NAME);
    assert_eq!(root_config.path, PathBuf::from(DEFAULT_CONFIG_PATH));
    assert_eq!(root_config.editor, "vim");
    assert!(root_config.simulate);
    assert_eq!(root_config.pods.path, PathBuf::from("./pods"));
    assert_eq!(
        root_config.recipes.path,
        PathBuf::from(DEFAULT_RECIPES_PATH)
    );
    assert_eq!(
        root_config.recipes.groups.get("all"),
        Some(&vec![String::from("recipe")])
    );
    assert!(
        Config::read_file(Path::new(DEFAULT_CONFIG_PATH))
            .unwrap()
            .contains("recipes {")
    );

    let temp_dir = TestTempDir::new("config_test");

    let staging_path = temp_dir.join("staging.conf");
    let staging_content = "\
general {
    editor nano
    simulate false
}
pods {
    path /var/lib/pods
}
recipes {
    path ./recipes
    groups {
        backend db-server cache-server
        all @backend web-frontend
    }
}
";
    system_fs::write_file(&staging_path, staging_content).unwrap();

    let loaded = Config::load(&staging_path).unwrap();
    assert_eq!(loaded.name, "staging");
    assert_eq!(loaded.path, staging_path);
    assert_eq!(loaded.editor, "nano");
    assert!(!loaded.simulate);
    assert_eq!(loaded.pods.path, PathBuf::from("/var/lib/pods"));
    assert_eq!(loaded.recipes.path, PathBuf::from("./recipes"));

    let missing_path = temp_dir.join("missing.conf");
    let missing_error = Config::load(&missing_path).unwrap_err();
    assert!(matches!(
        missing_error,
        PodError::Config(ref error)
            if error.kind == ConfigErrorKind::NotFound && error.line.is_none()
    ));
    assert!(
        missing_error
            .to_string()
            .contains("Config error: configuration file not found:")
    );
    assert!(matches!(
        Config::read_file(&missing_path),
        Err(PodError::Config(error)) if error.kind == ConfigErrorKind::NotFound
    ));

    let directory_config_path = temp_dir.join("directory.conf");
    fs::create_dir_all(&directory_config_path).unwrap();
    assert!(matches!(
        Config::load(&directory_config_path),
        Err(PodError::Io(_))
    ));

    for invalid_path in [
        "",
        ".",
        "..",
        "../secret.conf",
        "/tmp/../secret.conf",
        "-flag.conf",
        "/tmp/-flag.conf",
        ".conf",
        "/tmp/.conf",
        "..conf",
        "/tmp/pods.txt",
        "/tmp/pods.conf/",
        " /tmp/pods.conf",
        "/tmp/pods.conf\n",
        "/tmp/\u{202E}pods.conf",
    ] {
        assert!(matches!(
            Config::read_file(Path::new(invalid_path)),
            Err(PodError::Config(error)) if error.kind == ConfigErrorKind::InvalidPath
        ));
    }

    let invalid_utf8 = OsString::from_vec(vec![
        b'/', b't', b'm', b'p', b'/', 0xff, 0xfe, b'.', b'c', b'o', b'n', b'f',
    ]);
    assert!(matches!(
        Config::read_file(Path::new(&invalid_utf8)),
        Err(PodError::Config(error)) if error.kind == ConfigErrorKind::InvalidPath
    ));
}
