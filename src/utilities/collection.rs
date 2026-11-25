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

//! Slice and vector helpers for option token splitting, order-preserving deduplication, and bounded pre-allocation.

use std::borrow::Cow;
use std::collections::BTreeSet;

use crate::utilities::string;

/// Maximum initial capacity pre-allocated for dynamically sized collections.
pub const MAX_COLLECTION_PREALLOC: usize = 1024;

/// Appends `option` directive tokens from `options` into `arguments`, splitting combined `--flag value` entries.
pub fn append_option_tokens(arguments: &mut Vec<String>, options: &[String]) {
    for option in options {
        append_single_option(arguments, option);
    }
}

/// Returns `requested` clamped to `maximum` for bounded collection pre-allocation.
#[inline]
#[must_use]
pub const fn bounded_capacity(requested: usize, maximum: usize) -> usize {
    if requested < maximum {
        requested
    } else {
        maximum
    }
}

/// Returns a new vector containing the unique strings from `items` in first-appearance order.
#[must_use]
pub fn dedup_preserve_order(items: &[String]) -> Vec<String> {
    if items.is_empty() {
        return Vec::new();
    }
    let capacity = bounded_capacity(items.len(), MAX_COLLECTION_PREALLOC);
    let mut result = Vec::with_capacity(capacity);
    let mut seen = BTreeSet::new();
    for item in items {
        if seen.insert(item.as_str()) {
            result.push(item.clone());
        }
    }
    result
}

/// Evaluates whether `value` is a valid `option` directive argument with well-formed text and balanced inner quotes.
#[must_use]
pub fn is_valid_option_value(value: &str) -> bool {
    if !string::is_valid_text(value) {
        return false;
    }
    let trimmed = value.trim();
    if trimmed.starts_with('=') {
        return false;
    }
    if starts_with_quote(trimmed) {
        if let Some(unquoted) = unquote_single_wrapped(trimmed) {
            return string::is_valid_text(&unquoted);
        }
        return has_valid_shell_quoting(trimmed);
    }
    if let Some((key, assigned)) = split_assigned_token(trimmed) {
        return is_valid_assigned_value(key, assigned);
    }
    if !trimmed.starts_with('-') {
        return has_valid_shell_quoting(trimmed);
    }
    if let Some((flag, rest)) = split_first_whitespace(trimmed) {
        return is_valid_flag_token(flag) && is_valid_option_rest(rest);
    }
    is_valid_flag_token(trimmed)
}

/// Appends `flag` and its whitespace-separated `rest` value (unquoting `'...'`, `"..."`, or `KEY='...'`) to `arguments`.
fn append_flag_and_rest(arguments: &mut Vec<String>, flag: &str, rest: &str) {
    arguments.push(flag.to_owned());
    if let Some(unquoted) = unquote_single_wrapped(rest) {
        arguments.push(unquoted.into_owned());
        return;
    }
    if let Some((key, value)) = split_assigned_token(rest) {
        arguments.push(format_assigned_token(key, value));
        return;
    }
    arguments.push(rest.to_owned());
}

/// Appends a single `option` directive token (or whitespace-split `--flag value` pair) to `arguments`.
fn append_single_option(arguments: &mut Vec<String>, option: &str) {
    let trimmed = option.trim();
    if let Some(unquoted) = unquote_single_wrapped(trimmed) {
        arguments.push(unquoted.into_owned());
        return;
    }
    if let Some((key, value)) = split_assigned_token(trimmed) {
        arguments.push(format_assigned_token(key, value));
        return;
    }
    if !trimmed.starts_with('-') {
        arguments.push(trimmed.to_owned());
        return;
    }
    if let Some((flag, rest)) = split_first_whitespace(trimmed) {
        append_flag_and_rest(arguments, flag, rest);
        return;
    }
    arguments.push(trimmed.to_owned());
}

/// Returns the byte length of the `\<char>` sequence starting at `backslash_index` in `text`.
#[must_use]
fn escape_char_byte_len(text: &str, backslash_index: usize) -> usize {
    let char_len = text
        .get(backslash_index + 1..)
        .and_then(|rest| rest.chars().next())
        .map_or(0, char::len_utf8);
    1 + char_len
}

/// Formats a `key=value` or `--flag=value` token, stripping matching outer quotes from `value` when present.
#[must_use]
fn format_assigned_token(key: &str, value: &str) -> String {
    let clean_value = unquote_single_wrapped(value).unwrap_or(Cow::Borrowed(value));
    format!("{key}={clean_value}")
}

