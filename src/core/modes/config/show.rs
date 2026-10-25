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

//! Handler for `pods config show`.

use std::io::{self, Write};

use crate::core::config::Config;
use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;

/// Reads the active `pods.conf` file and outputs its raw contents prefixed with 1-based line
/// numbers.
///
/// # Workflow
///
/// 1. Read the raw contents of `context.config.path` via [`Config::read_file`].
/// 2. Print each line prefixed with its 1-based line number via [`log::write_numbered_lines`].
///
/// # Errors
///
/// Returns [`PodError`] if reading the configuration file from disk fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    show_with(&context.config, &mut io::stdout().lock())
}

/// Reads `config.path` from disk and writes numbered lines to `stdout`.
fn show_with<W: Write>(config: &Config, stdout: &mut W) -> Result<(), PodError> {
    let content = Config::read_file(&config.path)?;
    log::write_numbered_lines(stdout, &content);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs as std_fs;
    use std::path::PathBuf;
    use std::process;

    use crate::core::config::Config;
    use crate::core::context::Context;
    use crate::error::{ConfigErrorKind, PodError};
    use crate::io::system::fs;
    use super::{handle, show_with};

    fn create_temp_test_dir(suffix: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "podoxide_config_show_{}_{suffix}",
            process::id()
        ));
        let _ = std_fs::remove_dir_all(&dir);
        std_fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn show_with_outputs_numbered_lines_and_rejects_missing_file() {
        let temp_dir = create_temp_test_dir("show_lines");
        let config_path = temp_dir.join("pods.conf");
        fs::write_file(
            &config_path,
            "general {\n    editor vim\n    simulate false\n}\n",
        )
        .unwrap();

        let mut config = Config::new();
        config.path = config_path;

        let mut stdout = Vec::new();
        show_with(&config, &mut stdout).unwrap();
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "  1: general {\n  2:     editor vim\n  3:     simulate false\n  4: }\n"
        );

        let mut context = Context::new();
        context.config = config;
        assert!(handle(&context).is_ok());

        let mut missing_config = Config::new();
        missing_config.path = temp_dir.join("nonexistent.conf");
        let mut error_out = Vec::new();
        let error = show_with(&missing_config, &mut error_out).unwrap_err();
        match error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::NotFound);
            }
            other => panic!("expected PodError::Config(NotFound), got {other:?}"),
        }

        let _ = std_fs::remove_dir_all(&temp_dir);
    }
}
