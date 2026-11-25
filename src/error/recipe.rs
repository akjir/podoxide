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

//! Recipe semantic validation and container target resolution error types (`*.pod`).

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::error::MAX_ERROR_TOKEN_BYTES;
use crate::error::diagnostic::{self, DiagnosticError};
use crate::error::span;
use crate::utilities::string;

/// Recipe semantic validation or target container resolution error (`*.pod`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipeError {
    pub diagnostic: Option<Box<DiagnosticError<'static>>>,
    pub kind: RecipeErrorKind,
    pub line: Option<usize>,
    pub token: Option<String>,
}

impl RecipeError {
    /// Attaches a compiler-style [`DiagnosticError`] resolved from `source` and `file_path`.
    #[must_use]
    pub fn with_diagnostic(mut self, source: &str, file_path: &str) -> Self {
        let line_number = self.line.unwrap_or(1).max(1);
        let prefer_argument = !matches!(
            self.kind,
            RecipeErrorKind::DuplicateDirective
                | RecipeErrorKind::InvalidArgumentCount
                | RecipeErrorKind::MissingPodName
                | RecipeErrorKind::UnexpectedBlock
                | RecipeErrorKind::UnknownDirective
        );
        let token = self.token.as_deref();
        let built = diagnostic::build_diagnostic_from_line(
            source,
            file_path,
            line_number,
            self.kind.annotation_text(),
            self.kind.help_text(),
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

    /// Creates a new recipe error associated with a 1-based line number and offending token.
    #[must_use]
    pub fn with_token(kind: RecipeErrorKind, line: usize, token: impl AsRef<str>) -> Self {
        Self {
            diagnostic: None,
            kind,
            line: Some(line),
            token: Some(string::sanitize_text(token.as_ref(), MAX_ERROR_TOKEN_BYTES)),
        }
    }

    /// Creates a new recipe error associated with an offending token without a line number.
    #[must_use]
    pub fn without_line(kind: RecipeErrorKind, token: impl AsRef<str>) -> Self {
        Self {
            diagnostic: None,
            kind,
            line: None,
            token: Some(string::sanitize_text(token.as_ref(), MAX_ERROR_TOKEN_BYTES)),
        }
    }
}

impl Display for RecipeError {
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

impl Error for RecipeError {}

/// Specific failure category encountered while loading, validating, or resolving a `.pod` recipe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecipeErrorKind {
    ContainerNotFound,
    ContainerNotRunning,
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
    MultipleContainers,
    NotFound,
    UnexpectedBlock,
    UnknownDirective,
}

impl RecipeErrorKind {
    /// Returns the inline caret annotation message for this recipe error kind.
    #[must_use]
    pub const fn annotation_text(self) -> &'static str {
        match self {
            Self::ContainerNotFound => {
                "target container does not match any container in this recipe"
            }
            Self::ContainerNotRunning => "resolved container is not currently running in Podman",
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
            Self::InvalidPodId => "pod identifier must normalize to '[a-z0-9][a-z0-9_.-]*'",
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
            Self::MultipleContainers => {
                "recipe defines multiple containers; a specific container target is required"
            }
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
            Self::ContainerNotRunning => {
                "start the pod first with 'pods up <recipe>' or 'pods start <recipe>'."
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
                "use a unique name matching '[a-z0-9][a-z0-9_.-]*' or relative '*suffix'."
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
                "start the pod identifier with a letter or digit (for example: 'pod example-stack {')."
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
            Self::MultipleContainers => {
                "specify a container explicitly using '<recipe>/<container>' (by index, '*suffix', or name)."
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
            Self::ContainerNotRunning => "container is not running or does not exist",
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
            Self::MultipleContainers => "recipe has multiple containers; specify one explicitly",
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

#[cfg(test)]
mod tests {
    use crate::error::MAX_ERROR_TOKEN_BYTES;
    use super::{RecipeError, RecipeErrorKind};

    #[test]
    fn recipe_error_assigned_quote_token_highlights_only_assigned_argument() {
        let double_quoted =
            RecipeError::with_token(RecipeErrorKind::InvalidOption, 1, "--flag=bad val")
                .with_diagnostic("    option --flag=\"bad val\" # comment\n", "./recipe.pod");
        let double_diag = double_quoted.diagnostic.as_ref().unwrap();
        assert_eq!(double_diag.col_number, 12);
        assert_eq!(double_diag.annotation_carets, "           ^^^^^^^^^^^^^^^^");

        let single_quoted =
            RecipeError::with_token(RecipeErrorKind::InvalidOption, 1, "--flag=bad val")
                .with_diagnostic("    option --flag='bad val' # comment\n", "./recipe.pod");
        let single_diag = single_quoted.diagnostic.as_ref().unwrap();
        assert_eq!(single_diag.col_number, 12);
        assert_eq!(single_diag.annotation_carets, "           ^^^^^^^^^^^^^^^^");

        let escaped = RecipeError::with_token(RecipeErrorKind::InvalidOption, 1, "--flag=bad\nval")
            .with_diagnostic(
                "    option --flag=\"bad\\nval\" # comment\n",
                "./recipe.pod",
            );
        let escaped_diag = escaped.diagnostic.as_ref().unwrap();
        assert_eq!(escaped_diag.col_number, 12);
        assert_eq!(
            escaped_diag.annotation_carets,
            "           ^^^^^^^^^^^^^^^^^"
        );
    }

    #[test]
    fn recipe_error_kind_display_and_diagnostics() {
        let variants = [
            (RecipeErrorKind::ContainerNotFound, "container not found"),
            (
                RecipeErrorKind::ContainerNotRunning,
                "container is not running",
            ),
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
            (RecipeErrorKind::MissingPodName, "missing 'name' directive"),
            (RecipeErrorKind::MissingPodPath, "missing 'path' directive"),
            (
                RecipeErrorKind::MissingPodRegistry,
                "missing 'registry' directive",
            ),
            (
                RecipeErrorKind::MultipleContainers,
                "recipe has multiple containers",
            ),
            (RecipeErrorKind::NotFound, "recipe file not found"),
            (RecipeErrorKind::UnexpectedBlock, "unexpected block"),
            (RecipeErrorKind::UnknownDirective, "unknown directive"),
        ];

        for (index, (kind, expected_substring)) in variants.iter().enumerate() {
            let without_line = RecipeError::without_line(*kind, "bad_token");
            assert!(without_line.to_string().contains(expected_substring));
            assert!(without_line.to_string().contains("bad_token"));
            assert!(!kind.annotation_text().is_empty());
            assert!(!kind.help_text().is_empty());

            let with_line = RecipeError::with_token(*kind, index + 1, "bad_token");
            assert!(with_line.to_string().contains(expected_substring));
            assert!(with_line.to_string().contains("line "));

            let with_token = RecipeError::with_token(*kind, 1, "/var/lib/db/data:Z")
                .with_diagnostic(
                    "    volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'\n",
                    "./recipe.pod",
                );
            let diagnostic = with_token.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.annotation_text, kind.annotation_text());
            let rendered = with_token.to_string();
            assert!(rendered.contains(expected_substring));
            assert!(rendered.contains("/var/lib/db/data:Z"));
            assert!(rendered.contains("--> ./recipe.pod:1:19"));
            assert!(rendered.contains("                  ^^^^^^^^^^^^^^^^^^"));
            assert!(rendered.contains("= help: "));
        }

        let tokenless = RecipeError {
            diagnostic: None,
            kind: RecipeErrorKind::NotFound,
            line: None,
            token: None,
        };
        assert_eq!(tokenless.to_string(), "recipe file not found");
    }

    #[test]
    fn red_team_recipe_error_sanitization_and_utf8_bounds() {
        let injected =
            "bad\nINFO: Forged\rWARNING: Overwrite\x1b[31mANSI\0null\u{009b}CSI\u{202e}RLO";
        let recipe_error = RecipeError::with_token(RecipeErrorKind::InvalidPath, 4, injected);
        let recipe_token = recipe_error.token.as_deref().unwrap();
        assert!(!recipe_token.contains('\n'));
        assert!(!recipe_token.contains('\r'));
        assert!(!recipe_token.contains('\x1b'));
        assert!(!recipe_token.contains('\0'));
        assert!(!recipe_token.contains('\u{009b}'));
        assert!(!recipe_token.contains('\u{202e}'));

        let multi_byte = format!("x{}", "ä".repeat((MAX_ERROR_TOKEN_BYTES / 2) + 20));
        let bounded_recipe =
            RecipeError::with_token(RecipeErrorKind::UnknownDirective, 1, &multi_byte);
        let bounded_token = bounded_recipe.token.as_deref().unwrap();
        assert!(bounded_token.len() <= MAX_ERROR_TOKEN_BYTES);
        assert!(bounded_token.ends_with('ä'));
    }
}
