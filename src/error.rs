//! Central domain error types for Podoxide.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

/// Top-level error type encompassing all failure modes in Podoxide.
#[derive(Debug)]
pub enum PodError {
    Io(io::Error),
    Psl(PslError),
}

impl Display for PodError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Psl(error) => write!(formatter, "PSL error: {error}"),
        }
    }
}

impl Error for PodError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Psl(error) => Some(error),
        }
    }
}

impl From<PslError> for PodError {
    fn from(error: PslError) -> Self {
        Self::Psl(error)
    }
}

impl From<io::Error> for PodError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Syntax or resource limit error encountered while parsing a PSL document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PslError {
    pub kind: PslErrorKind,
    pub line: usize,
}

impl PslError {
    /// Creates a new PSL error with the given error kind and 1-based line number.
    #[must_use]
    pub const fn new(kind: PslErrorKind, line: usize) -> Self {
        Self { kind, line }
    }
}

impl Display for PslError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "line {}: {}", self.line, self.kind)
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

impl Display for PslErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
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
        };
        formatter.write_str(message)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;

    use super::{PodError, PslError, PslErrorKind};

    #[test]
    fn pod_error_io_display_and_source() {
        let io_error = io::Error::new(io::ErrorKind::NotFound, "missing file");
        let pod_error = PodError::from(io_error);

        assert!(pod_error.to_string().contains("I/O error"));
        assert!(pod_error.to_string().contains("missing file"));
        assert!(pod_error.source().is_some());
    }

    #[test]
    fn pod_error_psl_display_and_source() {
        let psl_error = PslError::new(PslErrorKind::UnexpectedRootDirective, 4);
        let pod_error = PodError::from(psl_error);

        assert!(pod_error.to_string().contains("PSL error"));
        assert!(pod_error.to_string().contains("line 4"));
        assert!(pod_error.to_string().contains("root level"));
        assert!(pod_error.source().is_some());
        assert!(psl_error.source().is_none());
    }

    #[test]
    fn psl_error_kind_display_messages() {
        let variants = [
            (PslErrorKind::InvalidBlockHeader, "invalid block header"),
            (
                PslErrorKind::InvalidEscapeSequence,
                "invalid escape sequence",
            ),
            (PslErrorKind::InvalidIdentifier, "invalid identifier"),
            (PslErrorKind::LimitExceeded, "resource limit exceeded"),
            (
                PslErrorKind::MissingDirectiveArgument,
                "missing directive argument",
            ),
            (PslErrorKind::UnclosedBlock, "unclosed block"),
            (
                PslErrorKind::UnexpectedClosingBrace,
                "unexpected closing brace",
            ),
            (
                PslErrorKind::UnexpectedRootDirective,
                "unexpected directive at root level",
            ),
            (PslErrorKind::UnexpectedToken, "unexpected token"),
            (PslErrorKind::UnterminatedString, "unterminated string"),
        ];

        for (index, (kind, expected_substring)) in variants.iter().enumerate() {
            let error = PslError::new(*kind, index + 1);
            let formatted = error.to_string();
            assert!(formatted.contains(expected_substring));
        }
    }
}
