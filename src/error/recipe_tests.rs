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

//! Unit tests for recipe error formatting, diagnostics, and sanitization in [`super`].

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
        (RecipeErrorKind::InvalidPodName, "invalid pod name"),
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

        let with_token = RecipeError::with_token(*kind, 1, "/var/lib/db/data:Z").with_diagnostic(
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
        arg_index: None,
        diagnostic: None,
        kind: RecipeErrorKind::NotFound,
        line: None,
        token: None,
    };
    assert_eq!(tokenless.to_string(), "recipe file not found");

    let zero_line = RecipeError::at_line_or_none(RecipeErrorKind::InvalidPodId, 0, "-bad");
    assert_eq!(zero_line.line, None);
    assert!(!zero_line.to_string().contains("line 0:"));
    assert!(!zero_line.to_string().contains("line 1:"));

    let nonzero_line = RecipeError::at_line_or_none(RecipeErrorKind::InvalidPodId, 4, "-bad");
    assert_eq!(nonzero_line.line, Some(4));
    assert!(nonzero_line.to_string().contains("line 4:"));

    let arg_pinned = RecipeError::with_token(RecipeErrorKind::InvalidVolume, 1, "data")
        .with_arg_index(2)
        .with_diagnostic("    volume data data\n", "./recipe.pod");
    let pinned_diag = arg_pinned.diagnostic.as_ref().unwrap();
    assert_eq!(pinned_diag.col_number, 17);
    assert_eq!(pinned_diag.annotation_carets, "                ^^^^");
}

#[test]
fn red_team_recipe_error_sanitization_and_utf8_bounds() {
    let injected = "bad\nINFO: Forged\rWARNING: Overwrite\x1b[31mANSI\0null\u{009b}CSI\u{202e}RLO";
    let recipe_error = RecipeError::with_token(RecipeErrorKind::InvalidPath, 4, injected);
    let recipe_token = recipe_error.token.as_deref().unwrap();
    assert!(!recipe_token.contains('\n'));
    assert!(!recipe_token.contains('\r'));
    assert!(!recipe_token.contains('\x1b'));
    assert!(!recipe_token.contains('\0'));
    assert!(!recipe_token.contains('\u{009b}'));
    assert!(!recipe_token.contains('\u{202e}'));

    let multi_byte = format!("x{}", "ä".repeat((MAX_ERROR_TOKEN_BYTES / 2) + 20));
    let bounded_recipe = RecipeError::with_token(RecipeErrorKind::UnknownDirective, 1, &multi_byte);
    let bounded_token = bounded_recipe.token.as_deref().unwrap();
    assert!(bounded_token.len() <= MAX_ERROR_TOKEN_BYTES);
    assert!(bounded_token.ends_with('ä'));
}