/// Evaluates whether quotes in `input` are properly opened and closed (allowing `\"`/`\\` inside double quotes and intra-word apostrophes).
#[must_use]
fn has_valid_shell_quoting(input: &str) -> bool {
    let bytes = input.as_bytes();
    let mut cursor = 0;
    while let Some(&byte) = bytes.get(cursor) {
        if byte == b'\'' && is_inline_apostrophe(bytes, cursor) {
            cursor += 1;
            continue;
        }
        if byte == b'\'' || byte == b'"' {
            let Some(next_cursor) = scan_closed_quote_segment(input, cursor, byte) else {
                return false;
            };
            cursor = next_cursor;
            continue;
        }
        cursor += 1;
    }
    true
}

/// Evaluates whether the single quote at `index` in `bytes` is an intra-word apostrophe (for example, `don't`).
#[inline]
#[must_use]
fn is_inline_apostrophe(bytes: &[u8], index: usize) -> bool {
    let Some(prev) = index.checked_sub(1).and_then(|pos| bytes.get(pos)) else {
        return false;
    };
    let Some(next) = bytes.get(index + 1) else {
        return false;
    };
    prev.is_ascii_alphanumeric() && next.is_ascii_alphanumeric()
}

/// Evaluates whether the value side of a `key=value` or `--flag=value` option token is well-formed.
#[must_use]
fn is_valid_assigned_value(key: &str, assigned: &str) -> bool {
    let is_flag = key.starts_with('-');
    if assigned.is_empty() {
        return !is_flag;
    }
    if let Some(unquoted) = unquote_single_wrapped(assigned) {
        return !is_flag || string::is_valid_text(&unquoted);
    }
    has_valid_shell_quoting(assigned) && (!is_flag || string::is_valid_text(assigned))
}

/// Evaluates whether `flag` is a non-empty CLI flag token other than bare `-` or `--`.
#[must_use]
fn is_valid_flag_token(flag: &str) -> bool {
    flag.starts_with('-') && flag != "-" && flag != "--" && !flag.contains(['=', '"', '\''])
}

/// Evaluates whether the trailing value portion of a whitespace-separated `--flag <rest>` option is a valid single value.
#[must_use]
fn is_valid_option_rest(rest: &str) -> bool {
    if rest.starts_with('=') {
        return false;
    }
    if starts_with_quote(rest) {
        return unquote_single_wrapped(rest).is_some_and(|inner| string::is_valid_text(&inner));
    }
    if let Some((_, assigned)) = split_assigned_token(rest) {
        if starts_with_quote(assigned) {
            return unquote_single_wrapped(assigned).is_some();
        }
        return !assigned.contains(char::is_whitespace) && has_valid_shell_quoting(assigned);
    }
    !rest.contains(char::is_whitespace) && has_valid_shell_quoting(rest)
}

/// Scans a single- or double-quoted segment starting at `start` in `input` and returns the byte offset after the closing quote.
#[must_use]
fn scan_closed_quote_segment(input: &str, start: usize, quote: u8) -> Option<usize> {
    let bytes = input.as_bytes();
    let opens_cleanly =
        start == 0 || bytes[start - 1].is_ascii_whitespace() || bytes[start - 1] == b'=';
    if !opens_cleanly {
        return None;
    }
    let mut cursor = start + 1;
    while let Some(&byte) = bytes.get(cursor) {
        if quote == b'"' && byte == b'\\' {
            if cursor + 1 >= bytes.len() {
                return None;
            }
            cursor += escape_char_byte_len(input, cursor);
            continue;
        }
        if byte == quote {
            let after = cursor + 1;
            return bytes
                .get(after)
                .is_none_or(u8::is_ascii_whitespace)
                .then_some(after);
        }
        cursor += 1;
    }
    None
}

/// Splits `input` at the first `=` when the key or flag prefix is non-empty and free of whitespace and quotes.
#[must_use]
fn split_assigned_token(input: &str) -> Option<(&str, &str)> {
    let (key, value) = input.split_once('=')?;
    if key.is_empty()
        || key == "-"
        || key == "--"
        || key.contains(char::is_whitespace)
        || key.contains(['"', '\''])
    {
        return None;
    }
    Some((key, value))
}

/// Splits `input` at the first whitespace character into `(head, trimmed_tail)`.
#[must_use]
fn split_first_whitespace(input: &str) -> Option<(&str, &str)> {
    let (head, tail) = input.split_once(char::is_whitespace)?;
    Some((head, tail.trim_start()))
}

