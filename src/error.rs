//! Central domain error types for Podoxide.

use std::error::Error;
use std::fmt::{self, Display, Formatter, Write};
use std::io;

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

fn clamp_utf8_prefix(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !input.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &input[..boundary]
}

const fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

const fn sanitize_char(character: char) -> char {
    if character.is_control() || is_bidi_control(character) {
        return ' ';
    }
    character
}

fn sanitize_error_token(raw: &str) -> String {
    let clamped = clamp_utf8_prefix(raw, MAX_ERROR_TOKEN_BYTES);
    let mut sanitized = String::with_capacity(clamped.len());
    for character in clamped.chars() {
        sanitized.push(sanitize_char(character));
    }
    sanitized
}

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
        PslError, PslErrorKind, SystemError,
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
        let bounded_arg = bounded_cli.argument.as_deref().unwrap();
        assert!(bounded_arg.len() <= MAX_ERROR_TOKEN_BYTES);
        assert!(bounded_arg.ends_with('ä'));

        let bounded_config = ConfigError::with_token(ConfigErrorKind::UnknownDirective, 1, &multi_byte);
        let bounded_tok = bounded_config.token.as_deref().unwrap();
        assert!(bounded_tok.len() <= MAX_ERROR_TOKEN_BYTES);
        assert!(bounded_tok.ends_with('ä'));

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
