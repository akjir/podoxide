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

//! Single-pass quote-aware byte scanning for inline comments, escapes, and unquoted delimiters.

use std::ops::ControlFlow;

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

/// Returns the byte length of the `\<char>` escape sequence starting at `backslash_index` in `text`.
#[must_use]
pub(super) fn escape_sequence_byte_len(text: &str, backslash_index: usize) -> usize {
    let char_len = text
        .get(backslash_index + 1..)
        .and_then(|rest| rest.chars().next())
        .map_or(0, char::len_utf8);
    1 + char_len
}

/// Finds the byte offset and length of an invalid escape sequence inside a double-quoted string on `code_slice`.
#[must_use]
pub(super) fn find_invalid_escape_bytes(code_slice: &str) -> Option<(usize, usize)> {
    scan_quoted_text(code_slice, |event| match event {
        QuoteScanEvent::DoubleQuoteEscape {
            len,
            next_byte: Some(next),
            start,
        } if !matches!(next, b'"' | b'\\' | b'n' | b'r' | b't') => ControlFlow::Break((start, len)),
        _ => ControlFlow::Continue(()),
    })
}

/// Finds the first byte offset of `target_byte` outside of single- or double-quoted strings in `code_slice`.
#[must_use]
pub(super) fn find_unquoted_byte(code_slice: &str, target_byte: u8) -> Option<usize> {
    scan_quoted_text(code_slice, |event| match event {
        QuoteScanEvent::UnquotedByte { byte, offset } if byte == target_byte => {
            ControlFlow::Break(offset)
        }
        _ => ControlFlow::Continue(()),
    })
}

/// Finds the byte offset of an unclosed opening `"` or `'` quote on `line`.
#[must_use]
pub(super) fn find_unterminated_quote_byte(line: &str) -> Option<usize> {
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
