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

//! Handler for `pods init` (template creation).

use std::io::{self, Write};
use std::path::Path;

use crate::core::config::DEFAULT_CONFIG_PATH;
use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;
use crate::io::system::fs;
use crate::utilities::path::{extract_config_stem, has_parent_dir};

const CONFIG_TEMPLATE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/pods.conf"));
const DEFAULT_RECIPE_PATH: &str = "./recipe.pod";
const RECIPE_TEMPLATE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/recipe.pod"));

/// Verifies that target files or directories do not exist and writes the root `recipe.pod`
/// and `pods.conf` templates to disk.
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any existing `.pod` files from disk; use the embedded
///    compile-time template strings (`recipe.pod` and `pods.conf`).
/// 2. Check whether `./recipe.pod` or `context.config.path` (`./pods.conf` or custom
///    `--config` path) already exists on disk as a file or directory; abort with an error
///    before writing if either path already exists.
/// 3. Write `./recipe.pod` and log `"Created './recipe.pod'."` via [`log::write_info`].
/// 4. Write `context.config.path` and log `"Created '<config.path>'."` via [`log::write_info`].
///
/// # Errors
///
/// Returns [`PodError`] if either target path already exists as a file or directory, if a
/// target path is invalid, or if writing either template file fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let recipe_path = Path::new(DEFAULT_RECIPE_PATH);
    let config_path = resolve_config_target(&context.config.path);
    create_files(recipe_path, config_path, &mut io::stderr().lock())
}

/// Validates availability of `recipe_path` and `config_path`, writes both embedded templates, and logs creation to `stderr_writer`.
fn create_files<W: Write>(
    recipe_path: &Path,
    config_path: &Path,
    stderr_writer: &mut W,
) -> Result<(), PodError> {
    ensure_target_available(recipe_path)?;
    ensure_target_available(config_path)?;
    ensure_distinct_targets(recipe_path, config_path)?;
    write_template_file(recipe_path, RECIPE_TEMPLATE, stderr_writer)?;
    write_template_file(config_path, CONFIG_TEMPLATE, stderr_writer)?;
    Ok(())
}

/// Verifies that `recipe_path` and `config_path` do not resolve to the same filesystem location.
fn ensure_distinct_targets(recipe_path: &Path, config_path: &Path) -> Result<(), PodError> {
    if fs::get_absolute_path(recipe_path) == fs::get_absolute_path(config_path) {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Target paths '{}' and '{}' must not point to the same file!",
                recipe_path.display(),
                config_path.display()
            ),
        )));
    }
    Ok(())
}

/// Verifies that `path` is a safe target path and does not already exist as a directory, file, or symlink.
fn ensure_target_available(path: &Path) -> Result<(), PodError> {
    if !is_valid_target_path(path) {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Invalid target path '{}'!", path.display()),
        )));
    }
    if fs::directory_exists(path) {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("Directory '{}' already exists!", path.display()),
        )));
    }
    if fs::file_exists(path) {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("File '{}' already exists!", path.display()),
        )));
    }
    fs::ensure_writable_target(path)
}

/// Evaluates whether `path` is a structurally valid, UTF-8 target file path without `..` traversal.
fn is_valid_target_path(path: &Path) -> bool {
    let Some(raw_path) = path.to_str() else {
        return false;
    };
    !has_parent_dir(path) && extract_config_stem(raw_path).is_some()
}

/// Returns `path` if non-empty, or falls back to [`DEFAULT_CONFIG_PATH`] (`"./pods.conf"`).
fn resolve_config_target(path: &Path) -> &Path {
    if path.as_os_str().is_empty() {
        return Path::new(DEFAULT_CONFIG_PATH);
    }
    path
}

