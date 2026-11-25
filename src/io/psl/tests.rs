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

//! Canonical PSL parsing and positive edge-case unit tests.

use std::borrow::Cow;

use crate::error::{PslError, PslErrorKind};
use super::{PslBlock, PslDirective, PslDocument, PslStatement, parse_psl};

const CANONICAL_EXAMPLE_POD: &str = "\
# Example Web Stack Recipe

pod example-stack {
    name        \"Example Web Stack\"
    description \"A minimal but complete example recipe with a web app and database.\"
    path        /pods/example-stack
    registry    docker.io
    publish     8080 80 TCP
    option      --userns=keep-id

    command migrate {
        description \"Run database migrations.\"
        container   *db
        user        dbuser
        execute     \"db-cli -U dbuser -d app -f /migrations/run.sql\"
    }
}

container *db {
    restart  always
    registry docker.io
    image    example/db-server:latest
    volume   data /var/lib/db/data Z # Mounts 'data' with SELinux label 'Z'
    option   \"--env DB_PASSWORD=secret\"
}

container *app {
    restart always
    image   example/example-app:latest
    command start-server.sh
}
";

const CANONICAL_PODS_CONF: &str = "\
# Podoxide Configuration (PSL)

general {
    editor   vim
    simulate false
}

pods {
    path /pods
}

recipes {
    path ./recipes

    groups {
        all      @stack
        stack    @database @web
        database db-stack
        web      app-stack
    }
}
";

fn child_blocks<'a>(block: &'a PslBlock<'a>) -> Vec<&'a PslBlock<'a>> {
    block
        .statements
        .iter()
        .filter_map(|statement| match statement {
            PslStatement::Block(child) => Some(child),
            PslStatement::Directive(_) => None,
        })
        .collect()
}

fn child_directives<'a>(block: &'a PslBlock<'a>) -> Vec<&'a PslDirective<'a>> {
    block
        .statements
        .iter()
        .filter_map(|statement| match statement {
            PslStatement::Block(_) => None,
            PslStatement::Directive(directive) => Some(directive),
        })
        .collect()
}

#[test]
fn parse_assigned_quote_edge_cases() {
    let input = "\
pod --label=\"app v1\" {
    utf8_prefix ä=\"val\"
    multi_eq a=b=\"val\"
    empty_double --flag=\"\"
    empty_single --flag=''
    with_comment --flag=\"val\"#comment
    with_single_comment --flag='val'#comment
    escaped_utf8 \"\\\"ä\"
}
";
    let document = parse_psl(input).unwrap_or_else(|error| panic!("{error}"));
    let pod_block = &document.blocks[0];
    assert_eq!(pod_block.arguments.len(), 1);
    assert!(matches!(pod_block.arguments[0], Cow::Owned(_)));
    assert_eq!(pod_block.arguments[0], "--label=app v1");

    let directives = child_directives(pod_block);
    assert_eq!(directives.len(), 7);
    assert_eq!(directives[0].arguments[0], "ä=val");
    assert_eq!(directives[1].arguments[0], "a=b=val");
    assert_eq!(directives[2].arguments[0], "--flag=");
    assert_eq!(directives[3].arguments[0], "--flag=");
    assert_eq!(directives[4].arguments[0], "--flag=val");
    assert_eq!(directives[5].arguments[0], "--flag=val");
    assert!(matches!(directives[6].arguments[0], Cow::Owned(_)));
    assert_eq!(directives[6].arguments[0], "\"ä");

    let bad_escape_input = "general {\n    option --flag=\"\\x\"\n}\n";
    let bad_escape = parse_psl(bad_escape_input)
        .unwrap_err()
        .with_diagnostic(bad_escape_input, "./pods.conf");
    assert_eq!(bad_escape.kind, PslErrorKind::InvalidEscapeSequence);
    assert_eq!(bad_escape.line, 2);
    assert_eq!(bad_escape.column, Some(20));
    assert_eq!(bad_escape.diagnostic.as_ref().unwrap().col_number, 20);
    assert_eq!(
        bad_escape.diagnostic.as_ref().unwrap().annotation_carets,
        "                   ^^"
    );

    assert_eq!(
        parse_psl("general {\n    option --flag=\"val\"}\n}\n"),
        Err(PslError::with_column(PslErrorKind::UnexpectedToken, 2, 24))
    );
    assert_eq!(
        parse_psl("general {\n    option --flag=\"val\" }\n}\n"),
        Err(PslError::with_column(PslErrorKind::UnexpectedToken, 2, 25))
    );
}