/// Evaluates whether `value` begins with an ASCII single or double quote (`'` or `"`).
#[inline]
#[must_use]
fn starts_with_quote(value: &str) -> bool {
    matches!(value.as_bytes().first(), Some(b'\'' | b'"'))
}

/// Unescapes `\"` and `\\` sequences inside a validated double-quoted option payload.
#[must_use]
fn unescape_double_quoted(inner: &str) -> String {
    let mut output = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(character) = chars.next() {
        if character == '\\'
            && let Some(next) = chars.next()
        {
            if next == '"' || next == '\\' {
                output.push(next);
            } else {
                output.push('\\');
                output.push(next);
            }
            continue;
        }
        output.push(character);
    }
    output
}

/// Validates and unquotes the interior `inner` of a double-quoted option token, handling `\"` and `\\` escapes.
#[must_use]
fn unquote_double_wrapped(inner: &str) -> Option<Cow<'_, str>> {
    let bytes = inner.as_bytes();
    let mut cursor = 0;
    let mut has_escape = false;
    while let Some(&byte) = bytes.get(cursor) {
        if byte == b'\\' {
            let &next = bytes.get(cursor + 1)?;
            if next == b'"' || next == b'\\' {
                has_escape = true;
            }
            cursor += escape_char_byte_len(inner, cursor);
            continue;
        }
        if byte == b'"' {
            return None;
        }
        cursor += 1;
    }
    if has_escape {
        Some(Cow::Owned(unescape_double_quoted(inner)))
    } else {
        Some(Cow::Borrowed(inner))
    }
}

