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

//! PSL resource-bound, syntax-error, column-accuracy, and diagnostic unit tests.

use crate::error::{PslError, PslErrorKind};
use super::{
    MAX_ARGS_PER_STMT, MAX_BLOCKS_PER_DOC, MAX_FILE_BYTES, MAX_NESTING_DEPTH, MAX_STMTS_PER_BLOCK,
    parse_psl,
};

#[test]
fn diagnostic_columns_highlight_exact_offending_tokens() {
    let trailing_input = "pod app {} trailing\n";
    let trailing_error = parse_psl(trailing_input)
        .unwrap_err()
        .with_diagnostic(trailing_input, "./recipe.pod");
    assert_eq!(trailing_error.kind, PslErrorKind::UnexpectedToken);
    assert_eq!(trailing_error.line, 1);
    assert_eq!(trailing_error.column, Some(12));
    let trailing_diagnostic = trailing_error.diagnostic.as_ref().unwrap();
    assert_eq!(trailing_diagnostic.col_number, 12);
    assert_eq!(trailing_diagnostic.annotation_carets, "           ^^^^^^^^");

    let double_brace_input = "pod app{ other {\n";
    let brace_error = parse_psl(double_brace_input)
        .unwrap_err()
        .with_diagnostic(double_brace_input, "./recipe.pod");
    assert_eq!(brace_error.kind, PslErrorKind::InvalidBlockHeader);
    assert_eq!(brace_error.line, 1);
    assert_eq!(brace_error.column, Some(8));
    let brace_diagnostic = brace_error.diagnostic.as_ref().unwrap();
    assert_eq!(brace_diagnostic.col_number, 8);
    assert_eq!(brace_diagnostic.annotation_carets, "       ^");

    let utf8_trailing_input = "pod äöü {} trailing\n";
    let utf8_error = parse_psl(utf8_trailing_input)
        .unwrap_err()
        .with_diagnostic(utf8_trailing_input, "./recipe.pod");
    let utf8_diagnostic = utf8_error.diagnostic.as_ref().unwrap();
    assert_eq!(utf8_diagnostic.col_number, 12);
    assert_eq!(utf8_diagnostic.annotation_carets, "           ^^^^^^^^");
}

