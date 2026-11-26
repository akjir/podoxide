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

//! Token and column span resolution for compiler-style diagnostics.

use std::ops::ControlFlow;
use std::str::Chars;

use crate::error::psl::PslErrorKind;
use crate::utilities::string;
use super::quote_scan;
pub(super) use super::quote_scan::strip_inline_comment;

/// Computes the 1-based character column and span for a [`PslErrorKind`] on `sanitized_line`.
#[must_use]
pub(super) fn locate_psl_error_span(
    sanitized_line: &str,
    kind: PslErrorKind,
    column: Option<usize>,
) -> (usize, usize) {
    let code_slice = quote_scan::strip_inline_comment(sanitized_line);
    if let Some(column_number) = column {
        return locate_psl_span_at_column(sanitized_line, code_slice, kind, column_number);
    }

    match kind {
        PslErrorKind::InvalidBlockHeader => span_from_byte_offset(
            code_slice,
            quote_scan::find_unquoted_byte(code_slice, b'{'),
            1,
        ),
        PslErrorKind::InvalidEscapeSequence => quote_scan::find_invalid_escape_bytes(code_slice)
            .map_or_else(
                || highlight_trimmed_code(code_slice),
                |(start, len)| byte_range_to_char_span(code_slice, start, len),
            ),
        PslErrorKind::InvalidIdentifier
        | PslErrorKind::MissingDirectiveArgument
        | PslErrorKind::UnexpectedRootDirective => highlight_first_token(code_slice),
        PslErrorKind::LimitExceeded
        | PslErrorKind::UnclosedBlock
        | PslErrorKind::UnexpectedToken => highlight_trimmed_code(code_slice),
        PslErrorKind::UnexpectedClosingBrace => span_from_byte_offset(
            code_slice,
            quote_scan::find_unquoted_byte(code_slice, b'}'),
            1,
        ),
        PslErrorKind::UnterminatedString => {
            quote_scan::find_unterminated_quote_byte(sanitized_line).map_or_else(
                || highlight_trimmed_code(code_slice),
                |byte_offset| {
                    let byte_len = sanitized_line.len().saturating_sub(byte_offset).max(1);
                    byte_range_to_char_span(sanitized_line, byte_offset, byte_len)
                },
            )
        }
    }
}

/// Locates `token` inside `code_slice` and returns its 1-based `(col_number, span_len)`.
#[must_use]
pub(super) fn locate_token_span(
    code_slice: &str,
    token: Option<&str>,
    prefer_argument: bool,
) -> (usize, usize) {
    if let Some(target) = token {
        if let Some((byte_start, byte_len)) =
            find_matching_token_bytes(code_slice, target, prefer_argument)
        {
            return byte_range_to_char_span(code_slice, byte_start, byte_len);
        }
        if !target.is_empty()
            && let Some(byte_start) = code_slice.find(target)
        {
            return byte_range_to_char_span(code_slice, byte_start, target.len());
        }
    }
    highlight_trimmed_code(code_slice)
}

/// Locates the `arg_index`-th token on `code_slice` when specified, falling back to [`locate_token_span`].
#[must_use]
pub(super) fn locate_token_span_at_arg(
    code_slice: &str,
    token: Option<&str>,
    prefer_argument: bool,
    arg_index: Option<usize>,
) -> (usize, usize) {
    if let Some(target_index) = arg_index
        && let Some((byte_start, byte_len)) = find_token_bytes_at_index(code_slice, target_index)
    {
        return byte_range_to_char_span(code_slice, byte_start, byte_len);
    }
    locate_token_span(code_slice, token, prefer_argument)
}

/// Converts a byte range `(byte_start, byte_len)` inside `line` into a 1-based character column and character span.
#[must_use]
fn byte_range_to_char_span(line: &str, byte_start: usize, byte_len: usize) -> (usize, usize) {
    let col_number = line
        .get(..byte_start)
        .map_or(1, |prefix| prefix.chars().count().saturating_add(1));
    let span_len = line
        .get(byte_start..byte_start.saturating_add(byte_len))
        .map_or(1, |slice| slice.chars().count().max(1));
    (col_number, span_len)
}

/// Returns the character width (1 or 2) of a backslash escape sequence starting at 1-based `column`.
#[must_use]
fn escape_span_at_column(line: &str, column: usize) -> usize {
    line.chars()
        .skip(column.saturating_sub(1))
        .take(2)
        .count()
        .max(1)
}

