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

//! Unit tests for option token splitting, quoting validation, and bounded capacities in [`super`].

use super::{MAX_COLLECTION_PREALLOC, append_option_tokens, bounded_capacity, is_valid_option_value};

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
        "'--'",
        "\"--\"",
        "'-'",
        "\"-\"",
        "'  --  '",
        "'='",
        "'=value'",
        "'--=value'",
        "'-=value'",
        "'--flag='",
        "\"--flag=\"",
        "'--flag=   '",
        "--read-only --",
        "--read-only -",
        "--read-only '--'",
        "--read-only \"--\"",
        "--read-only '-'",
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
