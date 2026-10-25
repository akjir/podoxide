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
use crate::utilities::collection::{MAX_COLLECTION_PREALLOC, bounded_capacity};
use crate::utilities::string::escape_shell;

const BYTES_PER_GB: u64 = 1_000_000_000;
const BYTES_PER_KB: u64 = 1_000;
const BYTES_PER_MB: u64 = 1_000_000;
const BYTES_PER_TB: u64 = 1_000_000_000_000;
/// Default target column for status alignment in formatted output lines.
pub const DEFAULT_TARGET_COLUMN: usize = 44;
const ESTIMATED_ARG_BYTES: usize = 12;
const MAX_SIZE_STRING_BYTES: usize = 64;
const QUOTED_ITEM_BYTES_ESTIMATE: usize = 16;

/// Formats a byte count into a human-readable decimal size string (`B`, `kB`, `MB`, or `GB`).
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    if bytes < BYTES_PER_KB {
        return format!("{bytes} B");
    }
    if bytes < BYTES_PER_MB {
        return format!("{:.1} kB", bytes as f64 / BYTES_PER_KB as f64);
    }
    if bytes < BYTES_PER_GB {
        return format!("{:.1} MB", bytes as f64 / BYTES_PER_MB as f64);
    }
    format!("{:.1} GB", bytes as f64 / BYTES_PER_GB as f64)
}

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