#[test]
fn parse_canonical_example_pod() {
    let document = parse_psl(CANONICAL_EXAMPLE_POD).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(document.blocks.len(), 3);

    let pod_block = &document.blocks[0];
    assert_eq!(pod_block.name, "pod");
    assert_eq!(pod_block.arguments, vec![Cow::Borrowed("example-stack")]);
    assert_eq!(pod_block.line, 3);
    assert_eq!(pod_block.statements.len(), 7);

    let pod_directives = child_directives(pod_block);
    assert_eq!(pod_directives.len(), 6);
    assert!(matches!(pod_directives[0].arguments[0], Cow::Borrowed(_)));
    assert_eq!(pod_directives[0].arguments[0], "Example Web Stack");
    assert_eq!(
        pod_directives[4].arguments,
        vec![
            Cow::Borrowed("8080"),
            Cow::Borrowed("80"),
            Cow::Borrowed("TCP"),
        ]
    );

    let nested_blocks = child_blocks(pod_block);
    assert_eq!(nested_blocks.len(), 1);
    let migrate = nested_blocks[0];
    assert_eq!(migrate.name, "command");
    assert_eq!(migrate.arguments, vec![Cow::Borrowed("migrate")]);
    assert_eq!(migrate.line, 11);
    assert_eq!(child_directives(migrate).len(), 4);

    let db_container = &document.blocks[1];
    assert_eq!(db_container.name, "container");
    assert_eq!(db_container.arguments, vec![Cow::Borrowed("*db")]);
    assert_eq!(db_container.line, 19);
    let db_directives = child_directives(db_container);
    assert_eq!(db_directives.len(), 5);
    assert_eq!(db_directives[3].name, "volume");
    assert_eq!(
        db_directives[3].arguments,
        vec![
            Cow::Borrowed("data"),
            Cow::Borrowed("/var/lib/db/data"),
            Cow::Borrowed("Z"),
        ]
    );
    assert_eq!(db_directives[3].line, 23);

    let app_container = &document.blocks[2];
    assert_eq!(app_container.name, "container");
    assert_eq!(app_container.arguments, vec![Cow::Borrowed("*app")]);
    assert_eq!(app_container.line, 27);
    assert_eq!(child_directives(app_container).len(), 3);
}

#[test]
fn parse_canonical_pods_conf() {
    let document = parse_psl(CANONICAL_PODS_CONF).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(document.blocks.len(), 3);

    let general = &document.blocks[0];
    assert_eq!(general.name, "general");
    assert!(general.arguments.is_empty());
    assert_eq!(general.arguments.capacity(), 0);
    assert_eq!(general.line, 3);
    let general_directives = child_directives(general);
    assert_eq!(general_directives.len(), 2);
    assert_eq!(general_directives[0].name, "editor");
    assert_eq!(general_directives[0].arguments, vec![Cow::Borrowed("vim")]);
    assert_eq!(general_directives[0].line, 4);
    assert_eq!(general_directives[1].name, "simulate");
    assert_eq!(
        general_directives[1].arguments,
        vec![Cow::Borrowed("false")]
    );
    assert_eq!(general_directives[1].line, 5);

    let pods = &document.blocks[1];
    assert_eq!(pods.name, "pods");
    assert_eq!(pods.arguments.capacity(), 0);
    assert_eq!(pods.line, 8);
    let pods_directives = child_directives(pods);
    assert_eq!(pods_directives.len(), 1);
    assert_eq!(pods_directives[0].name, "path");
    assert_eq!(pods_directives[0].arguments, vec![Cow::Borrowed("/pods")]);
    assert_eq!(pods_directives[0].line, 9);

    let recipes = &document.blocks[2];
    assert_eq!(recipes.name, "recipes");
    assert_eq!(recipes.arguments.capacity(), 0);
    assert_eq!(recipes.line, 12);
    assert_eq!(recipes.statements.len(), 2);
    let recipes_directives = child_directives(recipes);
    assert_eq!(recipes_directives.len(), 1);
    assert_eq!(recipes_directives[0].name, "path");
    assert_eq!(
        recipes_directives[0].arguments,
        vec![Cow::Borrowed("./recipes")]
    );
    assert_eq!(recipes_directives[0].line, 13);

    let recipes_blocks = child_blocks(recipes);
    assert_eq!(recipes_blocks.len(), 1);
    let groups = recipes_blocks[0];
    assert_eq!(groups.name, "groups");
    assert_eq!(groups.arguments.capacity(), 0);
    assert_eq!(groups.line, 15);
    let group_directives = child_directives(groups);
    assert_eq!(group_directives.len(), 4);
    assert_eq!(group_directives[1].name, "stack");
    assert_eq!(
        group_directives[1].arguments,
        vec![Cow::Borrowed("@database"), Cow::Borrowed("@web")]
    );
    assert_eq!(group_directives[1].line, 17);
}

