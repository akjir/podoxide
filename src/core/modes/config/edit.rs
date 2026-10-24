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

//! Handler for `pods config edit`.

use std::io::{self, Write};
use std::path::Path;

use crate::core::context::Context;
use crate::error::{ConfigError, ConfigErrorKind, PodError};
use crate::io::exec::command::{self, ExecOptions};
use crate::utilities::path::{
    extract_config_stem, has_parent_dir, is_valid_editor_value, sanitize_path_token,
};

/// Opens the active `pods.conf` configuration file in the configured external editor.
///
/// # Workflow
///
/// 1. Use `context.config` already loaded from `context.config.path`. Never read or parse
///    any `.pod` recipe files.
/// 2. Verify that `context.config.editor` is non-empty and structurally valid; return an error
///    if no editor is configured or the value is invalid.
/// 3. Execute `<editor> <config.path>` interactively via `src/io/exec/command.rs` (or print
///    the command without executing it when `context.config.simulate` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if `general.editor` is empty or invalid, `config.path` is invalid, or
/// spawning the editor process fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    edit_with(context, &mut stdout, &mut stderr)
}

/// Validates the configured editor and configuration file path, then executes or simulates `<editor> <config.path>`.
///
/// # Errors
///
/// Returns [`PodError`] if `context.config.editor` is missing or invalid, `context.config.path`
/// is invalid, or the editor command fails.
fn edit_with<W: Write, E: Write>(
    context: &Context,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    let editor = validate_editor(&context.config.editor)?;
    let config_path = validate_config_path(&context.config.path)?;
    command::exec_with(
        editor,
        &[config_path],
        &ExecOptions {
            interactive: true,
            prefix: "",
            simulate: context.config.simulate,
        },
        stdout,
        stderr,
    )
}

/// Verifies that `path` is a safe UTF-8 configuration file path without `..` traversal or leading dashes.
fn validate_config_path(path: &Path) -> Result<&str, PodError> {
    path.to_str()
        .filter(|raw| !has_parent_dir(path) && extract_config_stem(raw).is_some())
        .ok_or_else(|| {
            PodError::from(ConfigError::without_line(
                ConfigErrorKind::InvalidPath,
                sanitize_path_token(path),
            ))
        })
}

/// Verifies that `editor` is non-empty and structurally valid for direct process execution.
fn validate_editor(editor: &str) -> Result<&str, PodError> {
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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    use crate::core::context::Context;
    use crate::core::log;
    use crate::error::{ConfigErrorKind, PodError, SystemError};
    use super::{edit_with, handle};

    #[test]
    fn edit_with_executes_and_logs_debug_when_not_simulated() {
        let mut context = Context::new();
        context.config.editor = String::from("false");
        context.config.path = PathBuf::from("./tests/pods/configs/config_012_invalid_editor.conf");
        context.config.simulate = false;

        log::set_debug_enabled(true);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let error = edit_with(&context, &mut stdout, &mut stderr).unwrap_err();
        log::set_debug_enabled(false);

        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "DEBUG: Execute: false ./tests/pods/configs/config_012_invalid_editor.conf\n"
        );
        match error {
            PodError::System(SystemError::CommandFailed(Some(1))) => {}
            other => panic!("expected CommandFailed(Some(1)), got {other:?}"),
        }

        context.config.editor = String::from("true");
        let mut ok_stdout = Vec::new();
        let mut ok_stderr = Vec::new();
        edit_with(&context, &mut ok_stdout, &mut ok_stderr).unwrap();
        assert!(ok_stdout.is_empty());
        assert!(ok_stderr.is_empty());
    }

    #[test]
    fn edit_with_rejects_missing_or_invalid_editor_and_path() {
        let mut context = Context::new();
        context.config.editor = String::new();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let missing_error = edit_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match missing_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::MissingEditor);
            }
            other => panic!("expected PodError::Config(MissingEditor), got {other:?}"),
        }

        for invalid_editor in ["-vim", "../vim", "vim -u NONE", "vim\n"] {
            context.config.editor = String::from(invalid_editor);
            let error = edit_with(&context, &mut stdout, &mut stderr).unwrap_err();
            match error {
                PodError::Config(config_error) => {
                    assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
                }
                other => panic!(
                    "expected PodError::Config(InvalidPath) for {invalid_editor:?}, got {other:?}"
                ),
            }
        }

        context.config.editor = String::from("vim");
        for invalid_path in ["", ".", "..", "-pods.conf", "../pods.conf", "./.conf"] {
            context.config.path = PathBuf::from(invalid_path);
            let error = edit_with(&context, &mut stdout, &mut stderr).unwrap_err();
            match error {
                PodError::Config(config_error) => {
                    assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
                }
                other => panic!(
                    "expected PodError::Config(InvalidPath) for {invalid_path:?}, got {other:?}"
                ),
            }
        }

        context.config.path = PathBuf::from(OsString::from_vec(vec![b'.', b'/', 0xFF, b'.', b'c']));
        let non_utf8_error = edit_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match non_utf8_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
            }
            other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
        }
    }

    #[test]
    fn edit_with_simulates_editor_command_and_escapes_paths() {
        let mut context = Context::new();
        context.config.editor = String::from("vim");
        context.config.path = PathBuf::from("./pods.conf");
        context.config.simulate = true;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        edit_with(&context, &mut stdout, &mut stderr).unwrap();
        assert_eq!(String::from_utf8(stdout).unwrap(), "vim ./pods.conf\n");
        assert!(stderr.is_empty());

        context.config.path = PathBuf::from("./my configs/O'Hara.conf");
        let mut quoted_stdout = Vec::new();
        edit_with(&context, &mut quoted_stdout, &mut stderr).unwrap();
        assert_eq!(
            String::from_utf8(quoted_stdout).unwrap(),
            "vim './my configs/O'\\''Hara.conf'\n"
        );

        context.config.path = PathBuf::from("./pods.conf");
        assert!(handle(&context).is_ok());
    }
}
