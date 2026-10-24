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

//! Central domain error and compiler-style diagnostic types for Podoxide.

use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Display, Formatter, Write};
use std::io;
use std::str::Chars;

use crate::utilities::string::{clamp_utf8_prefix, sanitize_char, sanitize_text};

/// Maximum byte length stored or formatted for an offending CLI argument, PSL token, or line snippet.
pub const MAX_ERROR_TOKEN_BYTES: usize = 256;
/// Static buffer of carets for zero-allocation caret borrowing on typical line lengths.
const STATIC_CARETS: &str =
    "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^";

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
            argument: Some(sanitize_text(argument.as_ref(), MAX_ERROR_TOKEN_BYTES)),
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
    pub diagnostic: Option<Box<DiagnosticError<'static>>>,
    pub kind: ConfigErrorKind,
    pub line: Option<usize>,
    pub token: Option<String>,
}

impl ConfigError {
    /// Creates a new configuration error with an optional 1-based line number.
    #[must_use]
    pub const fn new(kind: ConfigErrorKind, line: Option<usize>) -> Self {
        Self {
            diagnostic: None,
            kind,
            line,
            token: None,
        }
    }

    /// Attaches a compiler-style [`DiagnosticError`] resolved from `source` and `file_path`.
    #[must_use]
    pub fn with_diagnostic(mut self, source: &str, file_path: &str) -> Self {
        let line_number = self.line.unwrap_or(1).max(1);
        let diagnostic = build_line_diagnostic(
            source,
            file_path,
            line_number,
            self.token.as_deref(),
            self.kind.message(),
            self.kind.annotation_text(),
            self.kind.help_text(),
        );
        self.diagnostic = Some(Box::new(diagnostic));
        self
    }

    /// Creates a new configuration error associated with a 1-based line number and offending token.
    #[must_use]
    pub fn with_token(kind: ConfigErrorKind, line: usize, token: impl AsRef<str>) -> Self {
        Self {
            diagnostic: None,
            kind,
            line: Some(line),
            token: Some(sanitize_text(token.as_ref(), MAX_ERROR_TOKEN_BYTES)),
        }
    }

    /// Creates a new configuration error associated with an offending token without a line number.
    #[must_use]
    pub fn without_line(kind: ConfigErrorKind, token: impl AsRef<str>) -> Self {
        Self {
            diagnostic: None,
            kind,
            line: None,
            token: Some(sanitize_text(token.as_ref(), MAX_ERROR_TOKEN_BYTES)),
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
        if let Some(diagnostic) = &self.diagnostic {
            formatter.write_char('\n')?;
            diagnostic.write_body(formatter)?;
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
    MissingEditor,
    MissingGroupsBlock,
    MissingRecipesBlock,
    NotFound,
    RecipeNotFound,
    UnexpectedBlock,
    UnknownDirective,
    UnknownGroup,
}

impl ConfigErrorKind {
    /// Returns the inline caret annotation message for this configuration error kind.
    #[must_use]
    pub const fn annotation_text(self) -> &'static str {
        match self {
            Self::DuplicateBlock => "block is already defined earlier in this configuration",
            Self::DuplicateDirective => "directive is already defined in this block",
            Self::EmptyGroupsBlock => "the 'groups' block must define at least one recipe group",
            Self::InvalidArgumentCount => "wrong number of arguments for this block or directive",
            Self::InvalidBoolean => "expected boolean literal 'true' or 'false'",
            Self::InvalidGroupTarget => {
                "group target is malformed or forms a circular '@group' reference"
            }
            Self::InvalidPath => "path must be a safe directory or editor path without '..' or ':'",
            Self::MissingEditor => "no external editor is configured in 'general.editor'",
            Self::MissingGroupsBlock => "'recipes' block does not contain a 'groups' sub-block",
            Self::MissingRecipesBlock => "configuration is missing the required 'recipes' block",
            Self::NotFound => "configuration file was not found at this path",
            Self::RecipeNotFound => "recipe is not registered in any configuration group",
            Self::UnexpectedBlock => "block is not permitted at this location in 'pods.conf'",
            Self::UnknownDirective => "unrecognized directive in configuration block",
            Self::UnknownGroup => "recipe group is not defined in 'recipes.groups'",
        }
    }

    /// Returns actionable remediation advice for this configuration error kind.
    #[must_use]
    pub const fn help_text(self) -> &'static str {
        match self {
            Self::DuplicateBlock => "merge the settings into a single block definition.",
            Self::DuplicateDirective => "keep only one definition of this directive per block.",
            Self::EmptyGroupsBlock => {
                "add at least one group mapping inside 'groups { ... }' (for example: 'all recipe')."
            }
            Self::InvalidArgumentCount => {
                "configuration blocks take no arguments, and 'editor', 'simulate', and 'path' take 1 argument."
            }
            Self::InvalidBoolean => "set this directive to 'true' or 'false'.",
            Self::InvalidGroupTarget => {
                "use valid recipe names or acyclic '@group' references inside 'groups'."
            }
            Self::InvalidPath => {
                "provide a valid path without parent traversal ('..'), colons (':'), or flags."
            }
            Self::MissingEditor => {
                "set 'editor <command>' inside the 'general { ... }' block in 'pods.conf'."
            }
            Self::MissingGroupsBlock => {
                "add a 'groups { ... }' sub-block inside the 'recipes' block."
            }
            Self::MissingRecipesBlock => {
                "define a 'recipes { groups { ... } }' block in the configuration file."
            }
            Self::NotFound => {
                "run 'pods init' to create a default 'pods.conf' or pass '--config=<path>'."
            }
            Self::RecipeNotFound => {
                "register the recipe in a group under 'recipes { groups { ... } }' in 'pods.conf'."
            }
            Self::UnexpectedBlock => {
                "allowed root blocks are 'general', 'pods', and 'recipes' (with nested 'groups')."
            }
            Self::UnknownDirective => {
                "use 'editor'/'simulate' in 'general', 'path' in 'pods'/'recipes', or group names in 'groups'."
            }
            Self::UnknownGroup => {
                "define the group under 'recipes { groups { ... } }' in 'pods.conf'."
            }
        }
    }

    /// Returns the static high-level error summary for this configuration error kind.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::DuplicateBlock => "duplicate block in configuration",
            Self::DuplicateDirective => "duplicate directive in configuration",
            Self::EmptyGroupsBlock => "no recipe groups defined in 'groups' block",
            Self::InvalidArgumentCount => "invalid number of arguments for block or directive",
            Self::InvalidBoolean => "invalid boolean value (expected 'true' or 'false')",
            Self::InvalidGroupTarget => "invalid target in recipe group",
            Self::InvalidPath => "invalid path value in configuration",
            Self::MissingEditor => "no editor configured in configuration",
            Self::MissingGroupsBlock => "missing 'groups' block in 'recipes'",
            Self::MissingRecipesBlock => "missing 'recipes' block in configuration",
            Self::NotFound => "configuration file not found",
            Self::RecipeNotFound => "recipe not found in configuration",
            Self::UnexpectedBlock => "unexpected block in configuration",
            Self::UnknownDirective => "unknown directive in configuration",
            Self::UnknownGroup => "unknown recipe group in configuration",
        }
    }
}

