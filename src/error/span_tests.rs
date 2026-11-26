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

//! Unit tests for quote-aware source line scanning and token span resolution in [`super`].

use crate::error::{ConfigError, ConfigErrorKind, RecipeError, RecipeErrorKind};
use super::unescaped_double_quote_matches;

#[test]
fn token_span_resolution_handles_empty_escaped_and_colliding_tokens() {
    let empty_double = ConfigError::with_token(ConfigErrorKind::InvalidPath, 1, "")
        .with_diagnostic("    path \"\" # empty\n", "./pods.conf");
    assert_eq!(empty_double.diagnostic.as_ref().unwrap().col_number, 10);
    assert_eq!(
        empty_double.diagnostic.as_ref().unwrap().annotation_carets,
        "         ^^"
    );

    let empty_single = ConfigError::with_token(ConfigErrorKind::InvalidPath, 1, "")
        .with_diagnostic("    path '' # empty\n", "./pods.conf");
    assert_eq!(empty_single.diagnostic.as_ref().unwrap().col_number, 10);
    assert_eq!(
        empty_single.diagnostic.as_ref().unwrap().annotation_carets,
        "         ^^"
    );

    let escaped_token =
        RecipeError::with_token(RecipeErrorKind::InvalidDescription, 1, "bad\ndesc")
            .with_diagnostic("    description \"bad\\ndesc\" # comment\n", "./recipe.pod");
    let escaped_diag = escaped_token.diagnostic.as_ref().unwrap();
    assert_eq!(escaped_diag.col_number, 17);
    assert_eq!(
        escaped_diag.annotation_carets,
        "                ^^^^^^^^^^^"
    );

    let collision = RecipeError::with_token(RecipeErrorKind::DuplicateBlock, 1, "command")
        .with_diagnostic("    command command {\n", "./recipe.pod");
    assert_eq!(collision.diagnostic.as_ref().unwrap().col_number, 13);

    let duplicate_group = ConfigError::with_token(ConfigErrorKind::DuplicateDirective, 1, "alpha")
        .with_diagnostic("        alpha beta alpha\n", "./pods.conf");
    assert_eq!(duplicate_group.diagnostic.as_ref().unwrap().col_number, 9);

    let invalid_boolean = ConfigError::with_token(ConfigErrorKind::InvalidBoolean, 1, "simulate")
        .with_diagnostic("    simulate simulate\n", "./pods.conf");
    assert_eq!(invalid_boolean.diagnostic.as_ref().unwrap().col_number, 14);

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
