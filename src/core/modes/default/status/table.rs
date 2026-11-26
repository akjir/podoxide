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

//! Status record parsing, column alignment, and table rendering for `pods status`.

use std::borrow::Cow;
use std::io::Write;
use std::iter;
use std::str::Chars;

use crate::core::log::{self, MAX_LOG_MESSAGE_BYTES};
use crate::io::exec::podman::FIELD_DELIMITER;
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::{format, string};

const COLUMN_SEPARATOR: &str = "   ";
const DEFAULT_COLUMN_COUNT: usize = 7;
const DEFAULT_COLUMN_HEADERS: [&str; DEFAULT_COLUMN_COUNT] = [
    "ID", "POD", "NAME", "STATUS", "RESTARTS", "CREATED", "IMAGE",
];
const FULL_COLUMN_COUNT: usize = 10;
const FULL_COLUMN_HEADERS: [&str; FULL_COLUMN_COUNT] = [
    "ID",
    "POD",
    "NAME",
    "TARGET NAME",
    "STATUS",
    "RESTARTS",
    "CREATED",
    "IMAGE",
    "COMMAND",
    "PORTS",
];
const MISSING_STATUS: &str = "Not Found";
const MISSING_VALUE: &str = "-";
const RECIPE_ERROR_STATUS: &str = "Recipe Error";

/// Single container row in the rendered runtime status table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct StatusRow<'a> {
    pub(super) command: Cow<'a, str>,
    pub(super) created: &'a str,
    pub(super) id: &'a str,
    pub(super) image: &'a str,
    pub(super) name: &'a str,
    pub(super) pod: &'a str,
    pub(super) ports: &'a str,
    pub(super) restarts: &'a str,
    pub(super) status: &'a str,
    pub(super) target_name: &'a str,
}

impl<'a> StatusRow<'a> {
    /// Creates a synthesized `"Not Found"` [`StatusRow`] for a missing managed container.
    #[must_use]
    pub(super) const fn missing(name: &'a str, pod: &'a str, target_name: &'a str) -> Self {
        Self {
            command: Cow::Borrowed(MISSING_VALUE),
            created: MISSING_VALUE,
            id: MISSING_VALUE,
            image: MISSING_VALUE,
            name,
            pod,
            ports: MISSING_VALUE,
            restarts: MISSING_VALUE,
            status: MISSING_STATUS,
            target_name,
        }
    }

    /// Creates a synthesized `"Not Found"` [`StatusRow`] for a configured recipe whose `.pod` file is missing on disk.
    #[must_use]
    pub(super) const fn missing_recipe(name: &'a str) -> Self {
        Self::missing(name, MISSING_VALUE, name)
    }

    /// Creates a synthesized `"Recipe Error"` [`StatusRow`] for an invalid `.pod` recipe.
    #[must_use]
    pub(super) const fn recipe_error(name: &'a str) -> Self {
        Self {
            command: Cow::Borrowed(MISSING_VALUE),
            created: MISSING_VALUE,
            id: MISSING_VALUE,
            image: MISSING_VALUE,
            name,
            pod: MISSING_VALUE,
            ports: MISSING_VALUE,
            restarts: MISSING_VALUE,
            status: RECIPE_ERROR_STATUS,
            target_name: name,
        }
    }

    /// Returns the seven default column values in display order.
    #[must_use]
    pub(super) const fn default_cells(&self) -> [&str; DEFAULT_COLUMN_COUNT] {
        [
            self.id,
            self.pod,
            self.name,
            self.status,
            self.restarts,
            self.created,
            self.image,
        ]
    }

    /// Returns all ten `--full` column values in display order.
    #[must_use]
    pub(super) fn full_cells(&self) -> [&str; FULL_COLUMN_COUNT] {
        [
            self.id,
            self.pod,
            self.name,
            self.target_name,
            self.status,
            self.restarts,
            self.created,
            self.image,
            self.command.as_ref(),
            self.ports,
        ]
    }
}

