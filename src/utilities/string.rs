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

//! Zero-copy UTF-8 string clamping, character sanitization, identifier validation, and ASCII parsing helpers.

/// Truncates `input` at the largest valid UTF-8 character boundary not exceeding `max_bytes`.
#[must_use]
pub fn clamp_utf8_prefix(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }
    let mut boundary = max_bytes;
    while boundary > 0 && !input.is_char_boundary(boundary) {
        boundary -= 1;
    }
    &input[..boundary]
}

/// Evaluates whether `value` contains any whitespace, control, or bidirectional formatting character.
#[must_use]
pub fn contains_control_or_whitespace(value: &str) -> bool {
    value.chars().any(|character| {
        character.is_whitespace() || character.is_control() || is_bidi_control(character)
    })
}

/// Evaluates whether `character` is a Unicode bidirectional formatting control character.
#[inline]
#[must_use]
pub const fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
    )
}

/// Evaluates whether `value` is empty or represents an explicit empty quote pair (`""` or `''`).
#[inline]
#[must_use]
pub fn is_empty_value_literal(value: &str) -> bool {
    value.is_empty() || value == "\"\"" || value == "''"
}

/// Evaluates whether `byte` is a valid continuation byte in an ASCII identifier (`[A-Za-z0-9_-]`).
#[inline]
#[must_use]
pub const fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit() || byte == b'-'
}

/// Evaluates whether `byte` is a valid leading byte in an ASCII identifier (`[A-Za-z_]`).
#[inline]
#[must_use]
pub const fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

/// Evaluates whether `identifier` is a valid PSL or group identifier (`[A-Za-z_][A-Za-z0-9_-]*`).
#[must_use]
pub fn is_valid_identifier(identifier: &str) -> bool {
    let Some((&first, rest)) = identifier.as_bytes().split_first() else {
        return false;
    };
    is_identifier_start(first) && rest.iter().copied().all(is_identifier_continue)
}

/// Evaluates whether `identifier` is a valid recipe identifier (`[A-Za-z0-9_][A-Za-z0-9_-]*`).
#[must_use]
pub fn is_valid_recipe_identifier(identifier: &str) -> bool {
    let Some((&first, rest)) = identifier.as_bytes().split_first() else {
        return false;
    };
    (first.is_ascii_alphanumeric() || first == b'_')
        && rest.iter().copied().all(is_identifier_continue)
}

/// Parses a non-empty ASCII decimal digit slice into a `u32`, rejecting signs and non-digits.
#[must_use]
pub fn parse_ascii_u32(segment: &str) -> Option<u32> {
    if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    segment.parse::<u32>().ok()
}

/// Replaces control characters and bidirectional formatting characters with an ASCII space.
#[inline]
#[must_use]
pub const fn sanitize_char(character: char) -> char {
    if character.is_control() || is_bidi_control(character) {
        return ' ';
    }
    character
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_utf8_prefix, contains_control_or_whitespace, is_bidi_control,
        is_empty_value_literal, is_identifier_continue, is_identifier_start, is_valid_identifier,
        is_valid_recipe_identifier, parse_ascii_u32, sanitize_char,
    };

    #[test]
    fn clamp_utf8_prefix_respects_character_boundaries() {
        assert_eq!(clamp_utf8_prefix("hello", 10), "hello");
        assert_eq!(clamp_utf8_prefix("hello", 5), "hello");
        assert_eq!(clamp_utf8_prefix("hello", 3), "hel");
        assert_eq!(clamp_utf8_prefix("hello", 0), "");

        let multi_byte = "aäö";
        assert_eq!(clamp_utf8_prefix(multi_byte, 1), "a");
        assert_eq!(clamp_utf8_prefix(multi_byte, 2), "a");
        assert_eq!(clamp_utf8_prefix(multi_byte, 3), "aä");
        assert_eq!(clamp_utf8_prefix(multi_byte, 4), "aä");
        assert_eq!(clamp_utf8_prefix(multi_byte, 5), "aäö");
        assert_eq!(clamp_utf8_prefix("ä", 1), "");
    }

    #[test]
    fn control_whitespace_bidi_and_sanitization_checks() {
        assert!(!contains_control_or_whitespace("example-app_1"));
        assert!(contains_control_or_whitespace("example app"));
        assert!(contains_control_or_whitespace("example\tapp"));
        assert!(contains_control_or_whitespace("example\napp"));
        assert!(contains_control_or_whitespace("example\0app"));
        assert!(contains_control_or_whitespace("example\u{202E}app"));
        assert!(contains_control_or_whitespace("example\u{2066}app"));

        assert!(is_bidi_control('\u{202E}'));
        assert!(is_bidi_control('\u{2066}'));
        assert!(!is_bidi_control('a'));

        assert_eq!(sanitize_char('A'), 'A');
        assert_eq!(sanitize_char('\n'), ' ');
        assert_eq!(sanitize_char('\x1b'), ' ');
        assert_eq!(sanitize_char('\u{202E}'), ' ');

        assert!(is_empty_value_literal(""));
        assert!(is_empty_value_literal("\"\""));
        assert!(is_empty_value_literal("''"));
        assert!(!is_empty_value_literal("a"));
        assert!(!is_empty_value_literal("\"'\""));
    }

    #[test]
    fn identifier_and_ascii_u32_validation() {
        assert!(is_identifier_start(b'a'));
        assert!(is_identifier_start(b'Z'));
        assert!(is_identifier_start(b'_'));
        assert!(!is_identifier_start(b'0'));
        assert!(!is_identifier_start(b'-'));

        assert!(is_identifier_continue(b'a'));
        assert!(is_identifier_continue(b'9'));
        assert!(is_identifier_continue(b'_'));
        assert!(is_identifier_continue(b'-'));
        assert!(!is_identifier_continue(b'.'));

        assert!(is_valid_identifier("backend_1-prod"));
        assert!(is_valid_identifier("_private"));
        assert!(!is_valid_identifier(""));
        assert!(!is_valid_identifier("1backend"));
        assert!(!is_valid_identifier("-backend"));
        assert!(!is_valid_identifier("back.end"));

        assert!(is_valid_recipe_identifier("10-database"));
        assert!(is_valid_recipe_identifier("example-app_1"));
        assert!(!is_valid_recipe_identifier(""));
        assert!(!is_valid_recipe_identifier("-bad"));
        assert!(!is_valid_recipe_identifier("bad.pod"));

        assert_eq!(parse_ascii_u32("0"), Some(0));
        assert_eq!(parse_ascii_u32("580"), Some(580));
        assert_eq!(parse_ascii_u32(""), None);
        assert_eq!(parse_ascii_u32("+5"), None);
        assert_eq!(parse_ascii_u32("-5"), None);
        assert_eq!(parse_ascii_u32("5a"), None);
        assert_eq!(parse_ascii_u32("999999999999999999999"), None);
    }
}