#[test]
fn enforce_all_resource_limits_and_prescan_bounds() {
    // 1. MAX_FILE_BYTES + 1
    let oversized = " ".repeat(MAX_FILE_BYTES + 1);
    assert_eq!(
        parse_psl(&oversized),
        Err(PslError::new(PslErrorKind::LimitExceeded, 1))
    );

    // 2. MAX_NESTING_DEPTH (8 succeeds, 9 fails; inline {} checked at boundary too)
    let valid_depth = "b {\n".repeat(MAX_NESTING_DEPTH) + &"}\n".repeat(MAX_NESTING_DEPTH);
    assert!(parse_psl(&valid_depth).is_ok());

    let valid_inline_depth = format!(
        "{}leaf {{}}\n{}",
        "b {\n".repeat(MAX_NESTING_DEPTH - 1),
        "}\n".repeat(MAX_NESTING_DEPTH - 1)
    );
    assert!(parse_psl(&valid_inline_depth).is_ok());

    let excessive_depth =
        "b {\n".repeat(MAX_NESTING_DEPTH + 1) + &"}\n".repeat(MAX_NESTING_DEPTH + 1);
    assert_eq!(
        parse_psl(&excessive_depth),
        Err(PslError::with_column(
            PslErrorKind::LimitExceeded,
            MAX_NESTING_DEPTH + 1,
            3,
        ))
    );

    let excessive_inline_depth = format!(
        "{}leaf {{}}\n{}",
        "b {\n".repeat(MAX_NESTING_DEPTH),
        "}\n".repeat(MAX_NESTING_DEPTH)
    );
    assert_eq!(
        parse_psl(&excessive_inline_depth),
        Err(PslError::with_column(
            PslErrorKind::LimitExceeded,
            MAX_NESTING_DEPTH + 1,
            6,
        ))
    );

    // 3. MAX_BLOCKS_PER_DOC (256 succeeds, 257 fails BEFORE scanning malformed args)
    let valid_blocks = "b {}\n".repeat(MAX_BLOCKS_PER_DOC);
    assert!(parse_psl(&valid_blocks).is_ok());

    let valid_256th_with_directive = format!(
        "{}b {{\n    k v\n}}\n",
        "b {}\n".repeat(MAX_BLOCKS_PER_DOC - 1)
    );
    assert!(parse_psl(&valid_256th_with_directive).is_ok());

    let excessive_root_blocks = format!(
        "{}pod \"unterminated\n",
        "pod {}\n".repeat(MAX_BLOCKS_PER_DOC)
    );
    assert_eq!(
        parse_psl(&excessive_root_blocks),
        Err(PslError::with_column(
            PslErrorKind::LimitExceeded,
            MAX_BLOCKS_PER_DOC + 1,
            1,
        )),
        "257th root block must fail with LimitExceeded before scanning malformed arguments"
    );

    let excessive_nested_block = format!(
        "{}b {{\n    sub {{}}\n}}\n",
        "b {}\n".repeat(MAX_BLOCKS_PER_DOC - 1)
    );
    assert_eq!(
        parse_psl(&excessive_nested_block),
        Err(PslError::with_column(
            PslErrorKind::LimitExceeded,
            MAX_BLOCKS_PER_DOC + 1,
            9,
        ))
    );

    // 4. MAX_STMTS_PER_BLOCK (512 succeeds, 513 fails before scanning 513th statement)
    let valid_stmts = format!("b {{\n{}}}\n", "k v\n".repeat(MAX_STMTS_PER_BLOCK));
    assert!(parse_psl(&valid_stmts).is_ok());

    let excessive_stmts = format!(
        "b {{\n{}123invalid \"unterminated\n}}\n",
        "k v\n".repeat(MAX_STMTS_PER_BLOCK)
    );
    assert_eq!(
        parse_psl(&excessive_stmts),
        Err(PslError::with_column(
            PslErrorKind::LimitExceeded,
            MAX_STMTS_PER_BLOCK + 2,
            1,
        ))
    );

    // 5. MAX_ARGS_PER_STMT (64 succeeds, 65 fails before scanning 65th argument)
    let valid_args = format!("b {{\n    k{}\n}}\n", " a".repeat(MAX_ARGS_PER_STMT));
    assert!(parse_psl(&valid_args).is_ok());

    let excessive_args = format!(
        "b {{\n    k{} \"unterminated\n}}\n",
        " a".repeat(MAX_ARGS_PER_STMT)
    );
    assert_eq!(
        parse_psl(&excessive_args),
        Err(PslError::with_column(
            PslErrorKind::LimitExceeded,
            2,
            7 + (MAX_ARGS_PER_STMT * 2),
        ))
    );
}

#[test]
fn reject_invalid_block_headers_and_root_or_empty_directives() {
    let bad_headers = [
        ("general{\n}\n", 8),
        ("pod app{\n}\n", 8),
        ("pod app{ other {\n", 8),
        ("pod \"app\"{\n}\n", 10),
        ("pod 'app'{\n}\n", 10),
        ("pod --flag='app'{\n}\n", 17),
    ];
    for (input, expected_column) in bad_headers {
        assert_eq!(
            parse_psl(input),
            Err(PslError::with_column(
                PslErrorKind::InvalidBlockHeader,
                1,
                expected_column,
            )),
            "unexpected result for {input:?}"
        );
    }

    assert_eq!(
        parse_psl("general\n"),
        Err(PslError::with_column(
            PslErrorKind::MissingDirectiveArgument,
            1,
            1,
        ))
    );
    assert_eq!(
        parse_psl("general {\n    editor\n}\n"),
        Err(PslError::with_column(
            PslErrorKind::MissingDirectiveArgument,
            2,
            5,
        ))
    );
    assert_eq!(
        parse_psl("editor vim\n"),
        Err(PslError::with_column(
            PslErrorKind::UnexpectedRootDirective,
            1,
            1,
        ))
    );
}

