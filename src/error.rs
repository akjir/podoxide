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

//! Central domain error types for Podoxide.

use std::error::Error;
use std::fmt::{self, Display, Formatter, Write};
use std::io;

use crate::utilities::string::{clamp_utf8_prefix, sanitize_char};

/// Maximum byte length stored or formatted for an offending CLI argument or PSL token.
pub const MAX_ERROR_TOKEN_BYTES: usize = 256;

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
            argument: Some(sanitize_error_token(argument.as_ref())),
            kind,
        }
    }
}

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.kind)?;
        if let Some(argument) = &self.argument {
            formatter.write_str(": '")?;
            write_sanitized_token(formatter, argument)?;
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

/// Configuration semantic validation error (`pods.conf`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigError {
    pub kind: ConfigErrorKind,
    pub line: Option<usize>,
    pub token: Option<String>,
}

impl ConfigError {
    /// Creates a new configuration error with an optional 1-based line number.
    #[must_use]
    pub const fn new(kind: ConfigErrorKind, line: Option<usize>) -> Self {
        Self {
            kind,
            line,
            token: None,
        }
    }

    /// Creates a new configuration error associated with a 1-based line number and offending token.
    #[must_use]
    pub fn with_token(kind: ConfigErrorKind, line: usize, token: impl AsRef<str>) -> Self {
        Self {
            kind,
            line: Some(line),
            token: Some(sanitize_error_token(token.as_ref())),
        }
    }
}

impl Display for ConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(formatter, "line {line}: {}", self.kind)?;
        } else {
            write!(formatter, "{}", self.kind)?;
        }
        if let Some(token) = &self.token {
            formatter.write_str(": '")?;
            write_sanitized_token(formatter, token)?;
            formatter.write_char('\'')?;
        }
        Ok(())
    }
}

impl Error for ConfigError {}

/// Specific semantic validation failure encountered while validating `pods.conf`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigErrorKind {
    DuplicateBlock,
    DuplicateDirective,
    EmptyGroupsBlock,
    InvalidArgumentCount,
    InvalidBoolean,
    InvalidGroupTarget,
    InvalidPath,
    MissingGroupsBlock,
    MissingRecipesBlock,
    NotFound,
    UnexpectedBlock,
    UnknownDirective,
}

impl Display for ConfigErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::DuplicateBlock => "duplicate block in configuration",
            Self::DuplicateDirective => "duplicate directive in configuration",
            Self::EmptyGroupsBlock => "no recipe groups defined in 'groups' block",
            Self::InvalidArgumentCount => "invalid number of arguments for block or directive",
            Self::InvalidBoolean => "invalid boolean value (expected 'true' or 'false')",
            Self::InvalidGroupTarget => "invalid target in recipe group",
            Self::InvalidPath => "invalid path value in configuration",
            Self::MissingGroupsBlock => "missing 'groups' block in 'recipes'",
            Self::MissingRecipesBlock => "missing 'recipes' block in configuration",
            Self::NotFound => "configuration file not found",
            Self::UnexpectedBlock => "unexpected block in configuration",
            Self::UnknownDirective => "unknown directive in configuration",
        };
        formatter.write_str(message)
    }
}

/// Top-level error type encompassing all failure modes in Podoxide.
#[derive(Debug)]
pub enum PodError {
    Cli(CliError),
    Config(ConfigError),
    Io(io::Error),
    Psl(PslError),
    Recipe(RecipeError),
    System(SystemError),
}

impl Display for PodError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "CLI error: {error}"),
            Self::Config(error) => write!(formatter, "Config error: {error}"),
            Self::Io(error) => {
                formatter.write_str("I/O error: ")?;
                write_sanitized_token(formatter, &error.to_string())
            }
            Self::Psl(error) => write!(formatter, "PSL error: {error}"),
            Self::Recipe(error) => write!(formatter, "Recipe error: {error}"),
            Self::System(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for PodError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cli(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Psl(error) => Some(error),
            Self::Recipe(error) => Some(error),
            Self::System(error) => Some(error),
        }
    }
}