/// Formats a 1-based list index (`"1)"` or `" 1)"` when `total_count > 9`).
#[must_use]
pub fn format_index_prefix(index: usize, total_count: usize) -> String {
    if total_count > 9 && index < 10 {
        format!(" {index})")
    } else {
        format!("{index})")
    }
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

/// Formats a slice of items as `'item1', 'item2' and 'itemN'`.
#[must_use]
pub fn format_quoted_conjunction(items: &[&str]) -> String {
    let Some((last, prefix)) = items.split_last() else {
        return String::new();
    };
    if prefix.is_empty() {
        return format!("'{last}'");
    }
    let capacity = bounded_capacity(
        items.len().saturating_mul(QUOTED_ITEM_BYTES_ESTIMATE),
        MAX_COLLECTION_PREALLOC,
    );
    let mut output = String::with_capacity(capacity);
    for (i, item) in prefix.iter().enumerate() {
        if i > 0 {
            output.push_str(", ");
        }
        output.push('\'');
        output.push_str(item);
        output.push('\'');
    }
    output.push_str(" and '");
    output.push_str(last);
    output.push('\'');
    output
}

/// Formats the remediation hint message for one or more recipes that failed inspection.
#[must_use]
pub fn format_recipe_error_hint(invalid_recipes: &[&str]) -> String {
    match invalid_recipes {
        [] => String::new(),
        [single] => {
            format!("Recipe '{single}' has errors! Run 'pods recipe inspect {single}' for details.")
        }
        many => {
            let joined = format_quoted_conjunction(many);
            format!("Recipes {joined} have errors! Run 'pods recipe inspect <recipe>' for details.")
        }
    }
}

/// Formats the application version header (`Podoxide v<version>`), appending ` - <title>` when `mode_title` is non-empty.
#[must_use]
pub fn format_version_header(mode_title: Option<&str>) -> String {
    match mode_title {
        Some(title) if !title.is_empty() => format!("{APP_NAME} {APP_VERSION} - {title}"),
        _ => format!("{APP_NAME} {APP_VERSION}"),
    }
}

/// Parses a human-readable decimal size string (for example, `"1.5 MB"`) into a byte count.
#[must_use]
pub fn parse_size_to_bytes(size_str: &str) -> u64 {
    let trimmed = size_str.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_SIZE_STRING_BYTES {
        return 0;
    }
    let numeric_end = trimmed
        .find(|character: char| !character.is_ascii_digit() && character != '.')
        .unwrap_or(trimmed.len());
    let (number_part, remainder) = trimmed.split_at(numeric_end);
    if number_part.is_empty() {
        return 0;
    }
    let Ok(number) = number_part.parse::<f64>() else {
        return 0;
    };
    let Some(multiplier) = unit_multiplier(remainder.trim_start()) else {
        return 0;
    };
    (number * multiplier).floor() as u64
}

/// Resolves the decimal byte multiplier for an optional unit suffix (`B`, `kB`, `MB`, `GB`, `TB`).
#[must_use]
fn unit_multiplier(unit: &str) -> Option<f64> {
    if !unit.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let multiplier = match unit.as_bytes().first().map(u8::to_ascii_uppercase) {
        Some(b'K') => BYTES_PER_KB as f64,
        Some(b'M') => BYTES_PER_MB as f64,
        Some(b'G') => BYTES_PER_GB as f64,
        Some(b'T') => BYTES_PER_TB as f64,
        _ => 1.0,
    };
    Some(multiplier)
}

#[cfg(test)]
mod tests {
    use crate::core::constants::{APP_NAME, APP_VERSION};
    use crate::core::log::MAX_LOG_MESSAGE_BYTES;
    use super::{
        DEFAULT_TARGET_COLUMN, MAX_SIZE_STRING_BYTES, format_bytes, format_command,
        format_index_prefix, format_line, format_quoted_conjunction, format_recipe_error_hint,
        format_version_header, parse_size_to_bytes,
    };

    #[test]
    fn format_bytes_and_parse_size_to_bytes_handle_units_and_invalid_inputs() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_500), "1.5 kB");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
        assert_eq!(format_bytes(1_500_000_000), "1.5 GB");

        assert_eq!(parse_size_to_bytes("500 B"), 500);
        assert_eq!(parse_size_to_bytes("1.5 kB"), 1_500);
        assert_eq!(parse_size_to_bytes("2 MB"), 2_000_000);
        assert_eq!(parse_size_to_bytes("1.25 GB"), 1_250_000_000);
        assert_eq!(parse_size_to_bytes("1.5 TB"), 1_500_000_000_000);
        assert_eq!(parse_size_to_bytes("1024"), 1_024);
        assert_eq!(parse_size_to_bytes("  10.5MB  "), 10_500_000);
        assert_eq!(parse_size_to_bytes("100 bytes"), 100);

        for invalid in [
            "",
            "   ",
            "MB",
            ".",
            "1.2.3 MB",
            "-5 MB",
            "10 MB extra",
            "10 M2",
            &"9".repeat(MAX_SIZE_STRING_BYTES + 1),
        ] {
            assert_eq!(
                parse_size_to_bytes(invalid),
                0,
                "expected 0 for {invalid:?}"
            );
        }
    }

    #[test]
    fn format_command_escapes_shell_arguments() {
        assert_eq!(
            format_command("podman", &["run", "--name", "app", "O'Hara", "Hancock 2"]),
            "podman run --name app 'O'\\''Hara' 'Hancock 2'"
        );
        assert_eq!(format_command("", &["--version"]), "'' --version");
        assert_eq!(format_index_prefix(1, 5), "1)");
        assert_eq!(format_index_prefix(1, 10), " 1)");
        assert_eq!(format_index_prefix(10, 10), "10)");
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
    fn format_quoted_conjunction_and_recipe_error_hint_handle_empty_single_and_multiple_items() {
        assert_eq!(format_quoted_conjunction(&[]), "");
        assert_eq!(format_quoted_conjunction(&["only"]), "'only'");
        assert_eq!(
            format_quoted_conjunction(&["first", "second"]),
            "'first' and 'second'"
        );
        assert_eq!(
            format_quoted_conjunction(&["first", "second", "third"]),
            "'first', 'second' and 'third'"
        );

        assert_eq!(format_recipe_error_hint(&[]), "");
        assert_eq!(
            format_recipe_error_hint(&["nginx"]),
            "Recipe 'nginx' has errors! Run 'pods recipe inspect nginx' for details."
        );
        assert_eq!(
            format_recipe_error_hint(&["buci", "nginx"]),
            "Recipes 'buci' and 'nginx' have errors! Run 'pods recipe inspect <recipe>' for details."
        );
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