/// Searches `code_slice` for a standalone token matching `target` (preferring argument tokens when `prefer_argument` is `true`).
#[must_use]
fn find_matching_token_bytes(
    code_slice: &str,
    target: &str,
    prefer_argument: bool,
) -> Option<(usize, usize)> {
    let mut first_match = None;
    let resolved = scan_code_tokens(code_slice, |index, start, end| {
        if token_matches_target(code_slice, start, end, target) {
            let span = (start, end - start);
            if !prefer_argument || index > 0 || first_match.is_some() {
                return ControlFlow::Break(span);
            }
            first_match = Some(span);
        }
        ControlFlow::Continue(())
    });
    resolved.or(first_match)
}

/// Returns the `(byte_start, byte_len)` of the `target_index`-th token (`0` = directive name, `1..` = arguments) in `code_slice`.
#[must_use]
fn find_token_bytes_at_index(code_slice: &str, target_index: usize) -> Option<(usize, usize)> {
    scan_code_tokens(code_slice, |index, start, end| {
        if index == target_index {
            ControlFlow::Break((start, end - start))
        } else {
            ControlFlow::Continue(())
        }
    })
}

/// Computes the 1-based character column and span for the first whitespace-delimited token on `code_slice`.
#[must_use]
fn highlight_first_token(code_slice: &str) -> (usize, usize) {
    let trimmed_start = code_slice.trim_start();
    let first_token = trimmed_start
        .split_whitespace()
        .next()
        .unwrap_or(trimmed_start);
    if first_token.is_empty() {
        return (1, 1);
    }
    let byte_start = code_slice.len() - trimmed_start.len();
    byte_range_to_char_span(code_slice, byte_start, first_token.len())
}

/// Computes the 1-based character column and span for the non-whitespace portion of `code_slice`.
#[must_use]
fn highlight_trimmed_code(code_slice: &str) -> (usize, usize) {
    let trimmed_end = code_slice.trim_end();
    let trimmed = trimmed_end.trim_start();
    if trimmed.is_empty() {
        return (1, 1);
    }
    let byte_start = trimmed_end.len() - trimmed.len();
    byte_range_to_char_span(code_slice, byte_start, trimmed.len())
}

/// Resolves a `(col_number, span_len)` diagnostic span from an explicit 1-based parser `column`.
#[must_use]
fn locate_psl_span_at_column(
    sanitized_line: &str,
    code_slice: &str,
    kind: PslErrorKind,
    column: usize,
) -> (usize, usize) {
    let effective_column = column.max(1);
    match kind {
        PslErrorKind::InvalidBlockHeader
        | PslErrorKind::UnclosedBlock
        | PslErrorKind::UnexpectedClosingBrace => (effective_column, 1),
        PslErrorKind::InvalidEscapeSequence => (
            effective_column,
            escape_span_at_column(sanitized_line, effective_column),
        ),
        PslErrorKind::InvalidIdentifier
        | PslErrorKind::LimitExceeded
        | PslErrorKind::MissingDirectiveArgument
        | PslErrorKind::UnexpectedRootDirective
        | PslErrorKind::UnexpectedToken => token_span_at_column(code_slice, effective_column),
        PslErrorKind::UnterminatedString => {
            let total_chars = sanitized_line.chars().count();
            let span_len = total_chars.saturating_sub(effective_column - 1).max(1);
            (effective_column, span_len)
        }
    }
}

/// Advances `chars` by one PSL double-quoted character, resolving backslash escapes and sanitizing control characters.
#[must_use]
fn next_unescaped_char(chars: &mut Chars<'_>) -> Option<char> {
    let character = chars.next()?;
    if character != '\\' {
        return Some(string::sanitize_char(character));
    }
    let resolved = match chars.next() {
        Some('n' | 'r' | 't') => ' ',
        Some(escaped) => string::sanitize_char(escaped),
        None => '\\',
    };
    Some(resolved)
}

/// Evaluates whether a single- or double-quoted slice `raw_quoted` unquotes to `target`.
#[must_use]
fn quoted_value_matches(raw_quoted: &str, target: &str) -> bool {
    if let Some(inner) = raw_quoted
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
    {
        return inner == target;
    }
    if let Some(inner) = raw_quoted
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        return inner == target || unescaped_double_quote_matches(inner, target);
    }
    false
}

/// Scans a bare (or assigned-quote `key="..."` / `key='...'`) token starting at `start` in `code_slice`.
#[must_use]
fn scan_bare_code_token(code_slice: &str, start: usize) -> (usize, usize) {
    let bytes = code_slice.as_bytes();
    let mut cursor = start;
    while let Some(&byte) = bytes.get(cursor) {
        if byte.is_ascii_whitespace() || byte == b'{' || byte == b'}' {
            break;
        }
        if matches!(byte, b'"' | b'\'')
            && cursor > start.saturating_add(1)
            && bytes.get(cursor - 1) == Some(&b'=')
        {
            let (_, quote_end) = scan_quoted_code_token(code_slice, cursor, byte);
            return (start, quote_end);
        }
        cursor += 1;
    }
    (start, cursor)
}