impl From<CliError> for PodError {
    fn from(error: CliError) -> Self {
        Self::Cli(error)
    }
}

impl From<ConfigError> for PodError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<PslError> for PodError {
    fn from(error: PslError) -> Self {
        Self::Psl(error)
    }
}

impl From<RecipeError> for PodError {
    fn from(error: RecipeError) -> Self {
        Self::Recipe(error)
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

/// Recipe semantic validation or target container resolution error (`*.pod`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipeError {
    pub kind: RecipeErrorKind,
    pub line: Option<usize>,
    pub token: Option<String>,
}

impl RecipeError {
    /// Creates a new recipe error associated with a 1-based line number and offending token.
    #[must_use]
    pub fn with_token(kind: RecipeErrorKind, line: usize, token: impl AsRef<str>) -> Self {
        Self {
            kind,
            line: Some(line),
            token: Some(sanitize_error_token(token.as_ref())),
        }
    }
}

impl Display for RecipeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(formatter, "line {line}: {}", self.kind)?;
        } else {
            write!(formatter, "{}", self.kind)?;
        }
        if let Some(token) = &self.token {
            formatter.write_str(": '")?;
            write_sanitized_token(formatter, token)?;
            formatter.write_char('\'')?;
        }
        Ok(())
    }
}

impl Error for RecipeError {}

/// Specific failure category encountered while loading, validating, or resolving a `.pod` recipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecipeErrorKind {
    ContainerNotFound,
    DuplicateBlock,
    DuplicateDirective,
    EmptyContainerBlock,
    EmptyPodBlock,
    InvalidArgumentCount,
    InvalidBoolean,
    InvalidCommand,
    InvalidContainerName,
    InvalidDescription,
    InvalidImage,
    InvalidOption,
    InvalidPath,
    InvalidPodId,
    InvalidPublish,
    InvalidRegistry,
    InvalidRestartPolicy,
    InvalidVolume,
    MissingContainerImage,
    MissingContainersBlock,
    MissingPodBlock,
    MissingPodName,
    MissingPodPath,
    MissingPodRegistry,
    NotFound,
    UnexpectedBlock,
    UnknownDirective,
}

impl Display for RecipeErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::ContainerNotFound => "container not found in recipe",
            Self::DuplicateBlock => "duplicate block in recipe",
            Self::DuplicateDirective => "duplicate directive in recipe",
            Self::EmptyContainerBlock => "container block in recipe is empty",
            Self::EmptyPodBlock => "pod block in recipe is empty",
            Self::InvalidArgumentCount => "invalid number of arguments for block or directive",
            Self::InvalidBoolean => "invalid boolean value (expected 'true' or 'false')",
            Self::InvalidCommand => "invalid command definition in recipe",
            Self::InvalidContainerName => "invalid container name in recipe",
            Self::InvalidDescription => "invalid description in recipe",
            Self::InvalidImage => "invalid container image in recipe",
            Self::InvalidOption => "invalid option value in recipe",
            Self::InvalidPath => "invalid path value in recipe",
            Self::InvalidPodId => "invalid pod identifier in recipe",
            Self::InvalidPublish => "invalid port publish specification in recipe",
            Self::InvalidRegistry => "invalid registry in recipe",
            Self::InvalidRestartPolicy => "invalid restart policy in recipe",
            Self::InvalidVolume => "invalid volume mount in recipe",
            Self::MissingContainerImage => "missing 'image' directive in container",
            Self::MissingContainersBlock => "no 'container' blocks defined in recipe",
            Self::MissingPodBlock => "missing 'pod' block in recipe",
            Self::MissingPodName => "missing 'name' directive in 'pod' block",
            Self::MissingPodPath => {
                "missing 'path' directive in 'pod' block and no default 'pods.path' configured"
            }
            Self::MissingPodRegistry => "missing 'registry' directive in 'pod' block",
            Self::NotFound => "recipe file not found",
            Self::UnexpectedBlock => "unexpected block in recipe",
            Self::UnknownDirective => "unknown directive in recipe",
        };
        formatter.write_str(message)
    }
}