#[test]
fn reject_invalid_escape_sequences_and_utf8_escapes() {
    for invalid in [
        "\"\\x\"",
        "\"\\a\"",
        "\"\\0\"",
        "\"\\'\"",
        "\"\\ä\"",
        "\"\\\u{10348}\"",
    ] {
        let input = format!("general {{\n    key {invalid}\n}}\n");
        let error = parse_psl(&input)
            .unwrap_err()
            .with_diagnostic(&input, "./pods.conf");
        assert_eq!(
            error.kind,
            PslErrorKind::InvalidEscapeSequence,
            "invalid escape should fail: {invalid}"
        );
        assert_eq!(error.line, 2);
        assert_eq!(error.column, Some(10));
        let diagnostic = error.diagnostic.as_ref().unwrap();
        assert_eq!(diagnostic.col_number, 10);
        assert_eq!(diagnostic.annotation_carets, "         ^^");
    }
}

#[test]
fn reject_invalid_identifiers_with_accurate_columns() {
    let cases = [
        ("\"general\" {}\n", 1, 1),
        ("'general' {}\n", 1, 1),
        ("123pod {}\n", 1, 1),
        ("general {\n    -opt val\n}\n", 2, 5),
        ("general {\n    bad.name val\n}\n", 2, 5),
    ];
    for (input, expected_line, expected_column) in cases {
        assert_eq!(
            parse_psl(input),
            Err(PslError::with_column(
                PslErrorKind::InvalidIdentifier,
                expected_line,
                expected_column,
            )),
            "unexpected result for {input:?}"
        );
    }
}

#[test]
fn reject_unclosed_blocks_and_unexpected_closing_braces() {
    let single = "general {\n    editor vim\n";
    let single_error = parse_psl(single)
        .unwrap_err()
        .with_diagnostic(single, "./pods.conf");
    assert_eq!(single_error.kind, PslErrorKind::UnclosedBlock);
    assert_eq!(single_error.line, 1);
    assert_eq!(single_error.column, Some(9));
    let single_diagnostic = single_error.diagnostic.as_ref().unwrap();
    assert_eq!(single_diagnostic.line_number, 1);
    assert_eq!(single_diagnostic.col_number, 9);
    assert_eq!(single_diagnostic.line_snippet, "general {");
    assert_eq!(single_diagnostic.annotation_carets, "        ^");

    let nested = "pod app {\n    command migrate {\n        user db\n";
    let nested_error = parse_psl(nested)
        .unwrap_err()
        .with_diagnostic(nested, "./recipe.pod");
    assert_eq!(nested_error.kind, PslErrorKind::UnclosedBlock);
    assert_eq!(nested_error.line, 2);
    assert_eq!(nested_error.column, Some(21));
    let nested_diagnostic = nested_error.diagnostic.as_ref().unwrap();
    assert_eq!(nested_diagnostic.line_number, 2);
    assert_eq!(nested_diagnostic.col_number, 21);
    assert_eq!(nested_diagnostic.line_snippet, "    command migrate {");

    let directive_at_eof = "general {\n    editor vim";
    let eof_error = parse_psl(directive_at_eof)
        .unwrap_err()
        .with_diagnostic(directive_at_eof, "./pods.conf");
    assert_eq!(eof_error.kind, PslErrorKind::UnclosedBlock);
    assert_eq!(eof_error.line, 1);
    assert_eq!(eof_error.column, Some(9));
    assert_eq!(
        eof_error.diagnostic.as_ref().unwrap().line_snippet,
        "general {"
    );

    let header_only_eof = "general {";
    let header_error = parse_psl(header_only_eof)
        .unwrap_err()
        .with_diagnostic(header_only_eof, "./pods.conf");
    assert_eq!(header_error.kind, PslErrorKind::UnclosedBlock);
    assert_eq!(header_error.line, 1);
    assert_eq!(header_error.column, Some(9));

    assert_eq!(
        parse_psl("}\n"),
        Err(PslError::with_column(
            PslErrorKind::UnexpectedClosingBrace,
            1,
            1,
        ))
    );
    assert_eq!(
        parse_psl("general {}\n  }\n"),
        Err(PslError::with_column(
            PslErrorKind::UnexpectedClosingBrace,
            2,
            3,
        ))
    );
}

