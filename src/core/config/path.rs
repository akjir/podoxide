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

//! Configuration file path resolution, stem extraction, editor validation, and validated file reading.

use std::io;
use std::path::{Path, PathBuf};

use crate::error::{ConfigError, ConfigErrorKind, PodError};
use crate::io::system::fs;
use crate::utilities::path as path_util;
use super::{DEFAULT_CONFIG_NAME, DEFAULT_CONFIG_PATH};

/// Extracts the configuration file stem from a raw `.conf` path string if it is structurally valid.
#[must_use]
pub fn extract_config_stem(raw_value: &str) -> Option<&str> {
    let file_name = path_util::validate_raw_target_file_name(raw_value)?;
    let stem = file_name.strip_suffix(".conf").unwrap_or(file_name);
    path_util::has_valid_stem(stem).then_some(stem)
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
    if !path_util::is_valid_editor_value(editor) {
        return Err(PodError::from(ConfigError::without_line(
            ConfigErrorKind::InvalidPath,
            editor,
        )));
    }
    Ok(editor)
}

/// Reads the raw file at `path`, mapping a missing file to [`ConfigErrorKind::NotFound`].
pub(super) fn read_validated_file(path: &Path) -> Result<String, PodError> {
    match fs::read_file(path) {
        Ok(content) => Ok(content),
        Err(PodError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            Err(PodError::from(ConfigError::without_line(
                ConfigErrorKind::NotFound,
                path_util::sanitize_path_token(path),
            )))
        }
        Err(error) => Err(error),
    }
}

/// Validates that `path` is a safe configuration file path and returns its configuration stem.
pub(super) fn validate_load_path(path: &Path) -> Result<&str, PodError> {
    path.to_str()
        .filter(|raw| raw.ends_with(".conf") && !path_util::has_parent_dir(path))
        .and_then(extract_config_stem)
        .ok_or_else(|| {
            PodError::from(ConfigError::without_line(
                ConfigErrorKind::InvalidPath,
                path_util::sanitize_path_token(path),
            ))
        })
}
