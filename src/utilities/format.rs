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

use std::borrow::Cow;
use std::iter;

use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::string;

const BYTES_PER_GB: u64 = 1_000_000_000;
const BYTES_PER_GIB: u64 = 1_073_741_824;
const BYTES_PER_KB: u64 = 1_000;
const BYTES_PER_KIB: u64 = 1_024;
const BYTES_PER_MB: u64 = 1_000_000;
const BYTES_PER_MIB: u64 = 1_048_576;
const BYTES_PER_PB: u64 = 1_000_000_000_000_000;
const BYTES_PER_PIB: u64 = 1_125_899_906_842_624;
const BYTES_PER_TB: u64 = 1_000_000_000_000;
const BYTES_PER_TIB: u64 = 1_099_511_627_776;
/// Default target column for status alignment in formatted output lines.
pub const DEFAULT_TARGET_COLUMN: usize = 44;
const ESTIMATED_ARG_BYTES: usize = 12;
/// Maximum byte length bound for formatted command and status alignment lines.
pub const MAX_FORMATTED_LINE_BYTES: usize = 4096;
const MAX_SIZE_STRING_BYTES: usize = 64;
const QUOTED_ITEM_BYTES_ESTIMATE: usize = 16;
const SIZE_UNITS: &[(u64, &str)] = &[
    (BYTES_PER_KB, "kB"),
    (BYTES_PER_MB, "MB"),
    (BYTES_PER_GB, "GB"),
    (BYTES_PER_TB, "TB"),
    (BYTES_PER_PB, "PB"),
];

/// Formats a byte count into a human-readable decimal size string (`B`, `kB`, `MB`, `GB`, `TB`, or `PB`).
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    if bytes < BYTES_PER_KB {
        return format!("{bytes} B");
    }
    for &(scale, label) in SIZE_UNITS {
        let tenths = ((bytes as f64 / scale as f64) * 10.0).round() as u64;
        if tenths < 10_000 || scale == BYTES_PER_PB {
            let whole = tenths / 10;
            let frac = tenths % 10;
            return format!("{whole}.{frac} {label}");
        }
    }
    format!("{bytes} B")
}

/// Formats `program` and `arguments` into a single shell-escaped command string.
#[must_use]
pub fn format_command<S: AsRef<str>>(program: &str, arguments: &[S]) -> String {
    let escaped_program = escape_shell_program(program);
    let capacity = arguments
        .len()
        .saturating_mul(ESTIMATED_ARG_BYTES)
        .saturating_add(escaped_program.len())
        .min(MAX_FORMATTED_LINE_BYTES);
    let mut formatted = String::with_capacity(capacity);
    formatted.push_str(&escaped_program);
    for argument in arguments {
        formatted.push(' ');
        formatted.push_str(&string::escape_shell(argument.as_ref()));
    }
    formatted
}

/// Formats a 1-based list index right-aligned to the digit width of `total_count` (for example, `"  1)"` when `total_count == 100`).
#[must_use]
pub fn format_index_prefix(index: usize, total_count: usize) -> String {
    let width = total_count.checked_ilog10().unwrap_or(0) as usize + 1;
    format!("{index:>width$})")
}

/// Formats `line` by padding it with ASCII spaces up to `target_column` (minimum 1 space) and appending `status`.
#[must_use]
pub fn format_line(line: &str, status: &str, target_column: usize) -> String {
    let visible = line.chars().count();
    let clamped_target = target_column.min(MAX_FORMATTED_LINE_BYTES);
    let pad = clamped_target.saturating_sub(visible).max(1);
    let capacity = line
        .len()
        .saturating_add(pad)
        .saturating_add(status.len())
        .min(MAX_FORMATTED_LINE_BYTES);
    let mut formatted = String::with_capacity(capacity);
    formatted.push_str(line);
    formatted.extend(iter::repeat_n(' ', pad));
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
    let capacity = collection::bounded_capacity(
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

/// Formats an application version header (`<app_name> <app_version>`), appending ` - <title>` when `mode_title` is non-empty.
#[must_use]
pub fn format_version_header(
    app_name: &str,
    app_version: &str,
    mode_title: Option<&str>,
) -> String {
    match mode_title {
        Some(title) if !title.is_empty() => format!("{app_name} {app_version} - {title}"),
        _ => format!("{app_name} {app_version}"),
    }
}

/// Parses a human-readable decimal or IEC size string (for example, `"1.5 MB"` or `"10 KiB"`) into a byte count.
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
    let scaled = (number * multiplier).round();
    if !scaled.is_finite() || scaled < 0.0 {
        return 0;
    }
    scaled as u64
}

/// Shell-escapes `program` in command word position, quoting `=` so POSIX shells do not parse it as an environment assignment.
#[must_use]
fn escape_shell_program(program: &str) -> Cow<'_, str> {
    let escaped = string::escape_shell(program);
    if matches!(escaped, Cow::Borrowed(_)) && program.contains('=') {
        return Cow::Owned(format!("'{program}'"));
    }
    escaped
}