/// Formats a slice of table `cells` padded to `widths`, separated by [`COLUMN_SEPARATOR`], with trailing ASCII spaces trimmed.
#[must_use]
pub(super) fn format_table_row(cells: &[&str], widths: &[usize]) -> String {
    let pair_count = cells.len().min(widths.len());
    let separator_bytes = pair_count
        .saturating_sub(1)
        .saturating_mul(COLUMN_SEPARATOR.len());
    let total_width = widths[..pair_count]
        .iter()
        .copied()
        .fold(0usize, usize::saturating_add);
    let capacity = total_width
        .saturating_add(separator_bytes)
        .min(MAX_LOG_MESSAGE_BYTES);
    let mut line = String::with_capacity(capacity);
    let last_index = pair_count.saturating_sub(1);
    for (i, (&cell, &width)) in cells.iter().zip(widths.iter()).enumerate() {
        if i > 0 {
            line.push_str(COLUMN_SEPARATOR);
        }
        if i == last_index {
            push_sanitized_cell(&mut line, cell);
        } else {
            push_padded_cell(&mut line, cell, width);
        }
    }
    line.truncate(line.trim_end_matches(' ').len());
    line
}

/// Parses a control-delimited `podman ps -a --format` record into a [`StatusRow`].
#[must_use]
pub(super) fn parse_status_record(record: &str) -> Option<StatusRow<'_>> {
    if record.is_empty() {
        return None;
    }
    let (id, rest) = record.split_once(FIELD_DELIMITER)?;
    let (image, rest) = rest.split_once(FIELD_DELIMITER)?;
    let (rest, restarts) = rest.rsplit_once(FIELD_DELIMITER)?;
    let (rest, pod) = rest.rsplit_once(FIELD_DELIMITER)?;
    let (rest, name) = rest.rsplit_once(FIELD_DELIMITER)?;
    let (rest, ports) = rest.rsplit_once(FIELD_DELIMITER)?;
    let (rest, status) = rest.rsplit_once(FIELD_DELIMITER)?;
    let (raw_command, created) = rest.rsplit_once(FIELD_DELIMITER)?;
    if id.is_empty() || name.is_empty() {
        return None;
    }
    Some(StatusRow {
        command: decode_json_string(raw_command),
        created,
        id,
        image,
        name,
        pod,
        ports,
        restarts,
        status,
        target_name: MISSING_VALUE,
    })
}

/// Updates each entry in `widths` with the maximum of its current value and the visible character count of the corresponding cell in `cells`.
pub(super) fn update_column_widths(widths: &mut [usize], cells: &[&str]) {
    for (width, cell) in widths.iter_mut().zip(cells.iter()) {
        let char_len = string::clamp_utf8_prefix(cell, MAX_LOG_MESSAGE_BYTES)
            .chars()
            .count();
        if char_len > *width {
            *width = char_len;
        }
    }
}

/// Writes a blank line and the `pods recipe inspect` hint when `invalid_recipes` is non-empty.
pub(super) fn write_invalid_recipes_hint<W: Write>(invalid_recipes: &[String], stdout: &mut W) {
    if invalid_recipes.is_empty() {
        return;
    }
    let capacity = collection::bounded_capacity(invalid_recipes.len(), MAX_COLLECTION_PREALLOC);
    let mut refs = Vec::with_capacity(capacity);
    for name in invalid_recipes {
        refs.push(name.as_str());
    }
    log::write_print(stdout, "");
    log::write_print(stdout, &format::format_recipe_error_hint(&refs));
}

