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

//! Unit tests for `pods.conf` PSL parsing, semantic validation, and red-team exploits in [`super`].

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::error::{ConfigErrorKind, MAX_ERROR_TOKEN_BYTES, PodError, PslErrorKind};
use super::{Config, DEFAULT_RECIPES_PATH, MAX_GROUP_EXPANSION_DEPTH, PodsConfig, RecipesConfig};

fn assert_config_error(input: &str, expected_kind: ConfigErrorKind) {
    let error = Config::from_str(input).unwrap_err();
    match error {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, expected_kind);
            let diagnostic = config_error.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.file_path, "<input>");
        }
        other => panic!("expected PodError::Config({expected_kind:?}), got {other:?}"),
    }
}

#[test]
fn parse_minimal_config_with_defaults() {
    let minimal = "\
recipes {
    groups {
        default example-app
    }
}
";
    let config = Config::from_str(minimal).unwrap();
    assert_eq!(config.editor, "");
    assert!(config.simulate);
    assert_eq!(config.pods, PodsConfig::new());
    assert_eq!(config.recipes.path, PathBuf::from(DEFAULT_RECIPES_PATH));
    assert_eq!(
        config.recipes.groups.get("default"),
        Some(&vec![String::from("example-app")])
    );
    assert_eq!(
        RecipesConfig::new().path,
        PathBuf::from(DEFAULT_RECIPES_PATH)
    );
}

