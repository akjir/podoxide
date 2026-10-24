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

use crate::core::log::MAX_LOG_MESSAGE_BYTES;
use crate::utilities::string::escape_shell;

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

#[cfg(test)]
mod tests {
    use super::format_command;

    #[test]
    fn format_command_escapes_shell_arguments() {
        assert_eq!(
            format_command("podman", &["run", "--name", "app", "O'Hara", "Hancock 2"]),
            "podman run --name app 'O'\\''Hara' 'Hancock 2'"
        );
        assert_eq!(format_command("", &["--version"]), "'' --version");
    }
}
