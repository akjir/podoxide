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

use std::borrow::Cow;

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

/// Evaluates whether `value` contains any control or Unicode bidirectional formatting character.
#[must_use]
pub fn contains_control_or_bidi(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() || is_bidi_control(character))
}

/// Evaluates whether `value` contains any whitespace, control, or bidirectional formatting character.
#[must_use]
pub fn contains_control_or_whitespace(value: &str) -> bool {
    value.chars().any(|character| {
        character.is_whitespace() || character.is_control() || is_bidi_control(character)
    })
}

/// Escapes `value` with single quotes for safe command display when it is empty or contains non-token characters.
#[must_use]
pub fn escape_shell(value: &str) -> Cow<'_, str> {
    if value.is_empty() {
        return Cow::Borrowed("''");
    }
    if value.bytes().all(is_safe_shell_byte) {
        return Cow::Borrowed(value);
    }
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('\'');
    for character in value.chars() {
        if character == '\'' {
            escaped.push_str("'\\''");
        } else {
            escaped.push(character);
        }
    }
    escaped.push('\'');
    Cow::Owned(escaped)
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

/// Evaluates whether `byte` can appear unquoted in a formatted shell command token (`[A-Za-z0-9_\-./:=@]`).
#[inline]
#[must_use]
pub const fn is_safe_shell_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(byte, b'_' | b'-' | b'.' | b'/' | b':' | b'=' | b'@')
}

/// Evaluates whether `target` is a valid `@<group>` reference or recipe stem identifier.
#[must_use]
pub fn is_valid_group_target(target: &str) -> bool {
    if let Some(group_name) = target.strip_prefix('@') {
        return is_valid_identifier(group_name);
    }
    is_valid_recipe_identifier(target)
}

/// Evaluates whether `identifier` is a valid PSL or group identifier (`[A-Za-z_][A-Za-z0-9_-]*`).
#[must_use]
pub fn is_valid_identifier(identifier: &str) -> bool {
    let Some((&first, rest)) = identifier.as_bytes().split_first() else {
        return false;
    };
    is_identifier_start(first) && rest.iter().copied().all(is_identifier_continue)
}

/// Evaluates whether `name` is a valid normalized pod or container name (`[a-z0-9_][a-z0-9_.-]*` without `..`).
#[must_use]
pub fn is_valid_pod_or_container_name(name: &str) -> bool {
    let Some((&first, rest)) = name.as_bytes().split_first() else {
        return false;
    };
    let valid_first = first.is_ascii_lowercase() || first.is_ascii_digit() || first == b'_';
    let valid_rest = rest.iter().copied().all(|byte| {
        byte.is_ascii_lowercase()
            || byte.is_ascii_digit()
            || byte == b'_'
            || byte == b'-'
            || byte == b'.'
    });
    valid_first && valid_rest && !name.contains("..")
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

/// Evaluates whether `segment` is a safe, non-empty CLI target component without path separators, leading dashes, `@`, or control/whitespace characters.
#[must_use]
pub fn is_valid_target_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.starts_with('-')
        && !segment.starts_with('@')
        && !segment.contains('/')
        && !contains_control_or_whitespace(segment)
}

/// Evaluates whether `value` contains at least one non-whitespace character and is free of control and Unicode BiDi characters.
#[must_use]
pub fn is_valid_text(value: &str) -> bool {
    !value.trim().is_empty() && !contains_control_or_bidi(value)
}

/// Evaluates whether `value` is a non-empty single token without leading hyphens, whitespace, or control characters.
#[must_use]
pub fn is_valid_token(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-') && !contains_control_or_whitespace(value)
}