/// Host operating system, runtime dependency, or privilege verification error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemError {
    AbortedExecution,
    CommandFailed(Option<i32>),
    UnsupportedOs,
    UnsupportedPodmanVersion,
}

impl Display for SystemError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbortedExecution => formatter.write_str("Aborting execution."),
            Self::CommandFailed(Some(code)) => {
                write!(formatter, "Command exited with code '{code}'!")
            }
            Self::CommandFailed(None) => formatter.write_str("Command terminated by signal!"),
            Self::UnsupportedOs => formatter.write_str("Only Linux is supported."),
            Self::UnsupportedPodmanVersion => {
                formatter.write_str("Podman 5.8.0 or higher is required.")
            }
        }
    }
}

impl Error for SystemError {}

/// Clamps `raw` to [`MAX_ERROR_TOKEN_BYTES`] and replaces all control and bidirectional characters with spaces.
fn sanitize_error_token(raw: &str) -> String {
    let clamped = clamp_utf8_prefix(raw, MAX_ERROR_TOKEN_BYTES);
    let mut sanitized = String::with_capacity(clamped.len());
    for character in clamped.chars() {
        sanitized.push(sanitize_char(character));
    }
    sanitized
}

/// Writes `raw` to `formatter`, clamped to [`MAX_ERROR_TOKEN_BYTES`] and sanitized of control characters.
fn write_sanitized_token(formatter: &mut Formatter<'_>, raw: &str) -> fmt::Result {
    let clamped = clamp_utf8_prefix(raw, MAX_ERROR_TOKEN_BYTES);
    for character in clamped.chars() {
        formatter.write_char(sanitize_char(character))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;

    use super::{
        CliError, CliErrorKind, ConfigError, ConfigErrorKind, MAX_ERROR_TOKEN_BYTES, PodError,
        PslError, PslErrorKind, RecipeError, RecipeErrorKind, SystemError,
    };

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
    fn config_error_kind_display_messages() {
        let variants = [
            (ConfigErrorKind::DuplicateBlock, "duplicate block"),
            (ConfigErrorKind::DuplicateDirective, "duplicate directive"),
            (ConfigErrorKind::EmptyGroupsBlock, "no recipe groups"),
            (ConfigErrorKind::InvalidArgumentCount, "invalid number of arguments"),
            (ConfigErrorKind::InvalidBoolean, "invalid boolean"),
            (ConfigErrorKind::InvalidGroupTarget, "invalid target"),
            (ConfigErrorKind::InvalidPath, "invalid path"),
            (ConfigErrorKind::MissingGroupsBlock, "missing 'groups'"),
            (ConfigErrorKind::MissingRecipesBlock, "missing 'recipes'"),
            (ConfigErrorKind::NotFound, "configuration file not found"),
            (ConfigErrorKind::UnexpectedBlock, "unexpected block"),
            (ConfigErrorKind::UnknownDirective, "unknown directive"),
        ];

        for (index, (kind, expected_substring)) in variants.iter().enumerate() {
            let plain = ConfigError::new(*kind, None);
            assert!(plain.to_string().contains(expected_substring));

            let line_only = ConfigError::new(*kind, Some(index + 1));
            assert!(line_only.to_string().contains(expected_substring));
            assert!(line_only.to_string().contains("line "));

            let with_token = ConfigError::with_token(*kind, index + 1, "bad_token");
            assert!(with_token.to_string().contains(expected_substring));
            assert!(with_token.to_string().contains("bad_token"));

            let token_no_line = ConfigError {
                kind: *kind,
                line: None,
                token: Some(String::from("bad_token")),
            };
            assert!(token_no_line.to_string().contains(expected_substring));
            assert!(token_no_line.to_string().contains("bad_token"));
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
    fn pod_error_config_display_and_source() {
        let config_error =
            ConfigError::with_token(ConfigErrorKind::UnknownDirective, 7, "unknown_key");
        let pod_error = PodError::from(config_error.clone());

        assert!(pod_error.to_string().contains("Config error"));
        assert!(pod_error.to_string().contains("line 7"));
        assert!(pod_error.to_string().contains("unknown_key"));
        assert!(pod_error.source().is_some());
        assert!(config_error.source().is_none());
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
    fn pod_error_recipe_display_and_source() {
        let recipe_error =
            RecipeError::with_token(RecipeErrorKind::UnknownDirective, 9, "unknown_key");
        let pod_error = PodError::from(recipe_error.clone());

        assert!(pod_error.to_string().contains("Recipe error"));
        assert!(pod_error.to_string().contains("line 9"));
        assert!(pod_error.to_string().contains("unknown_key"));
        assert!(pod_error.source().is_some());
        assert!(recipe_error.source().is_none());
    }

    #[test]
    fn pod_error_system_display_and_source() {
        let variants = [
            (SystemError::AbortedExecution, "Aborting execution."),
            (
                SystemError::CommandFailed(Some(125)),
                "Command exited with code '125'!",
            ),
            (
                SystemError::CommandFailed(None),
                "Command terminated by signal!",
            ),
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

    #[test]
    fn recipe_error_kind_display_messages() {
        let variants = [
            (RecipeErrorKind::ContainerNotFound, "container not found"),
            (RecipeErrorKind::DuplicateBlock, "duplicate block"),
            (RecipeErrorKind::DuplicateDirective, "duplicate directive"),
            (
                RecipeErrorKind::EmptyContainerBlock,
                "container block in recipe is empty",
            ),
            (
                RecipeErrorKind::EmptyPodBlock,
                "pod block in recipe is empty",
            ),
            (
                RecipeErrorKind::InvalidArgumentCount,
                "invalid number of arguments",
            ),
            (RecipeErrorKind::InvalidBoolean, "invalid boolean"),
            (RecipeErrorKind::InvalidCommand, "invalid command"),
            (
                RecipeErrorKind::InvalidContainerName,
                "invalid container name",
            ),
            (
                RecipeErrorKind::InvalidDescription,
                "invalid description in recipe",
            ),
            (RecipeErrorKind::InvalidImage, "invalid container image"),
            (RecipeErrorKind::InvalidOption, "invalid option"),
            (RecipeErrorKind::InvalidPath, "invalid path"),
            (RecipeErrorKind::InvalidPodId, "invalid pod identifier"),
            (
                RecipeErrorKind::InvalidPublish,
                "invalid port publish specification",
            ),
            (RecipeErrorKind::InvalidRegistry, "invalid registry"),
            (
                RecipeErrorKind::InvalidRestartPolicy,
                "invalid restart policy",
            ),
            (RecipeErrorKind::InvalidVolume, "invalid volume mount"),
            (
                RecipeErrorKind::MissingContainerImage,
                "missing 'image' directive",
            ),
            (
                RecipeErrorKind::MissingContainersBlock,
                "no 'container' blocks",
            ),
            (RecipeErrorKind::MissingPodBlock, "missing 'pod' block"),
            (
                RecipeErrorKind::MissingPodName,
                "missing 'name' directive",
            ),
            (
                RecipeErrorKind::MissingPodPath,
                "missing 'path' directive",
            ),
            (
                RecipeErrorKind::MissingPodRegistry,
                "missing 'registry' directive",
            ),
            (RecipeErrorKind::NotFound, "recipe file not found"),
            (RecipeErrorKind::UnexpectedBlock, "unexpected block"),
            (RecipeErrorKind::UnknownDirective, "unknown directive"),
        ];

        for (index, (kind, expected_substring)) in variants.iter().enumerate() {
            let plain = RecipeError {
                kind: *kind,
                line: None,
                token: None,
            };
            assert!(plain.to_string().contains(expected_substring));
            let line_only = RecipeError {
                kind: *kind,
                line: Some(index + 1),
                token: None,
            };
            assert!(line_only.to_string().contains(expected_substring));
            assert!(line_only.to_string().contains("line "));
            let with_token = RecipeError::with_token(*kind, index + 1, "bad_token");
            assert!(with_token.to_string().contains(expected_substring));
            assert!(with_token.to_string().contains("bad_token"));
            let token_no_line = RecipeError {
                kind: *kind,
                line: None,
                token: Some(String::from("bad_token")),
            };
            assert!(token_no_line.to_string().contains(expected_substring));
            assert!(token_no_line.to_string().contains("bad_token"));
        }
    }

    #[test]
    fn red_team_control_escape_bidi_and_oversized_token_bounds() {
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

        let config_error = ConfigError::with_token(ConfigErrorKind::InvalidPath, 3, injected);
        let config_token = config_error.token.as_deref().unwrap();
        assert!(!config_token.contains('\n'));
        assert!(!config_token.contains('\r'));
        assert!(!config_token.contains('\x1b'));
        assert!(!config_token.contains('\0'));
        assert!(!config_token.contains('\u{009b}'));
        assert!(!config_token.contains('\u{202e}'));

        let direct_cli = CliError {
            argument: Some(String::from(injected)),
            kind: CliErrorKind::InvalidTarget,
        };
        let direct_cli_display = direct_cli.to_string();
        assert_eq!(direct_cli_display.lines().count(), 1);
        assert!(!direct_cli_display.contains('\x1b'));
        assert!(!direct_cli_display.contains('\0'));
        assert!(!direct_cli_display.contains('\u{202e}'));

        let direct_config = ConfigError {
            kind: ConfigErrorKind::InvalidGroupTarget,
            line: Some(5),
            token: Some(String::from(injected)),
        };
        let direct_config_display = direct_config.to_string();
        assert_eq!(direct_config_display.lines().count(), 1);
        assert!(!direct_config_display.contains('\x1b'));
        assert!(!direct_config_display.contains('\0'));
        assert!(!direct_config_display.contains('\u{202e}'));

        let io_error = PodError::from(io::Error::new(io::ErrorKind::InvalidData, injected));
        let io_display = io_error.to_string();
        assert_eq!(io_display.lines().count(), 1);
        assert!(!io_display.contains('\x1b'));
        assert!(!io_display.contains('\0'));
        assert!(!io_display.contains('\u{202e}'));

        let multi_byte = format!("x{}", "ä".repeat((MAX_ERROR_TOKEN_BYTES / 2) + 20));
        let bounded_cli = CliError::with_argument(CliErrorKind::UnknownFlag, &multi_byte);
        let bounded_argument = bounded_cli.argument.as_deref().unwrap();
        assert!(bounded_argument.len() <= MAX_ERROR_TOKEN_BYTES);
        assert!(bounded_argument.ends_with('ä'));

        let bounded_config =
            ConfigError::with_token(ConfigErrorKind::UnknownDirective, 1, &multi_byte);
        let bounded_token = bounded_config.token.as_deref().unwrap();
        assert!(bounded_token.len() <= MAX_ERROR_TOKEN_BYTES);
        assert!(bounded_token.ends_with('ä'));

        let recipe_error = RecipeError::with_token(RecipeErrorKind::InvalidPath, 4, injected);
        let recipe_token = recipe_error.token.as_deref().unwrap();
        assert!(!recipe_token.contains('\n'));
        assert!(!recipe_token.contains('\r'));
        assert!(!recipe_token.contains('\x1b'));
        assert!(!recipe_token.contains('\0'));
        assert!(!recipe_token.contains('\u{009b}'));
        assert!(!recipe_token.contains('\u{202e}'));

        let bounded_recipe =
            RecipeError::with_token(RecipeErrorKind::UnknownDirective, 1, &multi_byte);
        let bounded_recipe_token = bounded_recipe.token.as_deref().unwrap();
        assert!(bounded_recipe_token.len() <= MAX_ERROR_TOKEN_BYTES);
        assert!(bounded_recipe_token.ends_with('ä'));

        let direct_oversized = ConfigError {
            kind: ConfigErrorKind::UnknownDirective,
            line: None,
            token: Some(multi_byte),
        };
        let formatted_oversized = direct_oversized.to_string();
        assert!(formatted_oversized.len() <= MAX_ERROR_TOKEN_BYTES + 64);
        assert!(formatted_oversized.ends_with("ä'"));
    }
}