#[test]
fn red_team_cycles_depth_path_traversal_editor_and_target_exploits() {
    assert_config_error(
        "recipes {\n    groups {\n        all @all\n    }\n}",
        ConfigErrorKind::InvalidGroupTarget,
    );
    assert_config_error(
        "recipes {\n    groups {\n        a @b\n        b @a\n    }\n}",
        ConfigErrorKind::InvalidGroupTarget,
    );
    assert_config_error(
        "recipes {\n    groups {\n        a app @b\n        b @c\n        c @a\n    }\n}",
        ConfigErrorKind::InvalidGroupTarget,
    );

    // Deep group reference chain exceeding MAX_GROUP_EXPANSION_DEPTH fails during parse.
    let mut deep_psl = String::from("recipes {\n    groups {\n");
    for index in 0..=MAX_GROUP_EXPANSION_DEPTH {
        deep_psl.push_str(&format!("        g{index:03} @g{:03}\n", index + 1));
    }
    deep_psl.push_str(&format!(
        "        g{:03} leaf\n    }}\n}}\n",
        MAX_GROUP_EXPANSION_DEPTH + 1
    ));
    assert_config_error(&deep_psl, ConfigErrorKind::InvalidGroupTarget);

    let valid_dag = "\
recipes {
    groups {
        db postgres
        cache redis
        backend @db @cache
        all @backend @db @missing_group frontend
    }
}
";
    assert!(Config::from_str(valid_dag).is_ok());

    for malicious_target in [
        "*",
        "*db",
        "@*",
        "app*",
        "app?",
        ".hidden",
        "..hidden",
        "app.pod",
        "a..b",
        "@.hidden",
        "@123",
        "@-bad",
        "\"app\\\\evil\"",
    ] {
        let input =
            format!("recipes {{\n    groups {{\n        all {malicious_target}\n    }}\n}}");
        assert_config_error(&input, ConfigErrorKind::InvalidGroupTarget);
    }

    for malicious_path in [
        "..",
        "../pods",
        "pods/..",
        "/var/pods/../../etc",
        "/var/pods:ro,Z",
        "pods:rw",
        "-rf",
        "--privileged",
        "\"   \"",
        "\" /var/pods\"",
        "\"/var/pods \"",
        "/",
        "///",
        "/var/pods/\u{202E}evil",
    ] {
        let pods_input = format!(
            "pods {{\n    path {malicious_path}\n}}\nrecipes {{\n    groups {{\n        all app\n    }}\n}}"
        );
        assert_config_error(&pods_input, ConfigErrorKind::InvalidPath);

        let recipes_input = format!(
            "recipes {{\n    path {malicious_path}\n    groups {{\n        all app\n    }}\n}}"
        );
        assert_config_error(&recipes_input, ConfigErrorKind::InvalidPath);
    }

    for malicious_editor in [
        "\"\"",
        "\"   \"",
        "\"vim --flag\"",
        "\"vim\\t-u\"",
        "\"vim\\nevil\"",
        "\"vim\\r\"",
        "-rf",
        "--help",
        "/usr/bin/-vim",
        ".",
        "..",
        "vim/.",
        "/usr/bin/vim/.",
        "/usr/bin/",
        "../../bin/vim",
        "./bin/editor_1",
        "./evil",
        "sub/vim",
        "/usr/bin/../../etc/passwd",
        "/usr/bin/\u{202E}miv",
    ] {
        let editor_input = format!(
            "general {{\n    editor {malicious_editor}\n}}\nrecipes {{\n    groups {{\n        all app\n    }}\n}}"
        );
        assert_config_error(&editor_input, ConfigErrorKind::InvalidPath);
    }

    let podman_editor_error = Config::from_str(
        "general {\n    editor podman\n}\nrecipes {\n    groups {\n        all app\n    }\n}\n",
    )
    .unwrap_err();
    match podman_editor_error {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
            let diagnostic = config_error.diagnostic.as_ref().unwrap();
            assert!(diagnostic.annotation_text.contains("podman"));
            assert!(diagnostic.help_text.contains("podman"));
        }
        other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
    }

    for valid_editor in ["vim", "nvim", "/usr/bin/vim", "/usr/local/bin/editor_1"] {
        let editor_input = format!(
            "general {{\n    editor {valid_editor}\n}}\nrecipes {{\n    groups {{\n        all app\n    }}\n}}"
        );
        assert!(Config::from_str(&editor_input).is_ok());
    }

    for malicious_load_path in [
        "",
        ".",
        "..",
        "../staging.conf",
        "/tmp/../staging.conf",
        "-staging.conf",
        "/tmp/-staging.conf",
        ".conf",
        "/tmp/.conf",
        "..conf",
        "/tmp/pods.txt",
        "/tmp/staging.conf/",
        " /tmp/staging.conf",
        "/tmp/staging.conf\n",
        "/tmp/\u{202E}staging.conf",
    ] {
        let error = Config::load(Path::new(malicious_load_path)).unwrap_err();
        assert!(matches!(
            error,
            PodError::Config(config_error) if config_error.kind == ConfigErrorKind::InvalidPath
        ));
    }

    let injected_load_error =
        Config::load(Path::new("/tmp/../bad\n\x1b[31m\u{202E}.conf")).unwrap_err();
    match injected_load_error {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
            let token = config_error.token.as_deref().unwrap();
            assert!(!token.contains('\n'));
            assert!(!token.contains('\x1b'));
            assert!(!token.contains('\u{202E}'));
        }
        other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
    }

    let oversized_path = format!("/tmp/../{}.conf", "a".repeat(MAX_ERROR_TOKEN_BYTES + 128));
    let oversized_load_error = Config::load(Path::new(&oversized_path)).unwrap_err();
    match oversized_load_error {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
            assert!(config_error.token.as_deref().unwrap().len() <= MAX_ERROR_TOKEN_BYTES);
        }
        other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
    }

    let invalid_utf8_os_string = OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff, 0xfe]);
    assert!(matches!(
        Config::load(Path::new(&invalid_utf8_os_string)),
        Err(PodError::Config(error)) if error.kind == ConfigErrorKind::InvalidPath
    ));
}

