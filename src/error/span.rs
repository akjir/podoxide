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

//! Quote-aware source line scanning and token span resolution for compiler-style diagnostics.

use std::ops::ControlFlow;
use std::str::Chars;

use crate::error::psl::PslErrorKind;
use crate::utilities::string;

/// Event emitted while scanning quote-delimited source text in a single pass.
enum QuoteScanEvent {
    DoubleQuoteEscape {
        len: usize,
        next_byte: Option<u8>,
        start: usize,
    },
    QuoteToggle {
        offset: usize,
        opened: bool,
    },
    UnquotedByte {
        byte: u8,
        offset: usize,
    },
}

/// Computes the 1-based character column and span for a [`PslErrorKind`] on `sanitized_line`.
#[must_use]
pub(super) fn locate_psl_error_span(sanitized_line: &str, kind: PslErrorKind) -> (usize, usize) {
    let code_slice = strip_inline_comment(sanitized_line);
    match kind {
        PslErrorKind::InvalidBlockHeader => {
            span_from_byte_offset(code_slice, find_unquoted_byte(code_slice, b'{', true), 1)
        }
        PslErrorKind::InvalidEscapeSequence => find_invalid_escape_bytes(code_slice).map_or_else(
            || highlight_trimmed_code(code_slice),
            |(start, len)| byte_range_to_char_span(code_slice, start, len),
        ),
        PslErrorKind::InvalidIdentifier
        | PslErrorKind::MissingDirectiveArgument
        | PslErrorKind::UnexpectedRootDirective => highlight_first_token(code_slice),
        PslErrorKind::LimitExceeded
        | PslErrorKind::UnclosedBlock
        | PslErrorKind::UnexpectedToken => highlight_trimmed_code(code_slice),
        PslErrorKind::UnexpectedClosingBrace => {
            span_from_byte_offset(code_slice, find_unquoted_byte(code_slice, b'}', false), 1)
        }
        PslErrorKind::UnterminatedString => find_unterminated_quote_byte(sanitized_line)
            .map_or_else(
                || highlight_trimmed_code(code_slice),
                |byte_offset| {
                    let byte_len = sanitized_line.len().saturating_sub(byte_offset).max(1);
                    byte_range_to_char_span(sanitized_line, byte_offset, byte_len)
                },
            ),
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

/// Strips any trailing `#` comment outside of single- or double-quoted strings.
#[must_use]
pub(super) fn strip_inline_comment(line: &str) -> &str {
    let comment_start = scan_quoted_text(line, |event| match event {
        QuoteScanEvent::UnquotedByte { byte: b'#', offset } => ControlFlow::Break(offset),
        _ => ControlFlow::Continue(()),
    });
    comment_start
        .and_then(|offset| line.get(..offset))
        .unwrap_or(line)
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

/// Returns the byte length of the `\<char>` escape sequence starting at `backslash_index` in `text`.
#[must_use]
fn escape_sequence_byte_len(text: &str, backslash_index: usize) -> usize {
    let char_len = text
        .get(backslash_index + 1..)
        .and_then(|rest| rest.chars().next())
        .map_or(0, char::len_utf8);
    1 + char_len
}

/// Finds the byte offset and length of an invalid escape sequence inside a double-quoted string on `code_slice`.
#[must_use]
fn find_invalid_escape_bytes(code_slice: &str) -> Option<(usize, usize)> {
    scan_quoted_text(code_slice, |event| match event {
        QuoteScanEvent::DoubleQuoteEscape {
            len,
            next_byte: Some(next),
            start,
        } if !matches!(next, b'"' | b'\\' | b'n' | b'r' | b't') => ControlFlow::Break((start, len)),
        _ => ControlFlow::Continue(()),
    })
}

/// Searches `code_slice` for a standalone token matching `target` (preferring argument tokens when `prefer_argument` is `true`).
#[must_use]
fn find_matching_token_bytes(
    code_slice: &str,
    target: &str,
    prefer_argument: bool,
) -> Option<(usize, usize)> {
    let bytes = code_slice.as_bytes();
    let mut cursor = 0;
    let mut first_match = None;
    let mut token_index = 0_usize;

    while let Some(&byte) = bytes.get(cursor) {
        if byte.is_ascii_whitespace() || byte == b'{' || byte == b'}' {
            cursor += 1;
            continue;
        }
        let (token_start, token_end) = scan_next_code_token(code_slice, cursor);
        if token_matches_target(code_slice, token_start, token_end, target) {
            let span = (token_start, token_end - token_start);
            if !prefer_argument || token_index > 0 || first_match.is_some() {
                return Some(span);
            }
            first_match = Some(span);
        }
        token_index += 1;
        cursor = token_end;
    }
    first_match
}

/// Finds the first or last byte offset of `target_byte` outside of single- or double-quoted strings in `code_slice`.
#[must_use]
fn find_unquoted_byte(code_slice: &str, target_byte: u8, from_end: bool) -> Option<usize> {
    let mut matched = None;
    let early = scan_quoted_text(code_slice, |event| match event {
        QuoteScanEvent::UnquotedByte { byte, offset } if byte == target_byte => {
            if !from_end {
                return ControlFlow::Break(offset);
            }
            matched = Some(offset);
            ControlFlow::Continue(())
        }
        _ => ControlFlow::Continue(()),
    });
    early.or(matched)
}

/// Finds the byte offset of an unclosed opening `"` or `'` quote on `line`.
#[must_use]
fn find_unterminated_quote_byte(line: &str) -> Option<usize> {
    let mut open_quote_offset = None;
    let _ = scan_quoted_text(line, |event| match event {
        QuoteScanEvent::QuoteToggle { offset, opened } => {
            open_quote_offset = opened.then_some(offset);
            ControlFlow::Continue(())
        }
        QuoteScanEvent::UnquotedByte { byte: b'#', .. } => ControlFlow::Break(()),
        _ => ControlFlow::Continue(()),
    });
    open_quote_offset
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
            cursor += escape_sequence_byte_len(code_slice, cursor);
            continue;
        }
        if byte == quote {
            return (start, cursor + 1);
        }
        cursor += 1;
    }
    (start, bytes.len())
}

/// Scans `text` in a single pass tracking single- and double-quote state and emitting [`QuoteScanEvent`] items.
fn scan_quoted_text<T, F>(text: &str, mut on_event: F) -> Option<T>
where
    F: FnMut(QuoteScanEvent) -> ControlFlow<T>,
{
    let bytes = text.as_bytes();
    let mut in_double = false;
    let mut in_single = false;
    let mut index = 0;

    while index < bytes.len() {
        if let Some(event) =
            step_quoted_text(text, bytes, &mut index, &mut in_double, &mut in_single)
            && let ControlFlow::Break(value) = on_event(event)
        {
            return Some(value);
        }
    }
    None
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

/// Advances the quote-aware scanner by one step at `*index` and returns any significant [`QuoteScanEvent`].
fn step_quoted_text(
    text: &str,
    bytes: &[u8],
    index: &mut usize,
    in_double: &mut bool,
    in_single: &mut bool,
) -> Option<QuoteScanEvent> {
    let offset = *index;
    let byte = bytes[offset];
    if *in_double && byte == b'\\' {
        let len = escape_sequence_byte_len(text, offset);
        *index += len;
        return Some(QuoteScanEvent::DoubleQuoteEscape {
            len,
            next_byte: bytes.get(offset + 1).copied(),
            start: offset,
        });
    }
    *index += 1;
    if !*in_single && byte == b'"' {
        *in_double = !*in_double;
        return Some(QuoteScanEvent::QuoteToggle {
            offset,
            opened: *in_double,
        });
    }
    if !*in_double && byte == b'\'' {
        *in_single = !*in_single;
        return Some(QuoteScanEvent::QuoteToggle {
            offset,
            opened: *in_single,
        });
    }
    (!*in_double && !*in_single).then_some(QuoteScanEvent::UnquotedByte { byte, offset })
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
mod tests {
    use crate::error::{ConfigError, ConfigErrorKind, RecipeError, RecipeErrorKind};
    use super::unescaped_double_quote_matches;

    #[test]
    fn token_span_resolution_handles_empty_escaped_and_colliding_tokens() {
        let empty_double = ConfigError::with_token(ConfigErrorKind::InvalidPath, 1, "")
            .with_diagnostic("    path \"\" # empty\n", "./pods.conf");
        assert_eq!(empty_double.diagnostic.as_ref().unwrap().col_number, 10);
        assert_eq!(
            empty_double.diagnostic.as_ref().unwrap().annotation_carets,
            "         ^^"
        );

        let empty_single = ConfigError::with_token(ConfigErrorKind::InvalidPath, 1, "")
            .with_diagnostic("    path '' # empty\n", "./pods.conf");
        assert_eq!(empty_single.diagnostic.as_ref().unwrap().col_number, 10);
        assert_eq!(
            empty_single.diagnostic.as_ref().unwrap().annotation_carets,
            "         ^^"
        );

        let escaped_token =
            RecipeError::with_token(RecipeErrorKind::InvalidDescription, 1, "bad\ndesc")
                .with_diagnostic("    description \"bad\\ndesc\" # comment\n", "./recipe.pod");
        let escaped_diag = escaped_token.diagnostic.as_ref().unwrap();
        assert_eq!(escaped_diag.col_number, 17);
        assert_eq!(
            escaped_diag.annotation_carets,
            "                ^^^^^^^^^^^"
        );

        let collision = RecipeError::with_token(RecipeErrorKind::DuplicateBlock, 1, "command")
            .with_diagnostic("    command command {\n", "./recipe.pod");
        assert_eq!(collision.diagnostic.as_ref().unwrap().col_number, 13);

        let duplicate_group =
            ConfigError::with_token(ConfigErrorKind::DuplicateDirective, 1, "alpha")
                .with_diagnostic("        alpha beta alpha\n", "./pods.conf");
        assert_eq!(duplicate_group.diagnostic.as_ref().unwrap().col_number, 9);

        let invalid_boolean =
            ConfigError::with_token(ConfigErrorKind::InvalidBoolean, 1, "simulate")
                .with_diagnostic("    simulate simulate\n", "./pods.conf");
        assert_eq!(invalid_boolean.diagnostic.as_ref().unwrap().col_number, 14);

        assert!(!unescaped_double_quote_matches("plain", "plain"));
        assert!(unescaped_double_quote_matches(
            "a\\nb\\rc\\td\\\"e\\\\f",
            "a b c d\"e\\f"
        ));
        assert!(unescaped_double_quote_matches("trailing\\", "trailing\\"));
        assert!(!unescaped_double_quote_matches("a\\nb", "a "));
        assert!(!unescaped_double_quote_matches("a\\nb", "a b extra"));
        assert!(!unescaped_double_quote_matches("a\\nb", "a x"));
    }
}
