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

//! Unit tests for `.pod` directive parsing, publish/registry/image validation, entry bounds, and diagnostics in [`super`].

use std::fmt::Write as _;
use std::str::FromStr;

use crate::error::{PodError, PslErrorKind, RecipeErrorKind};
use super::Recipe;
use super::directives::MAX_ENTRIES_PER_BLOCK;

fn assert_recipe_error(input: &str, expected_kind: RecipeErrorKind) {
    let error = Recipe::from_str(input).unwrap_err();
    match error {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, expected_kind, "input: {input}");
            let diagnostic = recipe_error.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.file_path, "<input>");
        }
        other => panic!("expected PodError::Recipe({expected_kind:?}), got {other:?}"),
    }
}

#[test]
fn recipe_parser_enforces_entry_cap_and_strict_publish_registry_image_and_volume_spans() {
    let valid_publish = Recipe::from_str(
        "pod app {\n    name App\n    registry docker.io\n    publish 8080 80 tcp\n    publish 8081 81 UDP\n    publish 8082 82\n    publish 8083\n    publish \"[::1]:8084\" 84 tcp\n    publish \"[::1]:\" 85\n    publish 8086/udp\n}\ncontainer {\n    image alpine:latest\n}\n",
    )
    .unwrap();
    assert_eq!(
        valid_publish.pod.publish,
        vec![
            "8080:80/TCP",
            "8081:81/UDP",
            "8082:82",
            "8083",
            "[::1]:8084:84/TCP",
            "[::1]::85",
            "8086/UDP",
        ]
    );

    for bad_publish in [
        "publish 1 2 3 4",
        "publish \"\"",
        "publish 0",
        "publish 65536",
        "publish -8080:80",
        "publish -8080 80",
        "publish 8080 \"\"",
        "publish 8080 -80",
        "publish \"\" 80 TCP",
        "publish 8080 \"\" TCP",
        "publish 8080 80 \"\"",
        "publish 8080 80 BOGUS",
        "publish foo bar",
        "publish 8080:80/BOGUS",
        "publish 999.999.999.999:80 80",
        "publish 8080:80:70",
    ] {
        let expected_kind = if bad_publish == "publish 1 2 3 4" {
            RecipeErrorKind::InvalidArgumentCount
        } else {
            RecipeErrorKind::InvalidPublish
        };
        assert_recipe_error(
            &format!("pod app {{\n    {bad_publish}\n}}\n"),
            expected_kind,
        );
    }

    for bad_registry in ["-flag", "docker.io/", "https://docker.io", "/docker.io"] {
        assert_recipe_error(
            &format!("pod app {{\n    registry {bad_registry}\n}}\n"),
            RecipeErrorKind::InvalidRegistry,
        );
        assert_recipe_error(
            &format!("container {{\n    registry {bad_registry}\n    image alpine\n}}\n"),
            RecipeErrorKind::InvalidRegistry,
        );
    }
    for bad_image in ["-bad", "/alpine:latest", "alpine:latest/", "repo//alpine"] {
        assert_recipe_error(
            &format!("container {{\n    image {bad_image}\n}}\n"),
            RecipeErrorKind::InvalidImage,
        );
    }

    let dup_vol_err = Recipe::from_str("container {\n    volume data data\n}\n").unwrap_err();
    match dup_vol_err {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidVolume);
            let diagnostic = recipe_error.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.col_number, 17);
            assert_eq!(diagnostic.annotation_carets, "                ^^^^");
        }
        other => panic!("expected InvalidVolume diagnostic at col 17, got {other:?}"),
    }

    let mut over_cap = String::from("pod app {\n    name App\n    registry docker.io\n");
    let lines_needed = (MAX_ENTRIES_PER_BLOCK / 50) + 1;
    for _ in 0..lines_needed {
        over_cap.push_str("    option");
        for i in 0..50 {
            let _ = write!(over_cap, " --env=K{i}=1");
        }
        over_cap.push('\n');
    }
    over_cap.push_str("}\n");
    assert_recipe_error(&over_cap, RecipeErrorKind::InvalidOption);
}

