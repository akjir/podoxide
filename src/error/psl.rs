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

//! PodScript Language (PSL) syntax and resource limit error types.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::error::diagnostic::{self, DiagnosticError};
use crate::error::span;

/// Syntax or resource limit error encountered while parsing a PSL document.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PslError {
    pub column: Option<usize>,
    pub diagnostic: Option<Box<DiagnosticError<'static>>>,
    pub kind: PslErrorKind,
    pub line: usize,
}

impl PslError {
    /// Creates a new PSL error with the given error kind and 1-based line number.
    #[must_use]
    pub const fn new(kind: PslErrorKind, line: usize) -> Self {
        Self {
            column: None,
            diagnostic: None,
            kind,
            line: if line == 0 { 1 } else { line },
        }
    }

    /// Creates a new PSL error with the given error kind, 1-based line number, and 1-based column.
    #[must_use]
    pub const fn with_column(kind: PslErrorKind, line: usize, column: usize) -> Self {
        Self {
            column: Some(if column == 0 { 1 } else { column }),
            diagnostic: None,
            kind,
            line: if line == 0 { 1 } else { line },
        }
    }

    /// Attaches a compiler-style [`DiagnosticError`] resolved from `source` and `file_path`.
    #[must_use]
    pub fn with_diagnostic(mut self, source: &str, file_path: &str) -> Self {
        let kind = self.kind;
        let line_number = self.line.max(1);
        let column = self.column.map(|value| value.max(1));
        self.line = line_number;
        self.column = column;
        let built = diagnostic::build_diagnostic_from_line(
            source,
            file_path,
            line_number,
            kind.annotation_text(),
            kind.help_text(),
            |sanitized_line| span::locate_psl_error_span(sanitized_line, kind, column),
        );
        self.diagnostic = Some(Box::new(built));
        self
    }
}

impl Display for PslError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        diagnostic::write_structured_error(
            formatter,
            Some(self.line.max(1)),
            &self.kind,
            None,
            self.diagnostic.as_deref(),
        )
    }
}

impl Error for PslError {}

/// Specific lexical, grammatical, or resource limit failure in a PSL document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PslErrorKind {
    InvalidBlockHeader,
    InvalidEscapeSequence,
    InvalidIdentifier,
    LimitExceeded,
    MissingDirectiveArgument,
    UnclosedBlock,
    UnexpectedClosingBrace,
    UnexpectedRootDirective,
    UnexpectedToken,
    UnterminatedString,
}