impl Display for ConfigErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

/// Compiler-style diagnostic error representation for PSL, configuration, and recipe files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticError<'a> {
    /// Caret string pointing to the error location (for example, `^^^`).
    pub annotation_carets: Cow<'a, str>,
    /// Explanation accompanying the carets.
    pub annotation_text: Cow<'a, str>,
    /// Column number where the error occurred (1-based).
    pub col_number: usize,
    /// Path to the file where the error occurred.
    pub file_path: Cow<'a, str>,
    /// Actionable advice on how to resolve the error.
    pub help_text: Cow<'a, str>,
    /// Line number where the error occurred (1-based).
    pub line_number: usize,
    /// Raw line content snippet from the source file.
    pub line_snippet: Cow<'a, str>,
    /// High-level error summary.
    pub message: Cow<'a, str>,
}

impl DiagnosticError<'_> {
    /// Writes the location header (`--> file:line:col`), source gutter, carets, and `= help:` block.
    pub fn write_body(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let line_len = self.line_number.checked_ilog10().unwrap_or(0) as usize + 1;

        formatter.write_str(" --> ")?;
        write_sanitized_token(formatter, &self.file_path)?;
        writeln!(formatter, ":{}:{}", self.line_number, self.col_number)?;
        writeln!(formatter, "{:>width$} |", "", width = line_len)?;
        write!(formatter, "{:>width$} | ", self.line_number, width = line_len)?;
        write_sanitized_token(formatter, &self.line_snippet)?;
        formatter.write_char('\n')?;
        write!(formatter, "{:>width$} | ", "", width = line_len)?;
        write_sanitized_carets(formatter, &self.annotation_carets)?;
        formatter.write_char(' ')?;
        write_sanitized_token(formatter, &self.annotation_text)?;
        formatter.write_char('\n')?;
        writeln!(formatter, "{:>width$} |", "", width = line_len)?;
        formatter.write_str("= help: ")?;
        write_sanitized_token(formatter, &self.help_text)
    }
}

impl Display for DiagnosticError<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str("error: ")?;
        write_sanitized_token(formatter, &self.message)?;
        formatter.write_char('\n')?;
        self.write_body(formatter)
    }
}

impl Error for DiagnosticError<'_> {}

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

impl PodError {
    /// Returns the attached [`DiagnosticError`] when this error carries source-code diagnostics.
    #[must_use]
    pub fn diagnostic(&self) -> Option<&DiagnosticError<'static>> {
        match self {
            Self::Config(error) => error.diagnostic.as_deref(),
            Self::Psl(error) => error.diagnostic.as_deref(),
            Self::Recipe(error) => error.diagnostic.as_deref(),
            _ => None,
        }
    }
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PslError {
    pub diagnostic: Option<Box<DiagnosticError<'static>>>,
    pub kind: PslErrorKind,
    pub line: usize,
}

impl PslError {
    /// Creates a new PSL error with the given error kind and 1-based line number.
    #[must_use]
    pub const fn new(kind: PslErrorKind, line: usize) -> Self {
        Self {
            diagnostic: None,
            kind,
            line,
        }
    }

