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

use std::collections::BTreeSet;

use crate::utilities::string::is_valid_text;

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
    if !is_valid_text(value) {
        return false;
    }
    let trimmed = value.trim();
    if starts_with_quote(trimmed) {
        return strip_matching_quotes(trimmed).is_some_and(is_valid_text);
    }
    if let Some((_, assigned)) = split_assigned_token(trimmed) {
        return is_valid_assigned_value(assigned);
    }
    if !trimmed.starts_with('-') {
        return has_balanced_quotes(trimmed);
    }
    if let Some((flag, rest)) = split_first_whitespace(trimmed) {
        return is_valid_flag_token(flag) && is_valid_option_rest(rest);
    }
    is_valid_flag_token(trimmed)
}

/// Appends `flag` and its whitespace-separated `rest` value (unquoting `'...'`, `"..."`, or `KEY='...'`) to `arguments`.
fn append_flag_and_rest(arguments: &mut Vec<String>, flag: &str, rest: &str) {
    arguments.push(flag.to_owned());
    if let Some(unquoted) = strip_matching_quotes(rest) {
        arguments.push(unquoted.to_owned());
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
    if let Some(unquoted) = strip_matching_quotes(trimmed) {
        arguments.push(unquoted.to_owned());
        return;
    }
    if let Some((key, value)) = split_assigned_token(trimmed) {
        arguments.push(format_assigned_token(key, value));
        return;
    }
    if !trimmed.starts_with('-') {
        arguments.push(option.to_owned());
        return;
    }
    if let Some((flag, rest)) = split_first_whitespace(trimmed) {
        append_flag_and_rest(arguments, flag, rest);
        return;
    }
    arguments.push(trimmed.to_owned());
}

/// Formats a `key=value` or `--flag=value` token, stripping matching outer quotes from `value` when present.
#[must_use]
fn format_assigned_token(key: &str, value: &str) -> String {
    let clean_value = strip_matching_quotes(value).unwrap_or(value);
    format!("{key}={clean_value}")
}

/// Evaluates whether all single and double quotes in `input` are properly paired.
#[must_use]
fn has_balanced_quotes(input: &str) -> bool {
    let mut active_quote = 0_u8;
    for byte in input.bytes() {
        if active_quote == 0 {
            if byte == b'\'' || byte == b'"' {
                active_quote = byte;
            }
        } else if byte == active_quote {
            active_quote = 0;
        }
    }
    active_quote == 0
}

/// Evaluates whether the value side of a `key=value` option token has valid quoting.
#[must_use]
fn is_valid_assigned_value(assigned: &str) -> bool {
    if starts_with_quote(assigned) {
        strip_matching_quotes(assigned).is_some()
    } else {
        has_balanced_quotes(assigned)
    }
}

/// Evaluates whether `flag` is a non-empty CLI flag token other than bare `-` or `--`.
#[must_use]
fn is_valid_flag_token(flag: &str) -> bool {
    flag.starts_with('-') && flag != "-" && flag != "--" && !flag.contains(['=', '"', '\''])
}

/// Evaluates whether the trailing value portion of a whitespace-separated `--flag <rest>` option is a valid single value.
#[must_use]
fn is_valid_option_rest(rest: &str) -> bool {
    if starts_with_quote(rest) {
        return strip_matching_quotes(rest).is_some_and(is_valid_text);
    }
    if let Some((_, assigned)) = split_assigned_token(rest) {
        if starts_with_quote(assigned) {
            return strip_matching_quotes(assigned).is_some();
        }
        return !assigned.contains(char::is_whitespace) && !assigned.contains(['"', '\'']);
    }
    !rest.contains(char::is_whitespace) && !rest.contains(['"', '\''])
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

/// Strips a single pair of matching outer `'...'` or `"..."` quotes when no matching quote appears inside.
#[must_use]
fn strip_matching_quotes(value: &str) -> Option<&str> {
    for quote in ['\'', '"'] {
        if let Some(inner) = value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
        {
            return (!inner.contains(quote)).then_some(inner);
        }
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
    fn append_option_tokens_splits_combined_flags_and_preserves_positional_values() {
        let mut arguments = Vec::new();
        let options = vec![
            String::from("--unknown thing"),
            String::from("--another"),
            String::from("unknown value"),
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
            ],
        );
        assert_eq!(
            env_args,
            vec!["--env", "FOO=bar baz", "--env", "BAR=baz qux", "--trimmed-single"]
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
    fn is_valid_option_value_validates_quoting_and_flag_forms() {
        for valid in [
            "--health-cmd='redis-cli ping | grep -q PONG || exit 1'",
            "--health-cmd=\"redis-cli ping | grep -q PONG || exit 1\"",
            "--health-cmd=redis-cli ping | grep -q PONG || exit 1",
            "--health-cmd 'redis-cli ping | grep -q PONG || exit 1'",
            "--health-cmd \"redis-cli ping | grep -q PONG || exit 1\"",
            "--health-cmd",
            "redis-cli ping | grep -q PONG || exit 1",
            "'quoted value'",
            "\"quoted value\"",
            "--env FOO='bar baz'",
            "--env FOO=bar",
            "FOO='bar baz'",
            "FOO=bar",
        ] {
            assert!(is_valid_option_value(valid), "expected valid: {valid}");
        }

        for invalid in [
            "",
            "   ",
            "-",
            "--",
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
            "--flag val --privileged",
            "--env FOO=bar --privileged",
            "--env FOO=bar 'baz qux'",
            "--env FOO=bar'baz qux'",
            "FOO='unterminated",
            "FOO=bar'baz",
            "redis-cli 'unterminated",
            "--flag\0bad",
            "--health-cmd='redis-cli ping",
            "--health-cmd=\"redis-cli ping",
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
            "--env FOO=bar'baz",
        ] {
            assert!(
                !is_valid_option_value(invalid),
                "expected invalid: {invalid}"
            );
        }
    }
}
