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

//! Configuration semantic validation error types (`pods.conf`).

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::error::MAX_ERROR_TOKEN_BYTES;
use crate::error::diagnostic::{self, DiagnosticError};
use crate::error::span;
use crate::utilities::string;

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
        let prefer_argument = matches!(
            self.kind,
            ConfigErrorKind::InvalidBoolean
                | ConfigErrorKind::InvalidGroupTarget
                | ConfigErrorKind::InvalidPath
        );
        let token = self.token.as_deref();
        let (annotation_text, help_text) = diagnostic_messages(self.kind, token);
        let built = diagnostic::build_diagnostic_from_line(
            source,
            file_path,
            line_number,
            annotation_text,
            help_text,
            |sanitized_line| {
                span::locate_token_span(
                    span::strip_inline_comment(sanitized_line),
                    token,
                    prefer_argument,
                )
            },
        );
        self.diagnostic = Some(Box::new(built));
        self
    }

    /// Creates a new configuration error associated with a 1-based line number and offending token.
    #[must_use]
    pub fn with_token(kind: ConfigErrorKind, line: usize, token: impl AsRef<str>) -> Self {
        Self {
            diagnostic: None,
            kind,
            line: Some(line),
            token: Some(string::sanitize_text(token.as_ref(), MAX_ERROR_TOKEN_BYTES)),
        }
    }

    /// Creates a new configuration error associated with an offending token without a line number.
    #[must_use]
    pub fn without_line(kind: ConfigErrorKind, token: impl AsRef<str>) -> Self {
        Self {
            diagnostic: None,
            kind,
            line: None,
            token: Some(string::sanitize_text(token.as_ref(), MAX_ERROR_TOKEN_BYTES)),
        }
    }
}

impl Display for ConfigError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        diagnostic::write_structured_error(
            formatter,
            self.line,
            &self.kind,
            self.token.as_deref(),
            self.diagnostic.as_deref(),
        )
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

/// Resolves tailored diagnostic annotation and help messages for `kind` and `token`.
#[must_use]
fn diagnostic_messages(kind: ConfigErrorKind, token: Option<&str>) -> (&'static str, &'static str) {
    if kind == ConfigErrorKind::InvalidPath && token.is_some_and(is_disallowed_podman_editor_token)
    {
        return (
            "'podman' is not allowed as an external editor",
            "configure a text editor such as 'vim', 'nvim', or 'nano' instead of 'podman'.",
        );
    }
    (kind.annotation_text(), kind.help_text())
}

/// Evaluates whether `token` is a bare or absolute `podman` executable name disallowed as an editor.
#[must_use]
fn is_disallowed_podman_editor_token(token: &str) -> bool {
    if token.is_empty()
        || token.starts_with('-')
        || (token.contains('/') && !token.starts_with('/'))
        || token.contains("..")
        || string::contains_control_or_whitespace(token)
    {
        return false;
    }
    token
        .rsplit('/')
        .next()
        .is_some_and(|name| name.eq_ignore_ascii_case("podman"))
}

#[cfg(test)]
mod tests {
    use crate::error::MAX_ERROR_TOKEN_BYTES;
    use super::{ConfigError, ConfigErrorKind};

    #[test]
    fn config_error_kind_display_and_diagnostics() {
        let variants = [
            (ConfigErrorKind::DuplicateBlock, "duplicate block"),
            (ConfigErrorKind::DuplicateDirective, "duplicate directive"),
            (ConfigErrorKind::EmptyGroupsBlock, "no recipe groups"),
            (
                ConfigErrorKind::InvalidArgumentCount,
                "invalid number of arguments",
            ),
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
            assert_eq!(diagnostic.annotation_text, kind.annotation_text());
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
    fn red_team_config_error_sanitization_and_utf8_bounds() {
        let injected =
            "bad\nINFO: Forged\rWARNING: Overwrite\x1b[31mANSI\0null\u{009b}CSI\u{202e}RLO";
        let config_error = ConfigError::with_token(ConfigErrorKind::InvalidPath, 3, injected);
        let config_token = config_error.token.as_deref().unwrap();
        assert!(!config_token.contains('\n'));
        assert!(!config_token.contains('\r'));
        assert!(!config_token.contains('\x1b'));
        assert!(!config_token.contains('\0'));
        assert!(!config_token.contains('\u{009b}'));
        assert!(!config_token.contains('\u{202e}'));

        let direct_config = ConfigError {
            diagnostic: None,
            kind: ConfigErrorKind::InvalidGroupTarget,
            line: Some(5),
            token: Some(String::from(injected)),
        };
        let direct_display = direct_config.to_string();
        assert_eq!(direct_display.lines().count(), 1);
        assert!(!direct_display.contains('\x1b'));
        assert!(!direct_display.contains('\0'));
        assert!(!direct_display.contains('\u{202e}'));

        let multi_byte = format!("x{}", "ä".repeat((MAX_ERROR_TOKEN_BYTES / 2) + 20));
        let bounded_config =
            ConfigError::with_token(ConfigErrorKind::UnknownDirective, 1, &multi_byte);
        let bounded_token = bounded_config.token.as_deref().unwrap();
        assert!(bounded_token.len() <= MAX_ERROR_TOKEN_BYTES);
        assert!(bounded_token.ends_with('ä'));

        let direct_oversized = ConfigError {
            diagnostic: None,
            kind: ConfigErrorKind::UnknownDirective,
            line: None,
            token: Some(multi_byte),
        };
        let formatted_oversized = direct_oversized.to_string();
        assert!(formatted_oversized.len() <= MAX_ERROR_TOKEN_BYTES + 64);
        assert!(formatted_oversized.ends_with("ä'"));

        let podman_editor = ConfigError::with_token(ConfigErrorKind::InvalidPath, 2, "podman")
            .with_diagnostic("general {\n    editor podman\n}\n", "./pods.conf");
        let podman_diag = podman_editor.diagnostic.as_ref().unwrap();
        assert!(podman_diag.annotation_text.contains("podman"));
        assert!(podman_diag.help_text.contains("podman"));

        let abs_podman =
            ConfigError::with_token(ConfigErrorKind::InvalidPath, 2, "/usr/bin/podman")
                .with_diagnostic("general {\n    editor /usr/bin/podman\n}\n", "./pods.conf");
        assert!(
            abs_podman
                .diagnostic
                .as_ref()
                .unwrap()
                .annotation_text
                .contains("podman")
        );

        let traversal_podman =
            ConfigError::with_token(ConfigErrorKind::InvalidPath, 2, "../podman")
                .with_diagnostic("pods {\n    path ../podman\n}\n", "./pods.conf");
        assert_eq!(
            traversal_podman
                .diagnostic
                .as_ref()
                .unwrap()
                .annotation_text,
            ConfigErrorKind::InvalidPath.annotation_text()
        );
    }
}