/// Iterates bare and quoted code tokens in `code_slice` with 0-based indices until `on_token` breaks.
fn scan_code_tokens<T, F>(code_slice: &str, mut on_token: F) -> Option<T>
where
    F: FnMut(usize, usize, usize) -> ControlFlow<T>,
{
    let bytes = code_slice.as_bytes();
    let mut cursor = 0;
    let mut token_index = 0_usize;
    while let Some(&byte) = bytes.get(cursor) {
        if byte.is_ascii_whitespace() || byte == b'{' || byte == b'}' {
            cursor += 1;
            continue;
        }
        let (token_start, token_end) = scan_next_code_token(code_slice, cursor);
        if let ControlFlow::Break(value) = on_token(token_index, token_start, token_end) {
            return Some(value);
        }
        token_index += 1;
        cursor = token_end;
    }
    None
}

/// Scans a single bare or quoted token starting at `start` in `code_slice` and returns `(start, end)`.
#[must_use]
fn scan_next_code_token(code_slice: &str, start: usize) -> (usize, usize) {
    if let Some(quote @ (b'"' | b'\'')) = code_slice.as_bytes().get(start).copied() {
        return scan_quoted_code_token(code_slice, start, quote);
    }
    scan_bare_code_token(code_slice, start)
}

/// Scans a single- or double-quoted token starting at `start` with delimiter `quote` and returns `(start, end)`.
#[must_use]
fn scan_quoted_code_token(code_slice: &str, start: usize, quote: u8) -> (usize, usize) {
    let bytes = code_slice.as_bytes();
    let mut cursor = start + 1;
    while let Some(&byte) = bytes.get(cursor) {
        if quote == b'"' && byte == b'\\' {
            cursor += quote_scan::escape_sequence_byte_len(code_slice, cursor);
            continue;
        }
        if byte == quote {
            return (start, cursor + 1);
        }
        cursor += 1;
    }
    (start, bytes.len())
}

/// Converts an optional byte offset and byte length in `code_slice` into a character span, or highlights `code_slice`.
#[must_use]
fn span_from_byte_offset(
    code_slice: &str,
    byte_offset: Option<usize>,
    byte_len: usize,
) -> (usize, usize) {
    byte_offset.map_or_else(
        || highlight_trimmed_code(code_slice),
        |byte_offset| byte_range_to_char_span(code_slice, byte_offset, byte_len),
    )
}

/// Evaluates whether `code_slice[start..end]` matches `target` directly, unquoted, or as an assigned `key="val"` token.
#[must_use]
fn token_matches_target(code_slice: &str, start: usize, end: usize, target: &str) -> bool {
    let Some(raw_token) = code_slice.get(start..end) else {
        return false;
    };
    if !target.is_empty() && raw_token == target {
        return true;
    }
    if quoted_value_matches(raw_token, target) {
        return true;
    }
    if let Some((raw_key, raw_value)) = raw_token.split_once('=')
        && let Some((target_key, target_value)) = target.split_once('=')
        && !raw_key.is_empty()
        && raw_key == target_key
    {
        return quoted_value_matches(raw_value, target_value);
    }
    false
}

/// Computes `(column, span_len)` for the token starting at 1-based `column` within `code_slice`.
#[must_use]
fn token_span_at_column(code_slice: &str, column: usize) -> (usize, usize) {
    let Some((byte_start, first_char)) = code_slice.char_indices().nth(column.saturating_sub(1))
    else {
        return (column, 1);
    };
    if first_char.is_whitespace() || matches!(first_char, '{' | '}') {
        return (column, 1);
    }
    let (token_start, token_end) = scan_next_code_token(code_slice, byte_start);
    let span_len = code_slice
        .get(token_start..token_end)
        .map_or(1, |slice| slice.chars().count().max(1));
    (column, span_len)
}

/// Unescapes PSL backslash sequences in `inner`, sanitizes control characters, and compares the result to `target`.
#[must_use]
fn unescaped_double_quote_matches(inner: &str, target: &str) -> bool {
    if !inner.contains('\\') {
        return false;
    }
    let mut inner_chars = inner.chars();
    let mut target_chars = target.chars();
    while let Some(resolved) = next_unescaped_char(&mut inner_chars) {
        if target_chars.next() != Some(resolved) {
            return false;
        }
    }
    target_chars.next().is_none()
}

#[cfg(test)]
#[path = "span_tests.rs"]
mod tests;