/// Writes `content` to `path` and logs the created file path to `stderr_writer`.
fn write_template_file<W: Write>(
    path: &Path,
    content: &str,
    stderr_writer: &mut W,
) -> Result<(), PodError> {
    fs::write_file(path, content)?;
    log::write_info(stderr_writer, &format!("Created '{}'.", path.display()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::ffi::OsString;
    use std::fs as std_fs;
    use std::io;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::{self as unix_fs, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::process;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::Context;
    use crate::error::PodError;
    use crate::io::psl::parse_psl;
    use crate::io::system::fs;
    use super::{
        CONFIG_TEMPLATE, DEFAULT_RECIPE_PATH, RECIPE_TEMPLATE, create_files,
        ensure_target_available, handle, resolve_config_target,
    };

    fn create_temp_test_dir(suffix: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("podoxide_init_test_{}_{suffix}", process::id()));
        let _ = std_fs::remove_dir_all(&dir);
        std_fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn create_files_rejects_existing_files_and_directories() {
        let temp_dir = create_temp_test_dir("existing");
        let recipe_path = temp_dir.join("recipe.pod");
        let config_path = temp_dir.join("pods.conf");

        // 1. Existing recipe.pod file
        fs::write_file(&recipe_path, "existing recipe").unwrap();
        let mut stderr = Vec::new();
        let recipe_file_err = create_files(&recipe_path, &config_path, &mut stderr).unwrap_err();
        match &recipe_file_err {
            PodError::Io(error) => assert_eq!(error.kind(), io::ErrorKind::AlreadyExists),
            other => panic!("expected PodError::Io(AlreadyExists), got {other:?}"),
        }
        assert!(
            recipe_file_err
                .to_string()
                .contains(&format!("File '{}' already exists!", recipe_path.display()))
        );
        assert_eq!(fs::read_file(&recipe_path).unwrap(), "existing recipe");
        assert!(!fs::file_exists(&config_path));
        assert!(stderr.is_empty());
        std_fs::remove_file(&recipe_path).unwrap();

        // 2. Existing pods.conf file (verify recipe.pod is not created)
        fs::write_file(&config_path, "existing config").unwrap();
        let config_file_err = create_files(&recipe_path, &config_path, &mut stderr).unwrap_err();
        match &config_file_err {
            PodError::Io(error) => assert_eq!(error.kind(), io::ErrorKind::AlreadyExists),
            other => panic!("expected PodError::Io(AlreadyExists), got {other:?}"),
        }
        assert!(
            config_file_err
                .to_string()
                .contains(&format!("File '{}' already exists!", config_path.display()))
        );
        assert!(!fs::file_exists(&recipe_path));
        assert_eq!(fs::read_file(&config_path).unwrap(), "existing config");
        assert!(stderr.is_empty());
        std_fs::remove_file(&config_path).unwrap();

        // 3. Existing recipe.pod directory/folder
        std_fs::create_dir_all(&recipe_path).unwrap();
        let recipe_dir_err = create_files(&recipe_path, &config_path, &mut stderr).unwrap_err();
        match &recipe_dir_err {
            PodError::Io(error) => assert_eq!(error.kind(), io::ErrorKind::AlreadyExists),
            other => panic!("expected PodError::Io(AlreadyExists), got {other:?}"),
        }
        assert!(
            recipe_dir_err.to_string().contains(&format!(
                "Directory '{}' already exists!",
                recipe_path.display()
            ))
        );
        assert!(!fs::file_exists(&config_path));
        assert!(stderr.is_empty());
        std_fs::remove_dir_all(&recipe_path).unwrap();

        // 4. Existing pods.conf directory/folder (verify recipe.pod is not created)
        std_fs::create_dir_all(&config_path).unwrap();
        let config_dir_err = create_files(&recipe_path, &config_path, &mut stderr).unwrap_err();
        match &config_dir_err {
            PodError::Io(error) => assert_eq!(error.kind(), io::ErrorKind::AlreadyExists),
            other => panic!("expected PodError::Io(AlreadyExists), got {other:?}"),
        }
        assert!(
            config_dir_err.to_string().contains(&format!(
                "Directory '{}' already exists!",
                config_path.display()
            ))
        );
        assert!(!fs::file_exists(&recipe_path));
        assert!(stderr.is_empty());

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn create_files_writes_valid_root_templates_and_logs_info() {
        let temp_dir = create_temp_test_dir("templates");
        let recipe_path = temp_dir.join("recipe.pod");
        let config_path = temp_dir.join("pods.conf");
        let mut stderr = Vec::new();

        create_files(&recipe_path, &config_path, &mut stderr).unwrap();

        let log_output = String::from_utf8(stderr).unwrap();
        assert!(log_output.contains(&format!("INFO: Created '{}'.\n", recipe_path.display())));
        assert!(log_output.contains(&format!("INFO: Created '{}'.\n", config_path.display())));

        let written_recipe = fs::read_file(&recipe_path).unwrap();
        let written_config = fs::read_file(&config_path).unwrap();
        let root_recipe = fs::read_file(Path::new(DEFAULT_RECIPE_PATH)).unwrap();
        let root_config = fs::read_file(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        assert_eq!(written_recipe, RECIPE_TEMPLATE);
        assert_eq!(written_recipe, root_recipe);
        assert_eq!(written_config, CONFIG_TEMPLATE);
        assert_eq!(written_config, root_config);

        let loaded_config = Config::load(&config_path).unwrap();
        assert_eq!(loaded_config.editor, "vim");
        assert!(loaded_config.simulate);
        assert_eq!(
            loaded_config.recipes.groups.get("all"),
            Some(&vec![String::from("recipe")])
        );

        let parsed_recipe = parse_psl(&written_recipe).unwrap();
        assert_eq!(parsed_recipe.blocks.len(), 3);
        assert_eq!(parsed_recipe.blocks[0].name, "pod");
        assert_eq!(parsed_recipe.blocks[1].name, "container");
        assert_eq!(parsed_recipe.blocks[2].name, "container");

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn handle_and_path_edge_cases_and_symlink_protection() {
        let context = Context::new();
        let handle_err = handle(&context).unwrap_err();
        assert!(
            handle_err
                .to_string()
                .contains("File './recipe.pod' already exists!")
        );

        assert_eq!(
            resolve_config_target(Path::new("")),
            Path::new(DEFAULT_CONFIG_PATH)
        );
        assert_eq!(
            resolve_config_target(Path::new("./staging.conf")),
            Path::new("./staging.conf")
        );

        assert!(ensure_target_available(Path::new("")).is_err());
        assert!(ensure_target_available(Path::new("../pods.conf")).is_err());
        assert!(ensure_target_available(Path::new("/tmp/../pods.conf")).is_err());
        for malformed in [
            "./pods.conf/",
            "./-pods.conf",
            "./.conf",
            "./..conf",
            "./pods\n.conf",
            "./pods\u{202e}.conf",
        ] {
            assert!(
                ensure_target_available(Path::new(malformed)).is_err(),
                "expected error for malformed path {malformed:?}"
            );
        }
        let non_utf8_path = PathBuf::from(OsString::from_vec(vec![b'.', b'/', 0xFF, b'.', b'c']));
        assert!(ensure_target_available(&non_utf8_path).is_err());

        let temp_dir = create_temp_test_dir("symlinks");
        let dangling_target = temp_dir.join("missing_target.pod");
        let dangling_recipe = temp_dir.join("recipe.pod");
        let config_path = temp_dir.join("pods.conf");
        unix_fs::symlink(&dangling_target, &dangling_recipe).unwrap();

        let mut stderr = Vec::new();
        assert!(create_files(&dangling_recipe, &config_path, &mut stderr).is_err());
        assert!(!fs::file_exists(&dangling_target));
        assert!(!fs::file_exists(&config_path));
        assert!(stderr.is_empty());
        std_fs::remove_file(&dangling_recipe).unwrap();

        // Dangling symlink on config_path must not leave recipe.pod orphaned
        let dangling_config_target = temp_dir.join("missing_target.conf");
        unix_fs::symlink(&dangling_config_target, &config_path).unwrap();
        assert!(create_files(&dangling_recipe, &config_path, &mut stderr).is_err());
        assert!(!fs::file_exists(&dangling_recipe));
        assert!(!fs::file_exists(&dangling_config_target));
        assert!(stderr.is_empty());
        std_fs::remove_file(&config_path).unwrap();

        // Missing parent directory on config_path must not leave recipe.pod orphaned
        let missing_parent_config = temp_dir.join("missing_dir").join("pods.conf");
        assert!(create_files(&dangling_recipe, &missing_parent_config, &mut stderr).is_err());
        assert!(!fs::file_exists(&dangling_recipe));
        assert!(stderr.is_empty());

        // Colliding target paths must be rejected before writing
        let dot_recipe = temp_dir.join("./recipe.pod");
        assert!(create_files(&dangling_recipe, &dangling_recipe, &mut stderr).is_err());
        assert!(create_files(&dangling_recipe, &dot_recipe, &mut stderr).is_err());
        assert!(!fs::file_exists(&dangling_recipe));
        assert!(stderr.is_empty());

        // Write-only existing file must be rejected as AlreadyExists without overwriting
        fs::write_file(&config_path, "protected write-only").unwrap();
        std_fs::set_permissions(&config_path, std_fs::Permissions::from_mode(0o200)).unwrap();
        let write_only_err = create_files(&dangling_recipe, &config_path, &mut stderr).unwrap_err();
        match &write_only_err {
            PodError::Io(error) => assert_eq!(error.kind(), io::ErrorKind::AlreadyExists),
            other => panic!("expected PodError::Io(AlreadyExists), got {other:?}"),
        }
        assert!(!fs::file_exists(&dangling_recipe));
        assert!(stderr.is_empty());
        std_fs::set_permissions(&config_path, std_fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(fs::read_file(&config_path).unwrap(), "protected write-only");

        let _ = std_fs::remove_dir_all(&temp_dir);
    }
}