/// Normalizes `input` by trimming whitespace, lowercasing characters, and collapsing internal whitespace runs into `'_'`.
#[must_use]
pub fn normalize_name(input: &str) -> String {
    let trimmed = input.trim();
    let mut normalized = String::with_capacity(trimmed.len());
    let mut in_whitespace = false;
    for character in trimmed.chars() {
        append_normalized_char(&mut normalized, character, &mut in_whitespace);
    }
    normalized
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

/// Clamps `input` to `max_bytes` on a UTF-8 character boundary and replaces control and bidirectional characters with ASCII spaces.
#[must_use]
pub fn sanitize_text(input: &str, max_bytes: usize) -> String {
    let clamped = clamp_utf8_prefix(input, max_bytes);
    let mut sanitized = String::with_capacity(clamped.len());
    for character in clamped.chars() {
        sanitized.push(sanitize_char(character));
    }
    sanitized
}

/// Appends `character` (lowercased, or collapsed to `'_'` on whitespace transitions) to `normalized`.
fn append_normalized_char(normalized: &mut String, character: char, in_whitespace: &mut bool) {
    if character.is_whitespace() {
        if !*in_whitespace {
            normalized.push('_');
            *in_whitespace = true;
        }
        return;
    }
    *in_whitespace = false;
    for lower in character.to_lowercase() {
        normalized.push(lower);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_utf8_prefix, contains_control_or_bidi, contains_control_or_whitespace, escape_shell,
        is_bidi_control, is_empty_value_literal, is_identifier_continue, is_identifier_start,
        is_safe_shell_byte, is_valid_group_target, is_valid_identifier,
        is_valid_pod_or_container_name, is_valid_recipe_identifier, is_valid_target_segment,
        is_valid_text, is_valid_token, normalize_name, parse_ascii_u32, sanitize_char,
        sanitize_text,
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
        assert!(!contains_control_or_bidi("example app"));
        assert!(contains_control_or_bidi("example\napp"));
        assert!(contains_control_or_bidi("example\u{202E}app"));

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
        assert_eq!(sanitize_text("ok\nbad\u{202E}end", 6), "ok bad");

        assert!(is_empty_value_literal(""));
        assert!(is_empty_value_literal("\"\""));
        assert!(is_empty_value_literal("''"));
        assert!(!is_empty_value_literal("a"));
        assert!(!is_empty_value_literal("\"'\""));

        assert!(is_safe_shell_byte(b'a'));
        assert!(is_safe_shell_byte(b'@'));
        assert!(!is_safe_shell_byte(b' '));
        assert!(!is_safe_shell_byte(b'\''));
        assert_eq!(escape_shell(""), "''");
        assert_eq!(
            escape_shell("registry.io/alpine:latest"),
            "registry.io/alpine:latest"
        );
        assert_eq!(escape_shell("O'Hara"), "'O'\\''Hara'");
        assert_eq!(escape_shell("Hancock 2"), "'Hancock 2'");
        assert_eq!(
            escape_shell("echo 'hello world'"),
            "'echo '\\''hello world'\\'''"
        );

        assert!(is_valid_text("Example Web Stack"));
        assert!(!is_valid_text(""));
        assert!(!is_valid_text("   "));
        assert!(!is_valid_text("bad\ntext"));
        assert!(!is_valid_text("bad\u{202E}text"));

        assert!(is_valid_token("docker.io/alpine:latest"));
        assert!(!is_valid_token(""));
        assert!(!is_valid_token("-flag"));
        assert!(!is_valid_token("two tokens"));
        assert!(!is_valid_token("bad\ntoken"));
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

        assert!(is_valid_group_target("@backend_1"));
        assert!(is_valid_group_target("10-database"));
        assert!(!is_valid_group_target("@1backend"));
        assert!(!is_valid_group_target("@"));
        assert!(!is_valid_group_target("bad.pod"));

        assert!(is_valid_target_segment("example-app"));
        assert!(is_valid_target_segment("*db"));
        assert!(!is_valid_target_segment(""));
        assert!(!is_valid_target_segment("."));
        assert!(!is_valid_target_segment(".."));
        assert!(!is_valid_target_segment("-flag"));
        assert!(!is_valid_target_segment("@group"));
        assert!(!is_valid_target_segment("a/b"));
        assert!(!is_valid_target_segment("a b"));

        assert_eq!(parse_ascii_u32("0"), Some(0));
        assert_eq!(parse_ascii_u32("580"), Some(580));
        assert_eq!(parse_ascii_u32(""), None);
        assert_eq!(parse_ascii_u32("+5"), None);
        assert_eq!(parse_ascii_u32("-5"), None);
        assert_eq!(parse_ascii_u32("5a"), None);
        assert_eq!(parse_ascii_u32("999999999999999999999"), None);

        assert_eq!(normalize_name("      si  po"), "si_po");
        assert_eq!(normalize_name(" supR App  "), "supr_app");
        assert_eq!(normalize_name("  *DB\t\tServer "), "*db_server");
        assert_eq!(normalize_name(""), "");
        assert_eq!(normalize_name("   "), "");

        assert!(is_valid_pod_or_container_name("example-stack_1.0"));
        assert!(is_valid_pod_or_container_name("10_db"));
        assert!(is_valid_pod_or_container_name("_internal"));
        assert!(!is_valid_pod_or_container_name(""));
        assert!(!is_valid_pod_or_container_name("-flag"));
        assert!(!is_valid_pod_or_container_name(".hidden"));
        assert!(!is_valid_pod_or_container_name("*db"));
        assert!(!is_valid_pod_or_container_name("Upper"));
        assert!(!is_valid_pod_or_container_name("a..b"));
        assert!(!is_valid_pod_or_container_name("a/b"));
        assert!(!is_valid_pod_or_container_name("a:b"));
    }
}