impl PslErrorKind {
    /// Returns the inline caret annotation message for this PSL syntax error kind.
    #[must_use]
    pub const fn annotation_text(self) -> &'static str {
        match self {
            Self::InvalidBlockHeader => "missing whitespace before opening '{'",
            Self::InvalidEscapeSequence => "unsupported backslash escape sequence",
            Self::InvalidIdentifier => {
                "identifier must be an unquoted name matching '[A-Za-z_][A-Za-z0-9_-]*'"
            }
            Self::LimitExceeded => "PSL document exceeds maximum allowed resource limit",
            Self::MissingDirectiveArgument => "expected at least one argument or an opening '{'",
            Self::UnclosedBlock => "file ended before all open '{' blocks were closed",
            Self::UnexpectedClosingBrace => "unmatched '}' at root level",
            Self::UnexpectedRootDirective => "standalone directives are not allowed at root level",
            Self::UnexpectedToken => "unexpected token or character at this position",
            Self::UnterminatedString => "string literal is missing its closing quote on this line",
        }
    }

    /// Returns actionable remediation advice for this PSL syntax error kind.
    #[must_use]
    pub const fn help_text(self) -> &'static str {
        match self {
            Self::InvalidBlockHeader => {
                "insert a space or tab before '{' (for example: 'pod app {')."
            }
            Self::InvalidEscapeSequence => {
                "use a valid escape sequence (\\\", \\\\, \\n, \\r, \\t) or single quotes '...' for literal strings."
            }
            Self::InvalidIdentifier => {
                "start block and directive names with an ASCII letter or underscore without quotes."
            }
            Self::LimitExceeded => {
                "reduce file size, block nesting depth, block count, or arguments per statement."
            }
            Self::MissingDirectiveArgument => {
                "supply a value for the directive or add ' {' to open a block."
            }
            Self::UnclosedBlock => "add a matching '}' on its own line for every opened block.",
            Self::UnexpectedClosingBrace => {
                "remove this extra '}' or add a matching block header before it."
            }
            Self::UnexpectedRootDirective => {
                "place this directive inside a root block (such as 'general {', 'pods {', or 'pod <id> {')."
            }
            Self::UnexpectedToken => {
                "separate quoted strings with spaces and place statements on separate lines."
            }
            Self::UnterminatedString => {
                "close the string with a matching quote before the end of the line."
            }
        }
    }

    /// Returns the static high-level error summary for this PSL syntax error kind.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidBlockHeader => "invalid block header: missing whitespace before '{'",
            Self::InvalidEscapeSequence => "invalid escape sequence in double-quoted string",
            Self::InvalidIdentifier => "invalid identifier",
            Self::LimitExceeded => "resource limit exceeded",
            Self::MissingDirectiveArgument => "missing directive argument or opening '{'",
            Self::UnclosedBlock => "unclosed block: expected '}' before end of file",
            Self::UnexpectedClosingBrace => "unexpected closing brace '}' at root level",
            Self::UnexpectedRootDirective => {
                "unexpected directive at root level: only blocks are allowed"
            }
            Self::UnexpectedToken => "unexpected token",
            Self::UnterminatedString => "unterminated string literal",
        }
    }
}

impl Display for PslErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

#[cfg(test)]
mod tests {
    use crate::error::PodError;
    use super::{PslError, PslErrorKind};

    #[test]
    fn psl_error_kind_display_and_diagnostics() {
        let variants = [
            (
                PslErrorKind::InvalidBlockHeader,
                "invalid block header",
                "pod app{\n",
            ),
            (
                PslErrorKind::InvalidEscapeSequence,
                "invalid escape sequence",
                "    key \"bad\\xescape\"\n",
            ),
            (
                PslErrorKind::InvalidIdentifier,
                "invalid identifier",
                "    123bad value\n",
            ),
            (
                PslErrorKind::LimitExceeded,
                "resource limit exceeded",
                "block {\n",
            ),
            (
                PslErrorKind::MissingDirectiveArgument,
                "missing directive argument",
                "    editor\n",
            ),
            (PslErrorKind::UnclosedBlock, "unclosed block", "general {\n"),
            (
                PslErrorKind::UnexpectedClosingBrace,
                "unexpected closing brace",
                "}\n",
            ),
            (
                PslErrorKind::UnexpectedRootDirective,
                "unexpected directive at root level",
                "editor vim\n",
            ),
            (
                PslErrorKind::UnexpectedToken,
                "unexpected token",
                "general {} trailing\n",
            ),
            (
                PslErrorKind::UnterminatedString,
                "unterminated string",
                "    key \"unclosed\n",
            ),
        ];

        for (index, (kind, expected_substring, sample_line)) in variants.iter().enumerate() {
            let error = PslError::new(*kind, index + 1);
            assert!(error.to_string().contains(expected_substring));

            let with_diagnostic =
                PslError::new(*kind, 1).with_diagnostic(sample_line, "./test.pod");
            let diagnostic = with_diagnostic.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.annotation_text, kind.annotation_text());
            let diagnostic_string = with_diagnostic.to_string();
            assert!(diagnostic_string.contains(expected_substring));
            assert!(diagnostic_string.contains("--> ./test.pod:1:"));
            assert!(diagnostic_string.contains("= help: "));
            assert!(PodError::from(with_diagnostic).diagnostic().is_some());
        }