#[test]
fn reject_unexpected_tokens_and_delimiter_injections() {
    let cases = [
        ("general { editor vim\n}\n", 1, 11),
        ("general {\n    editor vim\n} trailing\n", 3, 3),
        ("general {} trailing\n", 1, 12),
        ("general {\n    key foo\"bar\"\n}\n", 2, 12),
        ("general {\n    key \"foo\"bar\n}\n", 2, 14),
        ("general {\n    key 'foo'bar\n}\n", 2, 14),
        ("general {\n    key ='bar'\n}\n", 2, 10),
        ("general {\n    key =\"bar\"\n}\n", 2, 10),
        (
            "general {\n    option '--health-cmd='cache-cli ping | grep -q PONG || exit 1''\n}\n",
            2,
            27,
        ),
        (
            "general {\n    option \"--health-cmd=\"cache-cli ping | grep -q PONG || exit 1\"\"\n}\n",
            2,
            27,
        ),
        (
            "general {\n    option --health-cmd='cache-cli'extra\n}\n",
            2,
            36,
        ),
        (
            "general {\n    option --health-cmd=\"cache-cli\"extra\n}\n",
            2,
            36,
        ),
        ("general {\r    key val\n}\n", 1, 10),
        ("general\r {\n}\n", 1, 8),
        ("general {\n    key val\r}\n", 2, 12),
        ("general {\n    key \"val\"\r}\n", 2, 14),
        ("# comment\r\ngeneral {}\n# bad\rcomment\n", 3, 6),
        ("general\u{00A0}{\n}\n", 1, 8),
        ("\u{00A0}general {\n}\n", 1, 1),
        ("general {\n    key val\x07\n}\n", 2, 12),
        ("general {\n    key val }\n", 2, 13),
        ("general {\n    key}\n}\n", 2, 8),
    ];

    for (input, expected_line, expected_column) in cases {
        assert_eq!(
            parse_psl(input),
            Err(PslError::with_column(
                PslErrorKind::UnexpectedToken,
                expected_line,
                expected_column,
            )),
            "unexpected result for {input:?}"
        );
    }
}

#[test]
fn reject_unterminated_strings_and_standalone_cr_in_quotes() {
    let cases = [
        ("general {\n    key \"unclosed\n}\n", 2, 9),
        ("general {\n    key 'unclosed\n}\n", 2, 9),
        ("general {\n    key \"eof", 2, 9),
        ("general {\n    key 'eof", 2, 9),
        ("general {\n    key \"trailing\\\n\"\n}\n", 2, 9),
        ("general {\n    key \"trailing\\", 2, 9),
        ("general {\n    option --health-cmd='unclosed\n}\n", 2, 25),
        ("general {\n    option --health-cmd=\"unclosed\n}\n", 2, 25),
        ("general {\n    key \"a\rb\"\n}\n", 2, 9),
        ("general {\n    key 'a\rb'\n}\n", 2, 9),
        ("general {\n    key \"a\\\rb\"\n}\n", 2, 9),
        ("general {\n    key k=\"a\rb\"\n}\n", 2, 11),
        ("general {\n    key k='a\rb'\n}\n", 2, 11),
    ];
    for (input, expected_line, expected_column) in cases {
        assert_eq!(
            parse_psl(input),
            Err(PslError::with_column(
                PslErrorKind::UnterminatedString,
                expected_line,
                expected_column,
            )),
            "unexpected result for {input:?}"
        );
    }
}