/// Strips a single outer `'...'` or `"..."` pair spanning the entire `value`, unescaping `\"`/`\\` inside double quotes.
#[must_use]
fn unquote_single_wrapped(value: &str) -> Option<Cow<'_, str>> {
    if let Some(inner) = value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return (!inner.contains('\'')).then_some(Cow::Borrowed(inner));
    }
    if let Some(inner) = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        return unquote_double_wrapped(inner);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_COLLECTION_PREALLOC, append_option_tokens, bounded_capacity, dedup_preserve_order,
        is_valid_option_value,
    };

    #[test]
    fn append_option_tokens_splits_flags_unescapes_quotes_and_trims_positionals() {
        let mut arguments = Vec::new();
        let options = vec![
            String::from("--unknown thing"),
            String::from("--another"),
            String::from("  unknown value  "),
            String::from("'quoted positional'"),
        ];
        append_option_tokens(&mut arguments, &options);
        assert_eq!(
            arguments,
            vec![
                "--unknown",
                "thing",
                "--another",
                "unknown value",
                "quoted positional"
            ]
        );

        for assigned_option in [
            "--health-cmd='redis-cli ping | grep -q PONG || exit 1'",
            "--health-cmd=\"redis-cli ping | grep -q PONG || exit 1\"",
            "--health-cmd=redis-cli ping | grep -q PONG || exit 1",
        ] {
            let mut assigned_args = Vec::new();
            append_option_tokens(&mut assigned_args, &[String::from(assigned_option)]);
            assert_eq!(
                assigned_args,
                vec!["--health-cmd=redis-cli ping | grep -q PONG || exit 1"]
            );
        }

        for separated_options in [
            vec![
                String::from("--health-cmd"),
                String::from("redis-cli ping | grep -q PONG || exit 1"),
            ],
            vec![String::from(
                "--health-cmd 'redis-cli ping | grep -q PONG || exit 1'",
            )],
            vec![String::from(
                "--health-cmd \"redis-cli ping | grep -q PONG || exit 1\"",
            )],
        ] {
            let mut separated_args = Vec::new();
            append_option_tokens(&mut separated_args, &separated_options);
            assert_eq!(
                separated_args,
                vec!["--health-cmd", "redis-cli ping | grep -q PONG || exit 1"]
            );
        }

        let mut env_args = Vec::new();
        append_option_tokens(
            &mut env_args,
            &[
                String::from("--env FOO='bar baz'"),
                String::from("--env"),
                String::from("BAR='baz qux'"),
                String::from("  --trimmed-single  "),
                String::from("--env \"MSG=don't\""),
                String::from("--env FOO="),
                String::from("--env FOO=''"),
                String::from("--health-cmd=\"printf '%s' \\\"a\\\\\\\"b\\\"\""),
                String::from("--health-cmd='\"/bin/check\" \"--ok\"'"),
                String::from("--health-cmd=\"/bin/check\" \"--ok\""),
                String::from("  foo bar  "),
            ],
        );
        assert_eq!(
            env_args,
            vec![
                "--env",
                "FOO=bar baz",
                "--env",
                "BAR=baz qux",
                "--trimmed-single",
                "--env",
                "MSG=don't",
                "--env",
                "FOO=",
                "--env",
                "FOO=",
                "--health-cmd=printf '%s' \"a\\\"b\"",
                "--health-cmd=\"/bin/check\" \"--ok\"",
                "--health-cmd=\"/bin/check\" \"--ok\"",
                "foo bar",
            ]
        );
    }

    #[test]
    fn bounded_capacity_clamps_to_maximum() {
        assert_eq!(bounded_capacity(0, MAX_COLLECTION_PREALLOC), 0);
        assert_eq!(bounded_capacity(16, MAX_COLLECTION_PREALLOC), 16);
        assert_eq!(
            bounded_capacity(MAX_COLLECTION_PREALLOC, MAX_COLLECTION_PREALLOC),
            MAX_COLLECTION_PREALLOC
        );
        assert_eq!(
            bounded_capacity(MAX_COLLECTION_PREALLOC + 500, MAX_COLLECTION_PREALLOC),
            MAX_COLLECTION_PREALLOC
        );
    }

    #[test]
    fn dedup_preserve_order_retains_first_appearance() {
        let empty: Vec<String> = Vec::new();
        assert!(dedup_preserve_order(&empty).is_empty());

        let items = vec![
            String::from("lie"),
            String::from("cake"),
            String::from("the"),
            String::from("cake"),
            String::from("lie"),
        ];
        assert_eq!(
            dedup_preserve_order(&items),
            vec![
                String::from("lie"),
                String::from("cake"),
                String::from("the"),
            ]
        );
    }

    #[test]
    fn is_valid_option_value_validates_quoting_escapes_apostrophes_and_flag_forms() {
        for valid in [
            "--health-cmd='redis-cli ping | grep -q PONG || exit 1'",
            "--health-cmd=\"redis-cli ping | grep -q PONG || exit 1\"",
            "--health-cmd=redis-cli ping | grep -q PONG || exit 1",
            "--health-cmd 'redis-cli ping | grep -q PONG || exit 1'",
            "--health-cmd \"redis-cli ping | grep -q PONG || exit 1\"",
            "--health-cmd=\"printf '%s' \\\"a\\\\\\\"b\\\"\"",
            "--health-cmd=printf '%s' \"a\\\"b\"",
            "--health-cmd='\"/bin/check\" \"--ok\"'",
            "--health-cmd=\"/bin/check\" \"--ok\"",
            "\"/bin/check\" \"--ok\"",
            "--health-cmd",
            "redis-cli ping | grep -q PONG || exit 1",
            "'quoted value'",
            "\"quoted value\"",
            "--env \"MSG=don't\"",
            "--env MSG=don't",
            "MSG=don't",
            "don't panic",
            "--env FOO='bar baz'",
            "--env FOO=bar",
            "--env FOO=",
            "--env FOO=''",
            "FOO='bar baz'",
            "FOO=bar",
            "FOO=",
            "FOO=''",
        ] {
            assert!(is_valid_option_value(valid), "expected valid: {valid}");
        }

        for invalid in [
            "",
            "   ",
            "-",
            "--",
            "=",
            "=value",
            "- value",
            "-- value",
            "-- evil-image /bin/sh",
            "''",
            "\"\"",
            "'   '",
            "\"   \"",
            "--flag ''",
            "--flag \"\"",
            "--flag '   '",
            "--flag \"   \"",
            "--flag=''",
            "--flag=\"\"",
            "--flag=",
            "--flag='   '",
            "--flag =value",
            "--flag val --privileged",
            "--env FOO=bar --privileged",
            "--env FOO=bar 'baz qux'",
            "--env FOO=bar'baz qux'",
            "FOO='unterminated",
            "FOO=bar'",
            "FOO=bar\"baz",
            "redis-cli 'unterminated",
            "--flag\0bad",
            "--health-cmd='redis-cli ping",
            "--health-cmd=\"redis-cli ping",
            "--health-cmd=\"trailing\\\"",
            "--health-cmd='redis-cli'extra",
            "--health-cmd 'redis-cli ping",
            "--health-cmd \"redis-cli ping",
            "--health-cmd 'redis-cli'extra",
            "--health'cmd value",
            "--health\"cmd",
            "--=value",
            "-=value",
            "'unterminated",
            "\"unterminated",
            "'extra'tail",
            "--env FOO='unterminated",
            "--env FOO=bar'",
            "--env FOO=bar\"baz",
        ] {
            assert!(
                !is_valid_option_value(invalid),
                "expected invalid: {invalid}"
            );
        }
    }
}
