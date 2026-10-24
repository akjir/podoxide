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

//! Command-line formatting, byte size formatting, human-readable size parsing, and table column alignment helpers.

use crate::core::constants::{APP_NAME, APP_VERSION};
use crate::core::log::MAX_LOG_MESSAGE_BYTES;
use crate::utilities::string::escape_shell;

/// Default target column for status alignment in formatted output lines.
pub const DEFAULT_TARGET_COLUMN: usize = 44;
const ESTIMATED_ARG_BYTES: usize = 12;

/// Formats `program` and `arguments` into a single shell-escaped command string.
#[must_use]
pub fn format_command<S: AsRef<str>>(program: &str, arguments: &[S]) -> String {
    let escaped_program = escape_shell(program);
    let capacity = arguments
        .len()
        .saturating_mul(ESTIMATED_ARG_BYTES)
        .saturating_add(escaped_program.len())
        .min(MAX_LOG_MESSAGE_BYTES);
    let mut formatted = String::with_capacity(capacity);
    formatted.push_str(&escaped_program);
    for argument in arguments {
        formatted.push(' ');
        formatted.push_str(&escape_shell(argument.as_ref()));
    }
    formatted
}

/// Formats `line` by padding it with ASCII spaces up to `target_column` (minimum 1 space) and appending `status`.
#[must_use]
pub fn format_line(line: &str, status: &str, target_column: usize) -> String {
    let visible = line.chars().count();
    let clamped_target = target_column.min(MAX_LOG_MESSAGE_BYTES);
    let pad = clamped_target.saturating_sub(visible).max(1);
    let capacity = line
        .len()
        .saturating_add(pad)
        .saturating_add(status.len())
        .min(MAX_LOG_MESSAGE_BYTES);
    let mut formatted = String::with_capacity(capacity);
    formatted.push_str(line);
    formatted.push_str(&" ".repeat(pad));
    formatted.push_str(status);
    formatted
}

/// Formats the application version header (`Podoxide v<version>`), appending ` - <title>` when `mode_title` is non-empty.
#[must_use]
pub fn format_version_header(mode_title: Option<&str>) -> String {
    match mode_title {
        Some(title) if !title.is_empty() => format!("{APP_NAME} {APP_VERSION} - {title}"),
        _ => format!("{APP_NAME} {APP_VERSION}"),
    }
}

#[cfg(test)]
mod tests {
    use crate::core::constants::{APP_NAME, APP_VERSION};
    use crate::core::log::MAX_LOG_MESSAGE_BYTES;
    use super::{DEFAULT_TARGET_COLUMN, format_command, format_line, format_version_header};

    #[test]
    fn format_command_escapes_shell_arguments() {
        assert_eq!(
            format_command("podman", &["run", "--name", "app", "O'Hara", "Hancock 2"]),
            "podman run --name app 'O'\\''Hara' 'Hancock 2'"
        );
        assert_eq!(format_command("", &["--version"]), "'' --version");
    }

    #[test]
    fn format_line_aligns_ascii_utf8_and_clamps_extreme_bounds() {
        assert_eq!(
            format_line("short_line", "[OK]", DEFAULT_TARGET_COLUMN),
            "short_line                                  [OK]"
        );
        assert_eq!(
            format_line("short_line", "[OK]", 20),
            "short_line          [OK]"
        );
        assert_eq!(
            format_line("this_is_a_very_long_line", "[OK]", 10),
            "this_is_a_very_long_line [OK]"
        );
        assert_eq!(format_line("überlänge", "[OK]", 14), "überlänge     [OK]");
        assert_eq!(format_line("  • group", "[OK]", 12), "  • group   [OK]");

        let clamped = format_line("x", "[OK]", usize::MAX);
        assert_eq!(clamped.chars().count(), MAX_LOG_MESSAGE_BYTES + 4);
    }

    #[test]
    fn format_version_header_formats_default_and_mode_headers() {
        let expected_base = format!("{APP_NAME} {APP_VERSION}");
        assert_eq!(format_version_header(None), expected_base);
        assert_eq!(format_version_header(Some("")), expected_base);
        assert_eq!(
            format_version_header(Some("Command Mode")),
            format!("{expected_base} - Command Mode")
        );
    }
}
