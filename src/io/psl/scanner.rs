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

//! Single-pass O(N) lexical scanner for the PodScript Language (PSL).

use std::borrow::Cow;

use crate::error::{PslError, PslErrorKind};
use crate::utilities::string;

const INITIAL_ESCAPE_EXTRA_CAPACITY: usize = 16;

/// Single-pass UTF-8 cursor scanner tracking line numbers and line-start byte offsets.
pub(super) struct Scanner<'a> {
    bytes: &'a [u8],
    cursor: usize,
    input: &'a str,
    line: usize,
    line_start: usize,
}

impl<'a> Scanner<'a> {
    /// Creates a new [`Scanner`] initialized at the beginning of `input`.
    #[must_use]
    pub(super) const fn new(input: &'a str) -> Self {
        Self {
            bytes: input.as_bytes(),
            cursor: 0,
            input,
            line: 1,
            line_start: 0,
        }
    }

    /// Advances the cursor by one ASCII byte.
    #[inline]
    pub(super) const fn advance_byte(&mut self) {
        self.cursor += 1;
    }

    /// Consumes and returns the next UTF-8 character at the cursor, or `None` at end of input.
    fn advance_char(&mut self) -> Option<char> {
        let character = self.peek_char()?;
        self.cursor += character.len_utf8();
        Some(character)
    }

    /// Consumes a `\n` or `\r\n` newline sequence, updates line state, and rejects standalone `\r`.
    pub(super) fn advance_newline(&mut self) -> Result<bool, PslError> {
        let step = match self.peek_byte() {
            Some(b'\n') => 1,
            Some(b'\r') if self.is_crlf() => 2,
            Some(b'\r') => return Err(self.error_here(PslErrorKind::UnexpectedToken, self.line)),
            _ => return Ok(false),
        };
        self.cursor += step;
        self.line += 1;
        self.line_start = self.cursor;
        Ok(true)
    }

    /// Parses a backslash escape inside a double-quoted string and appends the preceding slice and unescaped character to `unescaped_buffer`.
    fn append_escape_sequence(
        &mut self,
        content_start: usize,
        quote_offset: usize,
        segment_start: &mut usize,
        unescaped_buffer: &mut Option<String>,
        line: usize,
    ) -> Result<(), PslError> {
        let backslash_offset = self.cursor;
        self.cursor += 1;
        let unescaped = self.scan_escape_sequence(backslash_offset, quote_offset, line)?;
        let prefix = self.slice_input(*segment_start, backslash_offset, line)?;
        let buffer = unescaped_buffer.get_or_insert_with(|| {
            let capacity =
                (backslash_offset - content_start).saturating_add(INITIAL_ESCAPE_EXTRA_CAPACITY);
            String::with_capacity(capacity)
        });
        buffer.push_str(prefix);
        buffer.push(unescaped);
        *segment_start = self.cursor;
        Ok(())
    }

    /// Computes the 1-based UTF-8 character column for `byte_offset` relative to `line_start`.
    #[must_use]
    pub(super) fn column_between(&self, line_start: usize, byte_offset: usize) -> usize {
        self.input
            .get(line_start..byte_offset)
            .map_or(1, |prefix| prefix.chars().count().saturating_add(1))
    }

    /// Returns the current byte offset of the scanner cursor.
    #[inline]
    #[must_use]
    pub(super) const fn cursor(&self) -> usize {
        self.cursor
    }

    /// Constructs a [`PslError`] with the 1-based column computed from `byte_offset` on the current line.
    #[must_use]
    pub(super) fn error_at(&self, kind: PslErrorKind, line: usize, byte_offset: usize) -> PslError {
        let column = self.column_between(self.line_start, byte_offset);
        PslError::with_column(kind, line, column)
    }

    /// Constructs a [`PslError`] at the current cursor byte offset.
    #[must_use]
    pub(super) fn error_here(&self, kind: PslErrorKind, line: usize) -> PslError {
        self.error_at(kind, line, self.cursor)
    }

    /// Skips optional horizontal whitespace and comments, and verifies that the cursor is at a newline or end of input.
    pub(super) fn expect_end_of_line(&mut self, line: usize) -> Result<(), PslError> {
        self.skip_horizontal_whitespace()?;
        if self.peek_byte() == Some(b'#') {
            self.skip_comment()?;
        }
        if self.is_eof() || self.advance_newline()? {
            Ok(())
        } else {
            Err(self.error_here(PslErrorKind::UnexpectedToken, line))
        }
    }