    /// Attaches a compiler-style [`DiagnosticError`] resolved from `source` and `file_path`.
    #[must_use]
    pub fn with_diagnostic(mut self, source: &str, file_path: &str) -> Self {
        let kind = self.kind;
        let diagnostic = build_diagnostic_from_line(
            source,
            file_path,
            self.line,
            kind.message(),
            kind.annotation_text(),
            kind.help_text(),
            |sanitized_line| locate_psl_error_span(sanitized_line, kind),
        );
        self.diagnostic = Some(Box::new(diagnostic));
        self
    }
}

impl Display for PslError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "line {}: {}", self.line, self.kind)?;
        if let Some(diagnostic) = &self.diagnostic {
            formatter.write_char('\n')?;
            diagnostic.write_body(formatter)?;
        }
        Ok(())
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

/// Recipe semantic validation or target container resolution error (`*.pod`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipeError {
    pub diagnostic: Option<Box<DiagnosticError<'static>>>,
    pub kind: RecipeErrorKind,
    pub line: Option<usize>,
    pub token: Option<String>,
}

impl RecipeError {
    /// Creates a new recipe error with an optional 1-based line number and no token.
    #[must_use]
    pub const fn new(kind: RecipeErrorKind, line: Option<usize>) -> Self {
        Self {
            diagnostic: None,
            kind,
            line,
            token: None,
        }
    }

    /// Attaches a compiler-style [`DiagnosticError`] resolved from `source` and `file_path`.
    #[must_use]
    pub fn with_diagnostic(mut self, source: &str, file_path: &str) -> Self {
        let line_number = self.line.unwrap_or(1).max(1);
        let diagnostic = build_line_diagnostic(
            source,
            file_path,
            line_number,
            self.token.as_deref(),
            self.kind.message(),
            self.kind.annotation_text(),
            self.kind.help_text(),
        );
        self.diagnostic = Some(Box::new(diagnostic));
        self
    }

    /// Creates a new recipe error associated with a 1-based line number and offending token.
    #[must_use]
    pub fn with_token(kind: RecipeErrorKind, line: usize, token: impl AsRef<str>) -> Self {
        Self {
            diagnostic: None,
            kind,
            line: Some(line),
            token: Some(sanitize_text(token.as_ref(), MAX_ERROR_TOKEN_BYTES)),
        }
    }