        // Single-quoted literal backslash preceding invalid double-quoted escape
        let single_then_bad_escape = PslError::new(PslErrorKind::InvalidEscapeSequence, 1)
            .with_diagnostic(
                "    command 'C:\\path\\ok' \"bad\\xescape\"\n",
                "./test.pod",
            );
        let escape_diagnostic = single_then_bad_escape.diagnostic.as_ref().unwrap();
        assert_eq!(escape_diagnostic.col_number, 30);
        assert_eq!(
            escape_diagnostic.annotation_carets,
            "                             ^^"
        );

        // Multi-byte UTF-8 character after backslash inside double-quoted string
        let utf8_bad_escape = PslError::new(PslErrorKind::InvalidEscapeSequence, 1)
            .with_diagnostic("    key \"bad\\äescape\"\n", "./test.pod");
        let utf8_diagnostic = utf8_bad_escape.diagnostic.as_ref().unwrap();
        assert_eq!(utf8_diagnostic.col_number, 13);
        assert_eq!(utf8_diagnostic.annotation_carets, "            ^^");

        // Unterminated double-quoted string containing single quote and escaped double quote
        let unterminated_with_inner_quotes = PslError::new(PslErrorKind::UnterminatedString, 1)
            .with_diagnostic("    key 'ok' \"Don't \\\"close\n", "./test.pod");
        let unterminated_diagnostic = unterminated_with_inner_quotes.diagnostic.as_ref().unwrap();
        assert_eq!(unterminated_diagnostic.col_number, 14);
        assert_eq!(
            unterminated_diagnostic.annotation_carets,
            "             ^^^^^^^^^^^^^^"
        );

        // Unquoted structural brace after quoted brace
        let quoted_brace_header = PslError::new(PslErrorKind::InvalidBlockHeader, 1)
            .with_diagnostic("pod \"{app}\"{\n", "./test.pod");
        let brace_diagnostic = quoted_brace_header.diagnostic.as_ref().unwrap();
        assert_eq!(brace_diagnostic.col_number, 12);
        assert_eq!(brace_diagnostic.annotation_carets, "           ^");

        // Explicit column overrides line-wide fallback for UnexpectedToken and InvalidBlockHeader
        let trailing_token = PslError::with_column(PslErrorKind::UnexpectedToken, 1, 12)
            .with_diagnostic("pod app {} trailing\n", "./test.pod");
        let trailing_diagnostic = trailing_token.diagnostic.as_ref().unwrap();
        assert_eq!(trailing_diagnostic.col_number, 12);
        assert_eq!(trailing_diagnostic.annotation_carets, "           ^^^^^^^^");

        let first_bad_brace = PslError::new(PslErrorKind::InvalidBlockHeader, 1)
            .with_diagnostic("pod app{ other {\n", "./test.pod");
        let first_brace_diagnostic = first_bad_brace.diagnostic.as_ref().unwrap();
        assert_eq!(first_brace_diagnostic.col_number, 8);
        assert_eq!(first_brace_diagnostic.annotation_carets, "       ^");
    }

    #[test]
    fn psl_error_line_zero_normalizes_consistently_with_diagnostic_header() {
        let constructed = PslError::new(PslErrorKind::LimitExceeded, 0)
            .with_diagnostic("general {\n}\n", "./pods.conf");
        let rendered = constructed.to_string();
        assert_eq!(constructed.line, 1);
        assert_eq!(constructed.column, None);
        assert!(!rendered.contains("line 0:"));
        assert!(rendered.contains("line 1:"));
        assert!(rendered.contains("--> ./pods.conf:1:"));

        let with_zero_column = PslError::with_column(PslErrorKind::UnexpectedToken, 0, 0)
            .with_diagnostic("general {}\n", "./pods.conf");
        assert_eq!(with_zero_column.line, 1);
        assert_eq!(with_zero_column.column, Some(1));

        let manual = PslError {
            column: Some(0),
            diagnostic: None,
            kind: PslErrorKind::LimitExceeded,
            line: 0,
        };
        assert!(manual.to_string().starts_with("line 1:"));
        let manual_diagnostic = manual.with_diagnostic("general {}\n", "./pods.conf");
        assert_eq!(manual_diagnostic.column, Some(1));
    }
}