#[test]
fn reject_all_semantic_and_syntactic_config_errors() {
    assert_config_error("", ConfigErrorKind::MissingRecipesBlock);
    assert_config_error(
        "general {\n    editor vim\n}",
        ConfigErrorKind::MissingRecipesBlock,
    );

    assert_config_error(
        "recipes {\n    path .\n}",
        ConfigErrorKind::MissingGroupsBlock,
    );
    assert_config_error(
        "recipes {\n    groups {}\n}",
        ConfigErrorKind::EmptyGroupsBlock,
    );

    assert_config_error(
        "unknown_root {}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::UnexpectedBlock,
    );
    assert_config_error(
        "general {\n    nested {}\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::UnexpectedBlock,
    );
    assert_config_error(
        "pods {\n    nested {}\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::UnexpectedBlock,
    );
    assert_config_error(
        "recipes {\n    other {}\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::UnexpectedBlock,
    );
    assert_config_error(
        "recipes {\n    groups {\n        nested {}\n    }\n}",
        ConfigErrorKind::UnexpectedBlock,
    );

    assert_config_error(
        "general {}\ngeneral {}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::DuplicateBlock,
    );
    assert_config_error(
        "pods {}\npods {}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::DuplicateBlock,
    );
    assert_config_error(
        "recipes {\n    groups {\n        all app\n    }\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::DuplicateBlock,
    );
    assert_config_error(
        "recipes {\n    groups {\n        all app\n    }\n    groups {\n        web frontend\n    }\n}",
        ConfigErrorKind::DuplicateBlock,
    );

    assert_config_error(
        "general {\n    editor vim\n    editor nano\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::DuplicateDirective,
    );
    assert_config_error(
        "general {\n    simulate true\n    simulate false\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::DuplicateDirective,
    );
    assert_config_error(
        "pods {\n    path /a\n    path /b\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::DuplicateDirective,
    );
    assert_config_error(
        "recipes {\n    path ./a\n    path ./b\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::DuplicateDirective,
    );
    assert_config_error(
        "recipes {\n    groups {\n        all app\n        all web\n    }\n}",
        ConfigErrorKind::DuplicateDirective,
    );

    assert_config_error(
        "general {\n    unknown_key val\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::UnknownDirective,
    );
    assert_config_error(
        "pods {\n    unknown_key val\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::UnknownDirective,
    );
    assert_config_error(
        "recipes {\n    unknown_key val\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::UnknownDirective,
    );

    assert_config_error(
        "general arg {}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::InvalidArgumentCount,
    );
    assert_config_error(
        "recipes {\n    groups arg {\n        all app\n    }\n}",
        ConfigErrorKind::InvalidArgumentCount,
    );
    assert_config_error(
        "general {\n    editor vim --flag\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::InvalidArgumentCount,
    );
    assert_config_error(
        "general {\n    simulate true false\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::InvalidArgumentCount,
    );
    assert_config_error(
        "pods {\n    path /a /b\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::InvalidArgumentCount,
    );

    assert_config_error(
        "general {\n    simulate yes\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::InvalidBoolean,
    );

    assert_config_error(
        "pods {\n    path \"\"\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::InvalidPath,
    );
    assert_config_error(
        "recipes {\n    path \"\"\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::InvalidPath,
    );
    assert_config_error(
        "pods {\n    path \"/bad\\npath\"\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
        ConfigErrorKind::InvalidPath,
    );

    for invalid_target in [
        "\"\"",
        ".",
        "..",
        "-flag",
        "@",
        "@@double",
        "@group/container",
        "recipe/container",
        "@group:1",
        "\"space target\"",
        "\"newline\\ntarget\"",
    ] {
        let input = format!("recipes {{\n    groups {{\n        all {invalid_target}\n    }}\n}}");
        assert_config_error(&input, ConfigErrorKind::InvalidGroupTarget);
    }

    let syntax_error = Config::from_str("editor vim").unwrap_err();
    match syntax_error {
        PodError::Psl(psl_error) => {
            assert_eq!(psl_error.kind, PslErrorKind::UnexpectedRootDirective);
            let diagnostic = psl_error.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.file_path, "<input>");
            assert_eq!(diagnostic.col_number, 1);
            assert_eq!(diagnostic.annotation_carets, "^^^^^^");
        }
        other => panic!("expected PodError::Psl, got {other:?}"),
    }
}