    /// Creates a new recipe error associated with an offending token without a line number.
    #[must_use]
    pub fn without_line(kind: RecipeErrorKind, token: impl AsRef<str>) -> Self {
        Self {
            diagnostic: None,
            kind,
            line: None,
            token: Some(sanitize_text(token.as_ref(), MAX_ERROR_TOKEN_BYTES)),
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
        if let Some(diagnostic) = &self.diagnostic {
            formatter.write_char('\n')?;
            diagnostic.write_body(formatter)?;
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

impl RecipeErrorKind {
    /// Returns the inline caret annotation message for this recipe error kind.
    #[must_use]
    pub const fn annotation_text(self) -> &'static str {
        match self {
            Self::ContainerNotFound => "target container does not match any container in this recipe",
            Self::DuplicateBlock => "block is already defined in this recipe",
            Self::DuplicateDirective => "directive may only appear once in this block",
            Self::EmptyContainerBlock => {
                "'container' block must contain at least an 'image' directive"
            }
            Self::EmptyPodBlock => "'pod' block cannot be empty",
            Self::InvalidArgumentCount => "invalid number of arguments for this block or directive",
            Self::InvalidCommand => "command definition or argument is empty or invalid",
            Self::InvalidContainerName => {
                "container name is invalid or collides with another container"
            }
            Self::InvalidDescription => {
                "description must be a non-empty text string without control characters"
            }
            Self::InvalidImage => "container image reference must be a valid single token",
            Self::InvalidOption => "option value must be non-empty text without control characters",
            Self::InvalidPath => "pod path must be a safe directory path without '..' or ':'",
            Self::InvalidPodId => "pod identifier must normalize to '[a-z0-9_][a-z0-9_.-]*'",
            Self::InvalidPublish => "port publish argument is malformed",
            Self::InvalidRegistry => {
                "registry must be a non-empty token without whitespace or leading '-'"
            }
            Self::InvalidRestartPolicy => {
                "restart policy must be a non-empty token without whitespace or leading '-'"
            }
            Self::InvalidVolume => {
                "volume mount path or option is invalid (paths cannot contain ':' or '..')"
            }
            Self::MissingContainerImage => {
                "container block is missing the required 'image' directive"
            }
            Self::MissingContainersBlock => "recipe must define at least one 'container' block",
            Self::MissingPodBlock => "recipe is missing the required 'pod <id> { ... }' block",
            Self::MissingPodName => "'pod' block is missing a non-empty 'name' directive",
            Self::MissingPodPath => {
                "no 'path' directive in 'pod' block and no default 'pods.path' configured"
            }
            Self::MissingPodRegistry => "'pod' block is missing the required 'registry' directive",
            Self::NotFound => "recipe file does not exist on disk",
            Self::UnexpectedBlock => "block is not permitted at this location in a '.pod' recipe",
            Self::UnknownDirective => "unrecognized directive in this recipe block",
        }
    }

    /// Returns actionable remediation advice for this recipe error kind.
    #[must_use]
    pub const fn help_text(self) -> &'static str {
        match self {
            Self::ContainerNotFound => {
                "specify a valid 1-based container index, '*suffix', or container name."
            }
            Self::DuplicateBlock => {
                "define only one 'pod' block and use unique names for 'command' blocks."
            }
            Self::DuplicateDirective => "remove the duplicate directive from this block.",
            Self::EmptyContainerBlock => "add 'image <name>:<tag>' inside the 'container' block.",
            Self::EmptyPodBlock => {
                "add required 'name' and 'registry' directives inside the 'pod' block."
            }
            Self::InvalidArgumentCount => {
                "check the expected argument count (for example: 'volume <host> <container> [<options>]' or 'publish <port> [<port>] [<proto>]')."
            }
            Self::InvalidCommand => {
                "provide a non-empty command string or valid 'command <name> { ... }' block."
            }
            Self::InvalidContainerName => {
                "use a unique name matching '[a-z0-9_][a-z0-9_.-]*' or relative '*suffix'."
            }
            Self::InvalidDescription => "provide a non-empty quoted string for 'description'.",
            Self::InvalidImage => {
                "specify a valid image reference such as 'alpine:latest' or 'example/app:1.0'."
            }
            Self::InvalidOption => {
                "provide valid Podman option arguments (for example: 'option --userns=keep-id')."
            }
            Self::InvalidPath => {
                "specify a valid directory path without parent traversal ('..') or colons (':')."
            }
            Self::InvalidPodId => {
                "start the pod identifier with a letter, digit, or underscore (for example: 'pod example-stack {')."
            }
            Self::InvalidPublish => {
                "use 'publish <host_port> <container_port> [TCP|UDP]' (for example: 'publish 8080 80 TCP')."
            }
            Self::InvalidRegistry => {
                "specify a valid container registry hostname (for example: 'registry docker.io')."
            }
            Self::InvalidRestartPolicy => {
                "use a valid Podman restart policy such as 'always', 'on-failure', 'unless-stopped', or 'never'."
            }
            Self::InvalidVolume => {
                "pass volume options as a separate third argument: 'volume <host_dir> <container_dir> [<options>]' (for example: 'volume data /var/lib/db/data Z')."
            }
            Self::MissingContainerImage => {
                "add 'image <repository>:<tag>' inside this 'container' block."
            }
            Self::MissingContainersBlock => {
                "add at least one 'container <name> { image ... }' block to the recipe."
            }
            Self::MissingPodBlock => {
                "add a 'pod <id> { name ...; registry ... }' block to the recipe."
            }
            Self::MissingPodName => "add 'name \"<Display Name>\"' inside the 'pod' block.",
            Self::MissingPodPath => {
                "set 'path /pods/<name>' in the recipe's 'pod' block or configure 'pods { path ... }' in 'pods.conf'."
            }
            Self::MissingPodRegistry => {
                "add 'registry <hostname>' (for example: 'registry docker.io') inside the 'pod' block."
            }
            Self::NotFound => {
                "verify that the '.pod' file exists in the configured 'recipes.path' directory."
            }
            Self::UnexpectedBlock => {
                "only 'pod' and 'container' blocks are allowed at root level, and only 'command' inside 'pod'."
            }
            Self::UnknownDirective => {
                "check the allowed directives for 'pod', 'command', and 'container' blocks in USAGE.md."
            }
        }
    }

    /// Returns the static high-level error summary for this recipe error kind.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::ContainerNotFound => "container not found in recipe",
            Self::DuplicateBlock => "duplicate block in recipe",
            Self::DuplicateDirective => "duplicate directive in recipe",
            Self::EmptyContainerBlock => "container block in recipe is empty",
            Self::EmptyPodBlock => "pod block in recipe is empty",
            Self::InvalidArgumentCount => "invalid number of arguments for block or directive",
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
        }
    }
}

impl Display for RecipeErrorKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
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

/// Constructs a diagnostic caret annotation string pointing to a 1-based source column and span.
#[must_use]
pub fn build_caret_annotation(
    col_number: usize,
    span_len: usize,
    max_len: usize,
) -> Cow<'static, str> {
    let clamped_max = max_len.clamp(1, MAX_ERROR_TOKEN_BYTES);
    let span = span_len.clamp(1, MAX_ERROR_TOKEN_BYTES);
    let padding_len = col_number.saturating_sub(1).min(clamped_max);

    if padding_len == 0 {
        let effective_span = span.min(clamped_max);
        if effective_span <= STATIC_CARETS.len() {
            Cow::Borrowed(&STATIC_CARETS[..effective_span])
        } else {
            Cow::Owned("^".repeat(effective_span))
        }
    } else {
        let effective_span = span.min(clamped_max.saturating_sub(padding_len).max(1));
        Cow::Owned(format!(
            "{}{}",
            " ".repeat(padding_len),
            "^".repeat(effective_span)
        ))
    }
}