/// Resolves the exact byte multiplier for a decimal (`B`, `kB`, `MB`, `GB`, `TB`, `PB`) or IEC (`KiB`..`PiB`) unit suffix.
#[must_use]
fn unit_multiplier(unit: &str) -> Option<f64> {
    if unit.is_empty()
        || unit.eq_ignore_ascii_case("b")
        || unit.eq_ignore_ascii_case("byte")
        || unit.eq_ignore_ascii_case("bytes")
    {
        return Some(1.0);
    }
    if unit.eq_ignore_ascii_case("kb") {
        return Some(BYTES_PER_KB as f64);
    }
    if unit.eq_ignore_ascii_case("kib") {
        return Some(BYTES_PER_KIB as f64);
    }
    if unit.eq_ignore_ascii_case("mb") {
        return Some(BYTES_PER_MB as f64);
    }
    if unit.eq_ignore_ascii_case("mib") {
        return Some(BYTES_PER_MIB as f64);
    }
    if unit.eq_ignore_ascii_case("gb") {
        return Some(BYTES_PER_GB as f64);
    }
    if unit.eq_ignore_ascii_case("gib") {
        return Some(BYTES_PER_GIB as f64);
    }
    if unit.eq_ignore_ascii_case("tb") {
        return Some(BYTES_PER_TB as f64);
    }
    if unit.eq_ignore_ascii_case("tib") {
        return Some(BYTES_PER_TIB as f64);
    }
    if unit.eq_ignore_ascii_case("pb") {
        return Some(BYTES_PER_PB as f64);
    }
    if unit.eq_ignore_ascii_case("pib") {
        return Some(BYTES_PER_PIB as f64);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_TARGET_COLUMN, MAX_FORMATTED_LINE_BYTES, MAX_SIZE_STRING_BYTES, format_bytes,
        format_command, format_index_prefix, format_line, format_quoted_conjunction,
        format_recipe_error_hint, format_version_header, parse_size_to_bytes,
    };

    #[test]
    fn format_bytes_and_parse_size_to_bytes_handle_units_precision_and_invalid_inputs() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_500), "1.5 kB");
        assert_eq!(format_bytes(999_949), "999.9 kB");
        assert_eq!(format_bytes(999_950), "1.0 MB");
        assert_eq!(format_bytes(999_999), "1.0 MB");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
        assert_eq!(format_bytes(1_500_000_000), "1.5 GB");
        assert_eq!(format_bytes(999_999_999_999), "1.0 TB");
        assert_eq!(format_bytes(1_500_000_000_000), "1.5 TB");
        assert_eq!(format_bytes(2_500_000_000_000_000), "2.5 PB");

        assert_eq!(parse_size_to_bytes("500 B"), 500);
        assert_eq!(parse_size_to_bytes("1.5 kB"), 1_500);
        assert_eq!(parse_size_to_bytes("0.29 kB"), 290);
        assert_eq!(parse_size_to_bytes("1.15 MB"), 1_150_000);
        assert_eq!(parse_size_to_bytes("2 MB"), 2_000_000);
        assert_eq!(parse_size_to_bytes("1.25 GB"), 1_250_000_000);
        assert_eq!(parse_size_to_bytes("1.5 TB"), 1_500_000_000_000);
        assert_eq!(parse_size_to_bytes("10 TB"), 10_000_000_000_000);
        assert_eq!(parse_size_to_bytes("10 PB"), 10_000_000_000_000_000);
        assert_eq!(parse_size_to_bytes("10 KiB"), 10_240);
        assert_eq!(parse_size_to_bytes("2 MiB"), 2_097_152);
        assert_eq!(parse_size_to_bytes("1 GiB"), 1_073_741_824);
        assert_eq!(parse_size_to_bytes("1 TiB"), 1_099_511_627_776);
        assert_eq!(parse_size_to_bytes("1 PiB"), 1_125_899_906_842_624);
        assert_eq!(parse_size_to_bytes("1024"), 1_024);
        assert_eq!(parse_size_to_bytes("  10.5MB  "), 10_500_000);
        assert_eq!(parse_size_to_bytes("1 byte"), 1);
        assert_eq!(parse_size_to_bytes("100 bytes"), 100);

        for invalid in [
            "",
            "   ",
            "MB",
            ".",
            "1.2.3 MB",
            "-5 MB",
            "10 foo",
            "10 meters",
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
    fn format_command_and_index_prefix_handle_escaping_and_wide_totals() {
        assert_eq!(
            format_command("podman", &["run", "--name", "app", "O'Hara", "Hancock 2"]),
            "podman run --name app 'O'\\''Hara' 'Hancock 2'"
        );
        assert_eq!(format_command("", &["--version"]), "'' --version");
        assert_eq!(
            format_command("FOO=bar", &["--env=BAZ", "./pods.conf"]),
            "'FOO=bar' --env=BAZ ./pods.conf"
        );
        assert_eq!(format_index_prefix(1, 5), "1)");
        assert_eq!(format_index_prefix(1, 10), " 1)");
        assert_eq!(format_index_prefix(10, 10), "10)");
        assert_eq!(format_index_prefix(1, 100), "  1)");
        assert_eq!(format_index_prefix(10, 100), " 10)");
        assert_eq!(format_index_prefix(100, 100), "100)");
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
        assert_eq!(clamped.chars().count(), MAX_FORMATTED_LINE_BYTES + 4);
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
        assert_eq!(
            format_version_header("Podoxide", "v0.1.0", None),
            "Podoxide v0.1.0"
        );
        assert_eq!(
            format_version_header("Podoxide", "v0.1.0", Some("")),
            "Podoxide v0.1.0"
        );
        assert_eq!(
            format_version_header("Podoxide", "v0.1.0", Some("Command Mode")),
            "Podoxide v0.1.0 - Command Mode"
        );
    }
}