    /// Finalizes a double-quoted string literal at the closing `"`, returning a borrowed slice or an owned unescaped string.
    fn finish_double_quoted(
        &mut self,
        content_start: usize,
        segment_start: usize,
        unescaped_buffer: Option<String>,
        line: usize,
    ) -> Result<Cow<'a, str>, PslError> {
        let content_end = self.cursor;
        self.cursor += 1;
        self.validate_post_quote(line)?;
        if let Some(mut buffer) = unescaped_buffer {
            let tail = self.slice_input(segment_start, content_end, line)?;
            buffer.push_str(tail);
            return Ok(Cow::Owned(buffer));
        }
        Ok(Cow::Borrowed(self.slice_input(
            content_start,
            content_end,
            line,
        )?))
    }

    /// Evaluates whether the bare word starting at `start` has at least one character followed by `=` immediately before `self.cursor`.
    #[inline]
    fn has_assignment_prefix(&self, start: usize) -> bool {
        self.cursor > start.saturating_add(1) && self.bytes.get(self.cursor - 1) == Some(&b'=')
    }

    /// Evaluates whether the byte immediately after `self.cursor` is `\n`, completing a `\r\n` sequence.
    #[inline]
    fn is_crlf(&self) -> bool {
        self.bytes.get(self.cursor + 1) == Some(&b'\n')
    }

    /// Evaluates whether the cursor has reached the end of the input buffer.
    #[inline]
    #[must_use]
    pub(super) const fn is_eof(&self) -> bool {
        self.cursor >= self.bytes.len()
    }

    /// Skips horizontal whitespace and evaluates whether the current statement line has ended at EOF, a comment, or a newline.
    pub(super) fn is_statement_end(&mut self, line: usize) -> Result<bool, PslError> {
        self.skip_horizontal_whitespace()?;
        match self.peek_byte() {
            None => Ok(true),
            Some(b'#') => {
                self.skip_comment()?;
                let _ = self.advance_newline()?;
                Ok(true)
            }
            Some(b'\n' | b'\r') => self.advance_newline(),
            Some(b'}') => Err(self.error_here(PslErrorKind::UnexpectedToken, line)),
            _ => Ok(false),
        }
    }

    /// Returns the current 1-based line number.
    #[inline]
    #[must_use]
    pub(super) const fn line(&self) -> usize {
        self.line
    }

    /// Returns the byte offset of the first byte on the current line.
    #[inline]
    #[must_use]
    pub(super) const fn line_start(&self) -> usize {
        self.line_start
    }

    /// Returns the byte at the current cursor position without advancing, or `None` at EOF.
    #[inline]
    #[must_use]
    pub(super) fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.cursor).copied()
    }

    /// Returns the UTF-8 character at the current cursor position without advancing, or `None` at EOF.
    fn peek_char(&self) -> Option<char> {
        self.input.get(self.cursor..)?.chars().next()
    }

    /// Dispatches argument scanning to double-quoted, single-quoted, or bare-word token scanners.
    pub(super) fn scan_argument(&mut self, line: usize) -> Result<Cow<'a, str>, PslError> {
        match self.peek_byte() {
            Some(b'"') => self.scan_double_quoted(line),
            Some(b'\'') => self.scan_single_quoted(line),
            _ => self.scan_bare_word(line),
        }
    }

    /// Scans a single- or double-quoted value immediately following an `=` bare-word prefix and concatenates them.
    fn scan_assigned_quote(
        &mut self,
        prefix: &'a str,
        line: usize,
    ) -> Result<Cow<'a, str>, PslError> {
        let quoted = if self.peek_byte() == Some(b'"') {
            self.scan_double_quoted(line)?
        } else {
            self.scan_single_quoted(line)?
        };
        let mut combined = String::with_capacity(prefix.len().saturating_add(quoted.len()));
        combined.push_str(prefix);
        combined.push_str(&quoted);
        Ok(Cow::Owned(combined))
    }

    /// Scans an unquoted bare-word argument token and returns it as a borrowed slice or an assigned `key=value` string.
    fn scan_bare_word(&mut self, line: usize) -> Result<Cow<'a, str>, PslError> {
        let start = self.cursor;
        while let Some(character) = self.peek_char() {
            if !self.step_bare_word_char(character, start, line)? {
                break;
            }
        }

        let slice = self.slice_input(start, self.cursor, line)?;
        if matches!(self.peek_byte(), Some(b'"' | b'\'')) {
            return self.scan_assigned_quote(slice, line);
        }
        Ok(Cow::Borrowed(slice))
    }

    /// Scans a double-quoted string argument, unescaping valid backslash sequences on demand.
    fn scan_double_quoted(&mut self, line: usize) -> Result<Cow<'a, str>, PslError> {
        let quote_offset = self.cursor;
        self.cursor += 1;
        let content_start = self.cursor;
        let mut segment_start = content_start;
        let mut unescaped_buffer: Option<String> = None;

        while let Some(character) = self.peek_char() {
            if character == '"' {
                return self.finish_double_quoted(
                    content_start,
                    segment_start,
                    unescaped_buffer,
                    line,
                );
            }
            if character == '\\' {
                self.append_escape_sequence(
                    content_start,
                    quote_offset,
                    &mut segment_start,
                    &mut unescaped_buffer,
                    line,
                )?;
                continue;
            }
            if character == '\r' || character == '\n' {
                return Err(self.unterminated(line, quote_offset));
            }
            self.cursor += character.len_utf8();
        }

        Err(self.unterminated(line, quote_offset))
    }

    /// Parses the character immediately following a `\` in a double-quoted string.
    fn scan_escape_sequence(
        &mut self,
        backslash_offset: usize,
        quote_offset: usize,
        line: usize,
    ) -> Result<char, PslError> {
        let Some(byte) = self
            .peek_byte()
            .filter(|&byte| byte != b'\r' && byte != b'\n')
        else {
            return Err(self.unterminated(line, quote_offset));
        };
        let Some(unescaped) = unescape_byte(byte) else {
            return Err(self.error_at(PslErrorKind::InvalidEscapeSequence, line, backslash_offset));
        };
        self.cursor += 1;
        Ok(unescaped)
    }

    /// Scans and validates a block or directive identifier (`[A-Za-z_][A-Za-z0-9_-]*`).
    pub(super) fn scan_identifier(&mut self, line: usize) -> Result<&'a str, PslError> {
        let start = self.cursor;
        if !self.peek_byte().is_some_and(string::is_identifier_start) {
            return Err(self.error_at(PslErrorKind::InvalidIdentifier, line, start));
        }

        self.cursor += 1;
        while self.peek_byte().is_some_and(string::is_identifier_continue) {
            self.cursor += 1;
        }

        self.validate_post_identifier(start, line)?;
        self.input
            .get(start..self.cursor)
            .ok_or_else(|| self.error_at(PslErrorKind::InvalidIdentifier, line, start))
    }

    /// Scans a literal single-quoted string argument without escape interpretation.
    fn scan_single_quoted(&mut self, line: usize) -> Result<Cow<'a, str>, PslError> {
        let quote_offset = self.cursor;
        self.cursor += 1;
        let content_start = self.cursor;

        while let Some(character) = self.advance_char() {
            if character == '\'' {
                let content_end = self.cursor - 1;
                self.validate_post_quote(line)?;
                return Ok(Cow::Borrowed(self.slice_input(
                    content_start,
                    content_end,
                    line,
                )?));
            }
            if character == '\r' || character == '\n' {
                return Err(self.unterminated(line, quote_offset));
            }
        }

        Err(self.unterminated(line, quote_offset))
    }

    /// Advances the cursor from `#` to the end of the line, rejecting control characters (except `\t`) and Unicode line separators.
    pub(super) fn skip_comment(&mut self) -> Result<(), PslError> {
        while let Some(character) = self.peek_char() {
            if character == '\n' || (character == '\r' && self.is_crlf()) {
                break;
            }
            if (character.is_control() && character != '\t')
                || matches!(character, '\u{2028}' | '\u{2029}')
            {
                return Err(self.error_here(PslErrorKind::UnexpectedToken, self.line));
            }
            self.cursor += character.len_utf8();
        }
        Ok(())
    }

    /// Advances the cursor past ASCII spaces and horizontal tabs, rejecting other whitespace or control characters.
    pub(super) fn skip_horizontal_whitespace(&mut self) -> Result<(), PslError> {
        while self
            .peek_byte()
            .is_some_and(|byte| byte == b' ' || byte == b'\t')
        {
            self.cursor += 1;
        }
        if !matches!(self.peek_byte(), None | Some(b'\n' | b'\r'))
            && let Some(character) = self.peek_char()
            && (character.is_whitespace() || character.is_control())
        {
            return Err(self.error_here(PslErrorKind::UnexpectedToken, self.line));
        }
        Ok(())
    }

    /// Returns `self.input[start..end]` or an `UnexpectedToken` error when bounds are invalid.
    fn slice_input(&self, start: usize, end: usize, line: usize) -> Result<&'a str, PslError> {
        self.input
            .get(start..end)
            .ok_or_else(|| self.error_here(PslErrorKind::UnexpectedToken, line))
    }

    /// Evaluates the next character in a bare-word token, advancing the cursor when valid or returning `Ok(false)` at a delimiter.
    fn step_bare_word_char(
        &mut self,
        character: char,
        start: usize,
        line: usize,
    ) -> Result<bool, PslError> {
        match character {
            ' ' | '\t' | '\n' | '#' => Ok(false),
            '\r' if self.is_crlf() => Ok(false),
            '\r' => Err(self.error_here(PslErrorKind::UnexpectedToken, line)),
            '{' => Err(self.error_here(PslErrorKind::InvalidBlockHeader, line)),
            '"' | '\'' if self.has_assignment_prefix(start) => Ok(false),
            '}' | '"' | '\'' => Err(self.error_here(PslErrorKind::UnexpectedToken, line)),
            _ if character.is_whitespace() || character.is_control() => {
                Err(self.error_here(PslErrorKind::UnexpectedToken, line))
            }
            _ => {
                self.cursor += character.len_utf8();
                Ok(true)
            }
        }
    }

    /// Constructs an `UnterminatedString` error pointing to the opening quote at `quote_offset`.
    #[must_use]
    fn unterminated(&self, line: usize, quote_offset: usize) -> PslError {
        self.error_at(PslErrorKind::UnterminatedString, line, quote_offset)
    }

    /// Validates the byte immediately following a scanned identifier starting at `start`.
    fn validate_post_identifier(&self, start: usize, line: usize) -> Result<(), PslError> {
        match self.peek_byte() {
            None | Some(b' ' | b'\t' | b'\n' | b'#') => Ok(()),
            Some(b'\r') if self.is_crlf() => Ok(()),
            Some(b'{') => Err(self.error_here(PslErrorKind::InvalidBlockHeader, line)),
            Some(b'\r' | b'}' | b'"' | b'\'') => {
                Err(self.error_here(PslErrorKind::UnexpectedToken, line))
            }
            Some(_) => {
                if let Some(character) = self.peek_char()
                    && (character.is_whitespace() || character.is_control())
                {
                    return Err(self.error_here(PslErrorKind::UnexpectedToken, line));
                }
                Err(self.error_at(PslErrorKind::InvalidIdentifier, line, start))
            }
        }
    }

    /// Validates that a closing quote is followed by whitespace, a comment, a newline, or EOF.
    fn validate_post_quote(&self, line: usize) -> Result<(), PslError> {
        match self.peek_byte() {
            None | Some(b' ' | b'\t' | b'\n' | b'#') => Ok(()),
            Some(b'\r') if self.is_crlf() => Ok(()),
            Some(b'{') => Err(self.error_here(PslErrorKind::InvalidBlockHeader, line)),
            Some(_) => Err(self.error_here(PslErrorKind::UnexpectedToken, line)),
        }
    }
}

/// Maps an escaped byte inside a double-quoted string to its unescaped character.
#[inline]
const fn unescape_byte(byte: u8) -> Option<char> {
    match byte {
        b'"' => Some('"'),
        b'\\' => Some('\\'),
        b'n' => Some('\n'),
        b'r' => Some('\r'),
        b't' => Some('\t'),
        _ => None,
    }
}