/// Builds an owned [`DiagnosticError`] by extracting `line_number` from `source` and resolving its span.
#[must_use]
fn build_diagnostic_from_line<F>(
    source: &str,
    file_path: &str,
    line_number: usize,
    message: &'static str,
    annotation_text: &'static str,
    help_text: &'static str,
    resolve_span: F,
) -> DiagnosticError<'static>
where
    F: FnOnce(&str) -> (usize, usize),
{
    let effective_line = line_number.max(1);
    let raw_line = extract_source_line(source, effective_line);
    let sanitized_line = sanitize_text(raw_line, MAX_ERROR_TOKEN_BYTES);
    let (col_number, span_len) = resolve_span(&sanitized_line);
    let max_len = sanitized_line.chars().count().max(1);
    let carets = build_caret_annotation(col_number, span_len, max_len);

    DiagnosticError {
        annotation_carets: carets,
        annotation_text: Cow::Borrowed(annotation_text),
        col_number,
        file_path: Cow::Owned(sanitize_text(file_path, MAX_ERROR_TOKEN_BYTES)),
        help_text: Cow::Borrowed(help_text),
        line_number: effective_line,
        line_snippet: Cow::Owned(sanitized_line),
        message: Cow::Borrowed(message),
    }
}

/// Builds an owned [`DiagnosticError`] for a 1-based line and optional offending token.
#[must_use]
fn build_line_diagnostic(
    source: &str,
    file_path: &str,
    line_number: usize,
    token: Option<&str>,
    message: &'static str,
    annotation_text: &'static str,
    help_text: &'static str,
) -> DiagnosticError<'static> {
    build_diagnostic_from_line(
        source,
        file_path,
        line_number,
        message,
        annotation_text,
        help_text,
        |sanitized_line| locate_token_span(strip_inline_comment(sanitized_line), token),
    )
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

/// Extracts the 1-based `line_number` from `source`, or returns an empty slice when out of bounds.
#[must_use]
fn extract_source_line(source: &str, line_number: usize) -> &str {
    source
        .lines()
        .nth(line_number.saturating_sub(1))
        .unwrap_or("")
}

/// Finds the byte offset and length of an invalid escape sequence inside a double-quoted string on `code_slice`.
#[must_use]
fn find_invalid_escape_bytes(code_slice: &str) -> Option<(usize, usize)> {
    let bytes = code_slice.as_bytes();
    let mut in_double = false;
    let mut in_single = false;
    let mut index = 0;

    while let Some(&byte) = bytes.get(index) {
        if in_double
            && byte == b'\\'
            && let Some(next) = bytes.get(index + 1).copied()
        {
            if !matches!(next, b'"' | b'\\' | b'n' | b'r' | b't') {
                return Some((index, invalid_escape_byte_len(code_slice, index)));
            }
            index += 2;
            continue;
        }
        if !in_single && byte == b'"' {
            in_double = !in_double;
        } else if !in_double && byte == b'\'' {
            in_single = !in_single;
        }
        index += 1;
    }
    None
}