/// Formats and writes the aligned status header and container rows to `stdout`.
pub(super) fn write_status_table<W: Write>(rows: &[StatusRow<'_>], is_full: bool, stdout: &mut W) {
    let widths = compute_column_widths(rows, is_full);
    if is_full {
        let header_line = format_table_row(&FULL_COLUMN_HEADERS, &widths);
        log::write_print(stdout, &header_line);
        for row in rows {
            let row_line = format_table_row(&row.full_cells(), &widths);
            log::write_print(stdout, &row_line);
        }
        return;
    }
    let active_widths = &widths[..DEFAULT_COLUMN_COUNT];
    let header_line = format_table_row(&DEFAULT_COLUMN_HEADERS, active_widths);
    log::write_print(stdout, &header_line);
    for row in rows {
        let row_line = format_table_row(&row.default_cells(), active_widths);
        log::write_print(stdout, &row_line);
    }
}

/// Computes the maximum visible character width for each active column across headers and `rows`.
#[must_use]
fn compute_column_widths(rows: &[StatusRow<'_>], is_full: bool) -> [usize; FULL_COLUMN_COUNT] {
    let mut widths = [0usize; FULL_COLUMN_COUNT];
    if is_full {
        update_column_widths(&mut widths, &FULL_COLUMN_HEADERS);
        for row in rows {
            update_column_widths(&mut widths, &row.full_cells());
        }
        return widths;
    }
    let active = &mut widths[..DEFAULT_COLUMN_COUNT];
    update_column_widths(active, &DEFAULT_COLUMN_HEADERS);
    for row in rows {
        update_column_widths(active, &row.default_cells());
    }
    widths
}

/// Decodes a single JSON escape sequence following a leading backslash.
#[must_use]
fn decode_json_escape(chars: &mut Chars<'_>) -> Option<char> {
    match chars.next()? {
        '"' => Some('"'),
        '\\' => Some('\\'),
        '/' => Some('/'),
        'b' => Some('\u{0008}'),
        'f' => Some('\u{000C}'),
        'n' => Some('\n'),
        'r' => Some('\r'),
        't' => Some('\t'),
        'u' => decode_json_unicode(chars),
        _ => None,
    }
}

/// Decodes a JSON-quoted string literal (as emitted by `{{json .Command}}`), borrowing zero-copy when unescaped.
#[must_use]
fn decode_json_string(raw: &str) -> Cow<'_, str> {
    let Some(inner) = raw
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    else {
        return Cow::Borrowed(raw);
    };
    if !inner.contains('\\') {
        return Cow::Borrowed(inner);
    }
    let mut decoded = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        let Some(escaped) = decode_json_escape(&mut chars) else {
            return Cow::Borrowed(raw);
        };
        decoded.push(escaped);
    }
    Cow::Owned(decoded)
}

/// Decodes a `\uXXXX` escape (including a UTF-16 surrogate pair) into a Unicode scalar value.
#[must_use]
fn decode_json_unicode(chars: &mut Chars<'_>) -> Option<char> {
    let high = read_hex_u16(chars)?;
    if !(0xD800..=0xDFFF).contains(&high) {
        return char::from_u32(u32::from(high));
    }
    if high > 0xDBFF || chars.next() != Some('\\') || chars.next() != Some('u') {
        return None;
    }
    let low = read_hex_u16(chars)?;
    if !(0xDC00..=0xDFFF).contains(&low) {
        return None;
    }
    let scalar = 0x10000 + ((u32::from(high) - 0xD800) << 10) + (u32::from(low) - 0xDC00);
    char::from_u32(scalar)
}

/// Appends sanitized `value` right-padded with ASCII spaces to `width` visible characters into `buffer`.
fn push_padded_cell(buffer: &mut String, value: &str, width: usize) {
    let char_len = push_sanitized_cell(buffer, value);
    let clamped_width = width.min(MAX_LOG_MESSAGE_BYTES);
    let pad = clamped_width.saturating_sub(char_len);
    if pad > 0 {
        buffer.extend(iter::repeat_n(' ', pad));
    }
}

/// Appends `value` clamped to [`MAX_LOG_MESSAGE_BYTES`] with control/BiDi characters sanitized into `buffer`, returning its visible character count.
fn push_sanitized_cell(buffer: &mut String, value: &str) -> usize {
    let clamped = string::clamp_utf8_prefix(value, MAX_LOG_MESSAGE_BYTES);
    if !string::contains_control_or_bidi(clamped) {
        buffer.push_str(clamped);
        return clamped.chars().count();
    }
    let mut char_len = 0usize;
    for character in clamped.chars() {
        buffer.push(string::sanitize_char(character));
        char_len += 1;
    }
    char_len
}

/// Reads four hexadecimal ASCII digits from `chars` into a `u16`.
#[must_use]
fn read_hex_u16(chars: &mut Chars<'_>) -> Option<u16> {
    let mut value = 0u16;
    for _ in 0..4 {
        let digit = chars.next()?.to_digit(16)?;
        value = (value << 4) | u16::try_from(digit).ok()?;
    }
    Some(value)
}