#[test]
fn recipe_parser_rejects_structural_and_security_violations() {
    let cases = [
        ("unknown_block {\n}\n", RecipeErrorKind::UnexpectedBlock),
        (
            "pod {\n    name Test\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        ),
        (
            "pod a b {\n    name Test\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        ),
        (
            "pod -bad {\n    name Test\n}\n",
            RecipeErrorKind::InvalidPodId,
        ),
        (
            "pod \"bad\\nname\" {\n    name Test\n}\n",
            RecipeErrorKind::InvalidPodId,
        ),
        ("pod app {\n}\n", RecipeErrorKind::EmptyPodBlock),
        (
            "pod a {\n    name A\n}\npod b {\n    name B\n}\n",
            RecipeErrorKind::DuplicateBlock,
        ),
        (
            "pod app {\n    unknown value\n}\n",
            RecipeErrorKind::UnknownDirective,
        ),
        (
            "pod app {\n    name A\n    name B\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "pod app {\n    description A\n    description B\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "pod app {\n    path /pods/a\n    path /pods/b\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "pod app {\n    registry a.io\n    registry b.io\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "pod app {\n    description \"\"\n}\n",
            RecipeErrorKind::InvalidDescription,
        ),
        (
            "pod app {\n    description \"bad\\ndesc\"\n}\n",
            RecipeErrorKind::InvalidDescription,
        ),
        (
            "pod app {\n    name \"   \"\n}\n",
            RecipeErrorKind::InvalidPodName,
        ),
        (
            "pod app {\n    name \"\\nApp\\n\"\n}\n",
            RecipeErrorKind::InvalidPodName,
        ),
        (
            "pod app {\n    name \"\\tApp\\t\"\n}\n",
            RecipeErrorKind::InvalidPodName,
        ),
        (
            "pod app {\n    name \"\u{0085}App\u{0085}\"\n}\n",
            RecipeErrorKind::InvalidPodName,
        ),
        (
            "pod app {\n    path ../escape\n}\n",
            RecipeErrorKind::InvalidPath,
        ),
        (
            "pod app {\n    option \"\"\n}\n",
            RecipeErrorKind::InvalidOption,
        ),
        (
            "pod app {\n    option \"   \"\n}\n",
            RecipeErrorKind::InvalidOption,
        ),
        (
            "pod app {\n    sub {\n    }\n}\n",
            RecipeErrorKind::UnexpectedBlock,
        ),
        (
            "pod app {\n    command {\n        execute ls\n    }\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        ),
        (
            "pod app {\n    command -bad {\n        execute ls\n    }\n}\n",
            RecipeErrorKind::InvalidCommand,
        ),
        (
            "pod app {\n    command run {\n    }\n}\n",
            RecipeErrorKind::InvalidCommand,
        ),
        (
            "pod app {\n    command run {\n        execute a\n    }\n    command run {\n        execute b\n    }\n}\n",
            RecipeErrorKind::DuplicateBlock,
        ),
        (
            "pod app {\n    command run {\n        container a\n        container b\n    }\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "pod app {\n    command run {\n        description a\n        description b\n    }\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "pod app {\n    command run {\n        execute a\n        execute b\n    }\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "pod app {\n    command run {\n        user a\n        user b\n    }\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "pod app {\n    command run {\n        nested {\n        }\n    }\n}\n",
            RecipeErrorKind::UnexpectedBlock,
        ),
        (
            "pod app {\n    command run {\n        unknown val\n    }\n}\n",
            RecipeErrorKind::UnknownDirective,
        ),
        ("container {\n}\n", RecipeErrorKind::EmptyContainerBlock),
        (
            "container \"\" {\n    image alpine\n}\n",
            RecipeErrorKind::InvalidContainerName,
        ),
        (
            "container a b {\n    image alpine\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        ),
        (
            "container *-bad {\n    image alpine\n}\n",
            RecipeErrorKind::InvalidContainerName,
        ),
        (
            "container {\n    image alpine\n    image redis\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "container {\n    registry a.io\n    registry b.io\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "container {\n    restart always\n    restart never\n}\n",
            RecipeErrorKind::DuplicateDirective,
        ),
        (
            "container {\n    unknown val\n}\n",
            RecipeErrorKind::UnknownDirective,
        ),
        (
            "container {\n    restart -always\n}\n",
            RecipeErrorKind::InvalidRestartPolicy,
        ),
        (
            "container {\n    command \"\"\n}\n",
            RecipeErrorKind::InvalidCommand,
        ),
        (
            "container {\n    command \"   \"\n}\n",
            RecipeErrorKind::InvalidCommand,
        ),
        (
            "container {\n    volume /only_one\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        ),
        (
            "container {\n    volume /a /b ro extra\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        ),
        (
            "container {\n    volume ../escape /data\n}\n",
            RecipeErrorKind::InvalidVolume,
        ),
        (
            "container {\n    volume data /var/lib/db/data:Z\n}\n",
            RecipeErrorKind::InvalidVolume,
        ),
        (
            "container {\n    volume ./data /data:\n}\n",
            RecipeErrorKind::InvalidVolume,
        ),
        (
            "container {\n    volume data /data -bad\n}\n",
            RecipeErrorKind::InvalidVolume,
        ),
        (
            "container {\n    nested {\n    }\n}\n",
            RecipeErrorKind::UnexpectedBlock,
        ),
    ];
    for (input, expected_kind) in cases {
        assert_recipe_error(input, expected_kind);
    }

    let empty_container_dir_error =
        Recipe::from_str("container {\n    volume data \"\"\n}\n").unwrap_err();
    match empty_container_dir_error {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidVolume);
            assert_eq!(recipe_error.token.as_deref(), Some("volume"));
        }
        other => panic!("expected InvalidVolume, got {other:?}"),
    }

    let volume_diag_error = Recipe::from_str(
        "container {\n    volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'\n}\n",
    )
    .unwrap_err();
    match volume_diag_error {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidVolume);
            assert_eq!(recipe_error.token.as_deref(), Some("/var/lib/db/data:Z"));
            let diagnostic = recipe_error.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.line_number, 2);
            assert_eq!(diagnostic.col_number, 19);
            assert_eq!(
                diagnostic.annotation_carets,
                "                  ^^^^^^^^^^^^^^^^^^"
            );
        }
        other => panic!("expected InvalidVolume, got {other:?}"),
    }

    let multi_cmd_err =
        Recipe::from_str("container {\n    command \"a b\" \"a\\nb\"\n}\n").unwrap_err();
    match multi_cmd_err {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidCommand);
            let diagnostic = recipe_error.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.col_number, 19);
        }
        other => panic!("expected InvalidCommand at arg 2, got {other:?}"),
    }

    let multi_opt_err =
        Recipe::from_str("container {\n    option \"foo bar\" \"foo\\nbar\"\n}\n").unwrap_err();
    match multi_opt_err {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidOption);
            let diagnostic = recipe_error.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.col_number, 22);
        }
        other => panic!("expected InvalidOption at arg 2, got {other:?}"),
    }

    let delimiter_error = Recipe::from_str("pod \"app\"{ name Test }\n").unwrap_err();
    match delimiter_error {
        PodError::Psl(psl_error) => {
            assert_eq!(psl_error.kind, PslErrorKind::InvalidBlockHeader);
            let diagnostic = psl_error.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.file_path, "<input>");
            assert_eq!(diagnostic.col_number, 10);
            assert_eq!(diagnostic.annotation_carets, "         ^");
        }
        other => panic!("expected InvalidBlockHeader, got {other:?}"),
    }
}