/// Searches `code_slice` for a standalone token matching `target` (preferring argument tokens over a leading keyword collision).
#[must_use]
fn find_matching_token_bytes(code_slice: &str, target: &str) -> Option<(usize, usize)> {
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
            if token_index > 0 || first_match.is_some() {
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
    let bytes = code_slice.as_bytes();
    let mut in_double = false;
    let mut in_single = false;
    let mut index = 0;
    let mut matched = None;

    while let Some(&byte) = bytes.get(index) {
        if in_double && byte == b'\\' {
            index += 2;
            continue;
        }
        if !in_single && byte == b'"' {
            in_double = !in_double;
        } else if !in_double && byte == b'\'' {
            in_single = !in_single;
        } else if !in_double && !in_single && byte == target_byte {
            if !from_end {
                return Some(index);
            }
            matched = Some(index);
        }
        index += 1;
    }
    matched
}

/// Finds the byte offset of an unclosed opening `"` or `'` quote on `line`.
#[must_use]
fn find_unterminated_quote_byte(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut in_double = false;
    let mut in_single = false;
    let mut index = 0;
    let mut open_quote_offset = None;

    while let Some(&byte) = bytes.get(index) {
        if in_double && byte == b'\\' {
            index += 2;
            continue;
        }
        if !in_single && byte == b'"' {
            in_double = !in_double;
            open_quote_offset = if in_double { Some(index) } else { None };
        } else if !in_double && byte == b'\'' {
            in_single = !in_single;
            open_quote_offset = if in_single { Some(index) } else { None };
        } else if !in_double && !in_single && byte == b'#' {
            break;
        }
        index += 1;
    }
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

/// Returns the byte length of the `\<char>` sequence starting at `backslash_index` in `code_slice`.
#[must_use]
fn invalid_escape_byte_len(code_slice: &str, backslash_index: usize) -> usize {
    let char_len = code_slice
        .get(backslash_index + 1..)
        .and_then(|rest| rest.chars().next())
        .map_or(1, char::len_utf8);
    1 + char_len
}

/// Computes the 1-based character column and span for a [`PslErrorKind`] on `sanitized_line`.
#[must_use]
fn locate_psl_error_span(sanitized_line: &str, kind: PslErrorKind) -> (usize, usize) {
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
fn locate_token_span(code_slice: &str, token: Option<&str>) -> (usize, usize) {
    if let Some(target) = token {
        if let Some((byte_start, byte_len)) = find_matching_token_bytes(code_slice, target) {
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

/// Advances `chars` by one PSL double-quoted character, resolving backslash escapes and sanitizing control characters.
#[must_use]
fn next_unescaped_char(chars: &mut Chars<'_>) -> Option<char> {
    let character = chars.next()?;
    if character != '\\' {
        return Some(sanitize_char(character));
    }
    let resolved = match chars.next() {
        Some('n' | 'r' | 't') => ' ',
        Some(escaped) => sanitize_char(escaped),
        None => '\\',
    };
    Some(resolved)
}

/// Scans a bare (unquoted) code token starting at `start` in `code_slice` and returns `(start, end)`.
#[must_use]
fn scan_bare_code_token(code_slice: &str, start: usize) -> (usize, usize) {
    let bytes = code_slice.as_bytes();
    let mut cursor = start;
    while let Some(&byte) = bytes.get(cursor) {
        if byte.is_ascii_whitespace() || byte == b'{' || byte == b'}' {
            break;
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
        if quote == b'"' && byte == b'\\' && cursor + 1 < bytes.len() {
            cursor += 2;
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

/// Strips any trailing `#` comment outside of single- or double-quoted strings.
#[must_use]
fn strip_inline_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_double = false;
    let mut in_single = false;
    let mut index = 0;

    while let Some(&byte) = bytes.get(index) {
        if in_double && byte == b'\\' {
            index += 2;
            continue;
        }
        if !in_single && byte == b'"' {
            in_double = !in_double;
        } else if !in_double && byte == b'\'' {
            in_single = !in_single;
        } else if !in_double && !in_single && byte == b'#' {
            return line.get(..index).unwrap_or(line);
        }
        index += 1;
    }
    line
}

/// Evaluates whether the token slice `code_slice[start..end]` matches `target` directly, unquoted, or after PSL double-quote unescaping.
#[must_use]
fn token_matches_target(code_slice: &str, start: usize, end: usize, target: &str) -> bool {
    let Some(raw_token) = code_slice.get(start..end) else {
        return false;
    };
    if !target.is_empty() && raw_token == target {
        return true;
    }
    if let Some(inner) = raw_token
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
    {
        return inner == target;
    }
    if let Some(inner) = raw_token
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    {
        return inner == target || unescaped_double_quote_matches(inner, target);
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

/// Writes `carets` to `formatter`, clamping to [`MAX_ERROR_TOKEN_BYTES`] and allowing only `' '` and `'^'`.
fn write_sanitized_carets(formatter: &mut Formatter<'_>, carets: &str) -> fmt::Result {
    let clamped = clamp_utf8_prefix(carets, MAX_ERROR_TOKEN_BYTES);
    for character in clamped.chars() {
        let safe = if character == '^' { '^' } else { ' ' };
        formatter.write_char(safe)?;
    }
    Ok(())
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
    use std::borrow::Cow;
    use std::error::Error;
    use std::io;

    use super::{
        CliError, CliErrorKind, ConfigError, ConfigErrorKind, DiagnosticError,
        MAX_ERROR_TOKEN_BYTES, PodError, PslError, PslErrorKind, RecipeError, RecipeErrorKind,
        SystemError, build_caret_annotation, unescaped_double_quote_matches,
    };

    #[test]
    fn caret_annotation_zero_allocation_and_extreme_bounds() {
        let carets = build_caret_annotation(1, 3, 10);
        assert_eq!(carets, "^^^");
        assert!(matches!(carets, Cow::Borrowed(_)));

        let padded = build_caret_annotation(4, 3, 10);
        assert_eq!(padded, "   ^^^");

        let carets_0 = build_caret_annotation(0, 5, 20);
        assert_eq!(carets_0, "^^^^^");
        assert!(matches!(carets_0, Cow::Borrowed(_)));

        let carets_clamped = build_caret_annotation(0, 50, 10);
        assert_eq!(carets_clamped, "^^^^^^^^^^");

        let carets_zero_span = build_caret_annotation(0, 0, 0);
        assert_eq!(carets_zero_span, "^");

        let max_len = 80;
        let extreme_col = build_caret_annotation(usize::MAX, 5, max_len);
        assert_eq!(extreme_col.len(), max_len + 1);
        assert_eq!(&extreme_col[..max_len], " ".repeat(max_len));
        assert_eq!(&extreme_col[max_len..], "^");

        let long_len = 120;
        let long_carets = build_caret_annotation(1, long_len, 200);
        assert_eq!(long_carets.len(), long_len);
        assert!(long_carets.chars().all(|character| character == '^'));
        assert!(matches!(long_carets, Cow::Owned(_)));
    }

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
    fn config_error_kind_display_and_diagnostics() {
        let variants = [
            (ConfigErrorKind::DuplicateBlock, "duplicate block"),
            (ConfigErrorKind::DuplicateDirective, "duplicate directive"),
            (ConfigErrorKind::EmptyGroupsBlock, "no recipe groups"),
            (ConfigErrorKind::InvalidArgumentCount, "invalid number of arguments"),
            (ConfigErrorKind::InvalidBoolean, "invalid boolean"),
            (ConfigErrorKind::InvalidGroupTarget, "invalid target"),
            (ConfigErrorKind::InvalidPath, "invalid path"),
            (ConfigErrorKind::MissingEditor, "no editor configured"),
            (ConfigErrorKind::MissingGroupsBlock, "missing 'groups'"),
            (ConfigErrorKind::MissingRecipesBlock, "missing 'recipes'"),
            (ConfigErrorKind::NotFound, "configuration file not found"),
            (ConfigErrorKind::RecipeNotFound, "recipe not found"),
            (ConfigErrorKind::UnexpectedBlock, "unexpected block"),
            (ConfigErrorKind::UnknownDirective, "unknown directive"),
            (ConfigErrorKind::UnknownGroup, "unknown recipe group"),
        ];

        for (index, (kind, expected_substring)) in variants.iter().enumerate() {
            let plain = ConfigError::new(*kind, None);
            assert!(plain.to_string().contains(expected_substring));
            assert!(!kind.annotation_text().is_empty());
            assert!(!kind.help_text().is_empty());

            let line_only = ConfigError::new(*kind, Some(index + 1));
            assert!(line_only.to_string().contains(expected_substring));
            assert!(line_only.to_string().contains("line "));

            let with_token = ConfigError::with_token(*kind, 1, "bad_token")
                .with_diagnostic("    key bad_token # comment\n", "./pods.conf");
            let diagnostic = with_token.diagnostic.as_ref().unwrap();
            assert!(matches!(diagnostic.message, Cow::Borrowed(_)));
            assert_eq!(diagnostic.message, kind.message());
            let rendered = with_token.to_string();
            assert!(rendered.contains(expected_substring));
            assert!(rendered.contains("bad_token"));
            assert!(rendered.contains("--> ./pods.conf:1:9"));
            assert!(rendered.contains("^^^^^^^^^"));
            assert!(rendered.contains("= help: "));

            let token_no_line = ConfigError::without_line(*kind, "bad_token");
            assert!(token_no_line.to_string().contains(expected_substring));
            assert!(token_no_line.to_string().contains("bad_token"));
        }
    }

    #[test]
    fn diagnostic_error_render_and_gutter_alignment() {
        let diagnostic = DiagnosticError {
            annotation_carets: build_caret_annotation(19, 18, 74),
            annotation_text: Cow::Borrowed(
                "volume mount path or option is invalid (paths cannot contain ':' or '..')",
            ),
            col_number: 19,
            file_path: Cow::Borrowed("./recipe.pod"),
            help_text: Cow::Borrowed(
                "pass volume options as a separate third argument: 'volume <host_dir> <container_dir> [<options>]'.",
            ),
            line_number: 28,
            line_snippet: Cow::Borrowed(
                "    volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'",
            ),
            message: Cow::Borrowed("invalid volume mount in recipe"),
        };

        let rendered = diagnostic.to_string();
        assert_eq!(format!("{diagnostic}"), rendered);
        assert!(rendered.contains("error: invalid volume mount in recipe"));
        assert!(rendered.contains(" --> ./recipe.pod:28:19"));
        assert!(rendered.contains(
            "   |\n28 |     volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'\n   |                   ^^^^^^^^^^^^^^^^^^ volume mount path or option is invalid"
        ));
        assert!(rendered.contains("= help: pass volume options"));

        let standard_error: &dyn Error = &diagnostic;
        assert!(standard_error.source().is_none());

        // Empty quoted token ("") and single-quoted empty token ('') span resolution
        let empty_double = ConfigError::with_token(ConfigErrorKind::InvalidPath, 1, "")
            .with_diagnostic("    path \"\" # empty\n", "./pods.conf");
        let empty_double_diagnostic = empty_double.diagnostic.as_ref().unwrap();
        assert_eq!(empty_double_diagnostic.col_number, 10);
        assert_eq!(empty_double_diagnostic.annotation_carets, "         ^^");

        let empty_single = ConfigError::with_token(ConfigErrorKind::InvalidPath, 1, "")
            .with_diagnostic("    path '' # empty\n", "./pods.conf");
        let empty_single_diagnostic = empty_single.diagnostic.as_ref().unwrap();
        assert_eq!(empty_single_diagnostic.col_number, 10);
        assert_eq!(empty_single_diagnostic.annotation_carets, "         ^^");

        // Escaped double-quoted token matching ("bad\ndesc" -> sanitized "bad desc")
        let escaped_token = RecipeError::with_token(
            RecipeErrorKind::InvalidDescription,
            1,
            "bad\ndesc",
        )
        .with_diagnostic("    description \"bad\\ndesc\" # comment\n", "./recipe.pod");
        let escaped_diagnostic = escaped_token.diagnostic.as_ref().unwrap();
        assert_eq!(escaped_diagnostic.col_number, 17);
        assert_eq!(
            escaped_diagnostic.annotation_carets,
            "                ^^^^^^^^^^^"
        );

        // Keyword vs argument collision ("command command {") highlights argument token
        let collision = RecipeError::with_token(RecipeErrorKind::DuplicateBlock, 1, "command")
            .with_diagnostic("    command command {\n", "./recipe.pod");
        let collision_diagnostic = collision.diagnostic.as_ref().unwrap();
        assert_eq!(collision_diagnostic.col_number, 13);
        assert_eq!(collision_diagnostic.annotation_carets, "            ^^^^^^^");

        // Direct unescaped double-quote comparison edge cases
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

    #[test]
    fn pod_error_cli_display_and_source() {
        let cli_error = CliError::with_argument(CliErrorKind::UnknownFlag, "--unknown");
        let pod_error = PodError::from(cli_error.clone());

        assert!(pod_error.to_string().contains("CLI error"));
        assert!(pod_error.to_string().contains("--unknown"));
        assert!(pod_error.diagnostic().is_none());
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
        assert!(pod_error.diagnostic().is_none());
        assert!(pod_error.source().is_some());
    }

    #[test]
    fn pod_error_psl_display_and_source() {
        let psl_error = PslError::new(PslErrorKind::UnexpectedRootDirective, 4);
        let pod_error = PodError::from(psl_error.clone());

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
            assert!(pod_error.diagnostic().is_none());
            assert!(pod_error.source().is_some());
        }
    }

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
            (
                PslErrorKind::UnclosedBlock,
                "unclosed block",
                "general {\n",
            ),
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
            let formatted = error.to_string();
            assert!(formatted.contains(expected_substring));

            let with_diagnostic =
                PslError::new(*kind, 1).with_diagnostic(sample_line, "./test.pod");
            let diagnostic = with_diagnostic.diagnostic.as_ref().unwrap();
            assert!(matches!(diagnostic.message, Cow::Borrowed(_)));
            assert_eq!(diagnostic.message, kind.message());
            let diagnostic_string = with_diagnostic.to_string();
            assert!(diagnostic_string.contains(expected_substring));
            assert!(diagnostic_string.contains("--> ./test.pod:1:"));
            assert!(diagnostic_string.contains("= help: "));
            assert!(PodError::from(with_diagnostic).diagnostic().is_some());
        }

        // Single-quoted literal backslash preceding invalid double-quoted escape
        let single_then_bad_escape = PslError::new(PslErrorKind::InvalidEscapeSequence, 1)
            .with_diagnostic("    command 'C:\\path\\ok' \"bad\\xescape\"\n", "./test.pod");
        let escape_diagnostic = single_then_bad_escape.diagnostic.as_ref().unwrap();
        assert_eq!(escape_diagnostic.col_number, 30);
        assert_eq!(
            escape_diagnostic.annotation_carets,
            "                             ^^"
        );

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
    }

    #[test]
    fn recipe_error_kind_display_and_diagnostics() {
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
            let plain = RecipeError::new(*kind, None);
            assert!(plain.to_string().contains(expected_substring));
            assert!(!kind.annotation_text().is_empty());
            assert!(!kind.help_text().is_empty());

            let line_only = RecipeError::new(*kind, Some(index + 1));
            assert!(line_only.to_string().contains(expected_substring));
            assert!(line_only.to_string().contains("line "));

            let with_token = RecipeError::with_token(*kind, 1, "/var/lib/db/data:Z")
                .with_diagnostic(
                    "    volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'\n",
                    "./recipe.pod",
                );
            let diagnostic = with_token.diagnostic.as_ref().unwrap();
            assert!(matches!(diagnostic.message, Cow::Borrowed(_)));
            assert_eq!(diagnostic.message, kind.message());
            let rendered = with_token.to_string();
            assert!(rendered.contains(expected_substring));
            assert!(rendered.contains("/var/lib/db/data:Z"));
            assert!(rendered.contains("--> ./recipe.pod:1:19"));
            assert!(rendered.contains("                  ^^^^^^^^^^^^^^^^^^"));
            assert!(rendered.contains("= help: "));

            let token_no_line = RecipeError::without_line(*kind, "bad_token");
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
            diagnostic: None,
            kind: ConfigErrorKind::InvalidGroupTarget,
            line: Some(5),
            token: Some(String::from(injected)),
        };
        let direct_config_display = direct_config.to_string();
        assert_eq!(direct_config_display.lines().count(), 1);
        assert!(!direct_config_display.contains('\x1b'));
        assert!(!direct_config_display.contains('\0'));
        assert!(!direct_config_display.contains('\u{202e}'));

        let direct_diagnostic = DiagnosticError {
            annotation_carets: Cow::Borrowed("^^^\nINFO: Forged"),
            annotation_text: Cow::Borrowed(injected),
            col_number: 1,
            file_path: Cow::Borrowed(injected),
            help_text: Cow::Borrowed(injected),
            line_number: 1,
            line_snippet: Cow::Borrowed(injected),
            message: Cow::Borrowed(injected),
        };
        let diagnostic_display = direct_diagnostic.to_string();
        assert_eq!(diagnostic_display.lines().count(), 7);
        assert!(!diagnostic_display.contains('\x1b'));
        assert!(!diagnostic_display.contains('\0'));
        assert!(!diagnostic_display.contains('\u{009b}'));
        assert!(!diagnostic_display.contains('\u{202e}'));

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
            diagnostic: None,
            kind: ConfigErrorKind::UnknownDirective,
            line: None,
            token: Some(multi_byte),
        };
        let formatted_oversized = direct_oversized.to_string();
        assert!(formatted_oversized.len() <= MAX_ERROR_TOKEN_BYTES + 64);
        assert!(formatted_oversized.ends_with("ä'"));
    }
}