#[test]
fn parse_empty_comments_crlf_and_inline_blocks() {
    assert_eq!(parse_psl(""), Ok(PslDocument::new()));
    assert_eq!(
        parse_psl("   \t  \n# comment only\r\n  # another comment"),
        Ok(PslDocument::new())
    );

    let input = "block_1 {}\r\nblock-2 arg1 { #inline-comment\r\n\tkey val#comment\r\n}\r\nempty_spaced {   } # trailing";
    let document = parse_psl(input).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(document.blocks.len(), 3);
    assert_eq!(document.blocks[0].name, "block_1");
    assert_eq!(document.blocks[0].arguments.capacity(), 0);
    assert!(document.blocks[0].statements.is_empty());
    assert_eq!(document.blocks[0].line, 1);

    assert_eq!(document.blocks[1].name, "block-2");
    assert_eq!(document.blocks[1].arguments, vec![Cow::Borrowed("arg1")]);
    assert_eq!(document.blocks[1].line, 2);
    assert_eq!(
        document.blocks[1].statements,
        vec![PslStatement::Directive(PslDirective::new(
            "key",
            vec![Cow::Borrowed("val")],
            3,
        ))]
    );

    assert_eq!(document.blocks[2].name, "empty_spaced");
    assert_eq!(document.blocks[2].arguments.capacity(), 0);
    assert_eq!(document.blocks[2].line, 5);
}

#[test]
fn parse_quoted_strings_cow_borrowed_vs_owned() {
    let input = "\
block {
    plain_double \"no escapes here\"
    escaped_double \"line1\\nline2\\t\\\"quoted\\\"\\\\\\r\"
    single_raw 'raw \\x \\n \\0 \"literal\"'
    assigned_single --health-cmd='cache-cli ping | grep -q PONG || exit 1'
    assigned_double --health-cmd=\"cache-cli ping | grep -q PONG || exit 1\"
    assigned_escaped --health-cmd=\"echo \\\"PONG\\\"\"
    wrapped_double \"--health-cmd='cache-cli ping | grep -q PONG || exit 1'\"
    separated_single --health-cmd 'cache-cli ping | grep -q PONG || exit 1'
    separated_double --health-cmd \"cache-cli ping | grep -q PONG || exit 1\"
}";
    let document = parse_psl(input).unwrap_or_else(|error| panic!("{error}"));
    let directives = child_directives(&document.blocks[0]);
    assert_eq!(directives.len(), 9);

    assert!(matches!(directives[0].arguments[0], Cow::Borrowed(_)));
    assert_eq!(directives[0].arguments[0], "no escapes here");

    assert!(matches!(directives[1].arguments[0], Cow::Owned(_)));
    assert_eq!(directives[1].arguments[0], "line1\nline2\t\"quoted\"\\\r");

    assert!(matches!(directives[2].arguments[0], Cow::Borrowed(_)));
    assert_eq!(directives[2].arguments[0], "raw \\x \\n \\0 \"literal\"");

    assert!(matches!(directives[3].arguments[0], Cow::Owned(_)));
    assert_eq!(
        directives[3].arguments[0],
        "--health-cmd=cache-cli ping | grep -q PONG || exit 1"
    );

    assert!(matches!(directives[4].arguments[0], Cow::Owned(_)));
    assert_eq!(
        directives[4].arguments[0],
        "--health-cmd=cache-cli ping | grep -q PONG || exit 1"
    );

    assert!(matches!(directives[5].arguments[0], Cow::Owned(_)));
    assert_eq!(directives[5].arguments[0], "--health-cmd=echo \"PONG\"");

    assert!(matches!(directives[6].arguments[0], Cow::Borrowed(_)));
    assert_eq!(
        directives[6].arguments[0],
        "--health-cmd='cache-cli ping | grep -q PONG || exit 1'"
    );

    assert_eq!(
        directives[7].arguments,
        vec![
            Cow::Borrowed("--health-cmd"),
            Cow::Borrowed("cache-cli ping | grep -q PONG || exit 1"),
        ]
    );
    assert_eq!(
        directives[8].arguments,
        vec![
            Cow::Borrowed("--health-cmd"),
            Cow::Borrowed("cache-cli ping | grep -q PONG || exit 1"),
        ]
    );
}

#[test]
fn parse_utf8_and_root_templates() {
    let utf8_input = "\
# UTF-8 comment: äöü ß
pod_utf8 arg_äöü {
    label \"Grüße \\\"Welt\\\" αβγ\"
    raw_label 'Grüße Welt αβγ'
}
";
    let document = parse_psl(utf8_input).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(document.blocks.len(), 1);
    assert_eq!(document.blocks[0].arguments, vec![Cow::Borrowed("arg_äöü")]);
    let directives = child_directives(&document.blocks[0]);
    assert_eq!(directives[0].arguments[0], "Grüße \"Welt\" αβγ");
    assert_eq!(directives[1].arguments[0], "Grüße Welt αβγ");

    let root_config = include_str!("../../../pods.conf");
    let config_document = parse_psl(root_config).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(config_document.blocks.len(), 3);

    let root_recipe = include_str!("../../../recipe.pod");
    let recipe_document = parse_psl(root_recipe).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(recipe_document.blocks.len(), 3);
}
