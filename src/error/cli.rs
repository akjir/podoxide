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

//! Command-line argument parsing and validation error types.

use std::error::Error;
use std::fmt::{self, Display, Formatter, Write};

use crate::error::MAX_ERROR_TOKEN_BYTES;
use crate::error::diagnostic;
use crate::utilities::string;

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
    pub fn with_argument(kind: CliErrorKind, argument: impl AsRef<str>) -> Self {
        Self {
            argument: Some(string::sanitize_text(
                argument.as_ref(),
                MAX_ERROR_TOKEN_BYTES,
            )),
            kind,
        }
    }
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.kind)?;
        if let Some(argument) = &self.argument {
            formatter.write_str(": '")?;
            diagnostic::write_sanitized_token(formatter, argument)?;
            formatter.write_char('\'')?;
        }
        Ok(())
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
    TargetNotFound,
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
            Self::TargetNotFound => "target does not exist",
            Self::TooManyArguments => "too many positional arguments",
            Self::UnexpectedFlag => "flag is not valid for the selected mode",
            Self::UnknownAction => "unknown mode or action",
            Self::UnknownFlag => "unknown command-line flag",
        };
        formatter.write_str(message)
    }
}

#[cfg(test)]
mod tests {
    use crate::error::{MAX_ERROR_TOKEN_BYTES, PodError};
    use super::{CliError, CliErrorKind};

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
            (CliErrorKind::TargetNotFound, "target does not exist"),
            (CliErrorKind::TooManyArguments, "too many positional"),
            (
                CliErrorKind::UnexpectedFlag,
                "not valid for the selected mode",
            ),
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
    fn red_team_cli_error_sanitization_and_utf8_bounds() {
        let injected =
            "bad\nINFO: Forged\rWARNING: Overwrite\x1b[31mANSI\0null\u{009b}CSI\u{202e}RLO";
        let cli_error = CliError::with_argument(CliErrorKind::InvalidFlagValue, injected);
        let cli_argument = cli_error.argument.as_deref().unwrap();
        assert!(!cli_argument.contains('\n'));
        assert!(!cli_argument.contains('\r'));
        assert!(!cli_argument.contains('\x1b'));
        assert!(!cli_argument.contains('\0'));
        assert!(!cli_argument.contains('\u{009b}'));
        assert!(!cli_argument.contains('\u{202e}'));

        let cli_display = PodError::from(cli_error).to_string();
        assert_eq!(cli_display.lines().count(), 1);
        assert!(!cli_display.contains('\x1b'));
        assert!(!cli_display.contains('\u{202e}'));

        let direct_cli = CliError {
            argument: Some(String::from(injected)),
            kind: CliErrorKind::InvalidTarget,
        };
        let direct_display = direct_cli.to_string();
        assert_eq!(direct_display.lines().count(), 1);
        assert!(!direct_display.contains('\x1b'));
        assert!(!direct_display.contains('\0'));
        assert!(!direct_display.contains('\u{202e}'));

        let multi_byte = format!("x{}", "ä".repeat((MAX_ERROR_TOKEN_BYTES / 2) + 20));
        let bounded_cli = CliError::with_argument(CliErrorKind::UnknownFlag, &multi_byte);
        let bounded_argument = bounded_cli.argument.as_deref().unwrap();
        assert!(bounded_argument.len() <= MAX_ERROR_TOKEN_BYTES);
        assert!(bounded_argument.ends_with('ä'));
    }
}
