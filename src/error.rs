//! Central domain error types for Podoxide.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

/// Command-line argument parsing or validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliError {
    pub argument: Option<String>,
    pub kind: CliErrorKind,
}

impl CliError {
    /// Creates a new CLI error without an associated argument token.
    #[must_use]
    pub const fn new(kind: CliErrorKind) -> Self {
        Self {
            argument: None,
            kind,
        }
    }

    /// Creates a new CLI error associated with the offending argument token.
    #[must_use]
    pub fn with_argument(kind: CliErrorKind, argument: impl Into<String>) -> Self {
        Self {
            argument: Some(argument.into()),
            kind,
        }
    }
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if let Some(argument) = &self.argument {
            write!(formatter, "{}: '{argument}'", self.kind)
        } else {
            write!(formatter, "{}", self.kind)
        }
    }
}

impl Error for CliError {}

/// Specific failure category encountered while parsing CLI arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CliErrorKind {
    EmptyFlagValue,
    InvalidFlagValue,
    InvalidTarget,
    InvalidUtf8,
    LimitExceeded,
    MissingFlagValue,
    MissingTarget,
    TooManyArguments,
    UnexpectedFlag,
    UnknownAction,
    UnknownFlag,
}

impl Display for CliErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::EmptyFlagValue => "empty value for command-line flag",
            Self::InvalidFlagValue => "invalid value for command-line flag",
            Self::InvalidTarget => "invalid target specification",
            Self::InvalidUtf8 => "command-line argument is not valid UTF-8",
            Self::LimitExceeded => "command-line argument limit exceeded",
            Self::MissingFlagValue => "missing value for command-line flag",
            Self::MissingTarget => "missing required target argument",
            Self::TooManyArguments => "too many positional arguments",
            Self::UnexpectedFlag => "flag is not valid for the selected mode",
            Self::UnknownAction => "unknown mode or action",
            Self::UnknownFlag => "unknown command-line flag",
        };
        formatter.write_str(message)
    }
}

/// Top-level error type encompassing all failure modes in Podoxide.
#[derive(Debug)]
pub enum PodError {
    Cli(CliError),
    Io(io::Error),
    Psl(PslError),
    System(SystemError),
}

impl Display for PodError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "CLI error: {error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Psl(error) => write!(formatter, "PSL error: {error}"),
            Self::System(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for PodError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cli(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Psl(error) => Some(error),
            Self::System(error) => Some(error),
        }
    }
}

impl From<CliError> for PodError {
    fn from(error: CliError) -> Self {
        Self::Cli(error)
    }
}

impl From<PslError> for PodError {
    fn from(error: PslError) -> Self {
        Self::Psl(error)
    }
}

impl From<SystemError> for PodError {
    fn from(error: SystemError) -> Self {
        Self::System(error)
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

/// Host operating system, runtime dependency, or privilege verification error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemError {
    AbortedExecution,
    UnsupportedOs,
    UnsupportedPodmanVersion,
}

impl Display for SystemError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::AbortedExecution => "Aborting execution.",
            Self::UnsupportedOs => "Only Linux is supported.",
            Self::UnsupportedPodmanVersion => "Podman 5.8.0 or higher is required.",
        };
        formatter.write_str(message)
    }
}

impl Error for SystemError {}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;

    use super::{CliError, CliErrorKind, PodError, PslError, PslErrorKind, SystemError};

    #[test]
    fn cli_error_kind_display_messages() {
        let variants = [
            (CliErrorKind::EmptyFlagValue, "empty value"),
            (CliErrorKind::InvalidFlagValue, "invalid value"),
            (CliErrorKind::InvalidTarget, "invalid target"),
            (CliErrorKind::InvalidUtf8, "not valid UTF-8"),
            (CliErrorKind::LimitExceeded, "limit exceeded"),
            (CliErrorKind::MissingFlagValue, "missing value"),
            (CliErrorKind::MissingTarget, "missing required target"),
            (CliErrorKind::TooManyArguments, "too many positional"),
            (CliErrorKind::UnexpectedFlag, "not valid for the selected mode"),
            (CliErrorKind::UnknownAction, "unknown mode or action"),
            (CliErrorKind::UnknownFlag, "unknown command-line flag"),
        ];

        for (kind, expected_substring) in variants {
            let plain_error = CliError::new(kind);
            assert!(plain_error.to_string().contains(expected_substring));

            let argument_error = CliError::with_argument(kind, "--bad");
            assert!(argument_error.to_string().contains(expected_substring));
            assert!(argument_error.to_string().contains("--bad"));
        }
    }

    #[test]
    fn pod_error_cli_display_and_source() {
        let cli_error = CliError::with_argument(CliErrorKind::UnknownFlag, "--unknown");
        let pod_error = PodError::from(cli_error.clone());

        assert!(pod_error.to_string().contains("CLI error"));
        assert!(pod_error.to_string().contains("--unknown"));
        assert!(pod_error.source().is_some());
        assert!(cli_error.source().is_none());
    }

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
    fn pod_error_system_display_and_source() {
        let variants = [
            (SystemError::AbortedExecution, "Aborting execution."),
            (SystemError::UnsupportedOs, "Only Linux is supported."),
            (
                SystemError::UnsupportedPodmanVersion,
                "Podman 5.8.0 or higher is required.",
            ),
        ];

        for (system_error, expected_message) in variants {
            assert_eq!(system_error.to_string(), expected_message);
            assert!(system_error.source().is_none());

            let pod_error = PodError::from(system_error);
            assert_eq!(pod_error.to_string(), expected_message);
            assert!(pod_error.source().is_some());
        }
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
