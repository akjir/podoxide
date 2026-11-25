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

//! Unit tests for configuration target expansion and membership queries in [`super`].

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::str::FromStr;

use crate::core::log;
use crate::error::{ConfigErrorKind, PodError};
use super::{Config, MAX_GROUP_EXPANSION_DEPTH};

#[test]
fn expand_targets_and_has_recipe_preserve_first_appearance_and_reject_invalid_targets() {
    let config_input = "\
recipes {
    groups {
        single target
        stack push pop
        glados the cake lie
        nested @stack @glados
        diamond @nested @stack
    }
}
";
    let config = Config::from_str(config_input).unwrap();

    assert!(config.has_recipe("target"));
    assert!(config.has_recipe("push"));
    assert!(!config.has_recipe("invalid"));
    assert!(!config.has_recipe("@stack"));
    assert!(!config.has_recipe("../evil"));

    assert!(config.expand_targets(&[]).unwrap().is_empty());

    log::set_debug_enabled(true);
    assert_eq!(
        config.expand_targets(&[String::from("target")]).unwrap(),
        vec![String::from("target")]
    );
    assert_eq!(
        config.expand_targets(&[String::from("@stack")]).unwrap(),
        vec![String::from("push"), String::from("pop")]
    );
    assert_eq!(
        config
            .expand_targets(&[String::from("target"), String::from("@stack")])
            .unwrap(),
        vec![
            String::from("target"),
            String::from("push"),
            String::from("pop"),
        ]
    );
    assert_eq!(
        config
            .expand_targets(&[
                String::from("target"),
                String::from("@stack"),
                String::from("target"),
            ])
            .unwrap(),
        vec![
            String::from("target"),
            String::from("push"),
            String::from("pop"),
        ]
    );
    assert_eq!(
        config
            .expand_targets(&[
                String::from("the"),
                String::from("@glados"),
                String::from("lie"),
            ])
            .unwrap(),
        vec![
            String::from("the"),
            String::from("cake"),
            String::from("lie"),
        ]
    );
    assert_eq!(
        config
            .expand_targets(&[
                String::from("lie"),
                String::from("cake"),
                String::from("@glados"),
            ])
            .unwrap(),
        vec![
            String::from("lie"),
            String::from("cake"),
            String::from("the"),
        ]
    );
    assert_eq!(
        config.expand_targets(&[String::from("@diamond")]).unwrap(),
        vec![
            String::from("push"),
            String::from("pop"),
            String::from("the"),
            String::from("cake"),
            String::from("lie"),
        ]
    );
    log::set_debug_enabled(false);

    let unknown_recipe = config
        .expand_targets(&[
            String::from("push"),
            String::from("invalid"),
            String::from("pop"),
        ])
        .unwrap_err();
    match unknown_recipe {
        PodError::Config(error) => {
            assert_eq!(error.kind, ConfigErrorKind::RecipeNotFound);
            assert_eq!(error.token.as_deref(), Some("invalid"));
        }
        other => panic!("expected RecipeNotFound, got {other:?}"),
    }

    for bad_group in ["@invalid", "@", "@-bad"] {
        let unknown_group = config
            .expand_targets(&[String::from(bad_group)])
            .unwrap_err();
        match unknown_group {
            PodError::Config(error) => {
                assert_eq!(error.kind, ConfigErrorKind::UnknownGroup);
                assert_eq!(error.token.as_deref(), Some(bad_group));
            }
            other => panic!("expected UnknownGroup, got {other:?}"),
        }
    }

    let mut manual_config = Config::new();
    manual_config
        .recipes
        .groups
        .insert(String::from("cycle"), vec![String::from("@cycle")]);
    manual_config
        .recipes
        .groups
        .insert(String::from("bad_entry"), vec![String::from("../bad")]);

    let cycle_error = manual_config
        .expand_targets(&[String::from("@cycle")])
        .unwrap_err();
    match cycle_error {
        PodError::Config(error) => assert_eq!(error.kind, ConfigErrorKind::InvalidGroupTarget),
        other => panic!("expected InvalidGroupTarget, got {other:?}"),
    }

    let entry_error = manual_config
        .expand_targets(&[String::from("@bad_entry")])
        .unwrap_err();
    match entry_error {
        PodError::Config(error) => assert_eq!(error.kind, ConfigErrorKind::InvalidGroupTarget),
        other => panic!("expected InvalidGroupTarget, got {other:?}"),
    }
}

#[test]
fn expand_targets_handles_wide_diamond_dag_and_enforces_depth_bound() {
    let mut diamond = Config::new();
    for index in 0..30 {
        let group_name = format!("g{index:02}");
        let next_target = format!("@g{:02}", index + 1);
        diamond
            .recipes
            .groups
            .insert(group_name, vec![next_target.clone(), next_target]);
    }
    diamond
        .recipes
        .groups
        .insert(String::from("g30"), vec![String::from("leaf")]);
    assert_eq!(
        diamond.expand_targets(&[String::from("@g00")]).unwrap(),
        vec![String::from("leaf")]
    );

    let mut exact_depth = Config::new();
    for index in 0..(MAX_GROUP_EXPANSION_DEPTH - 1) {
        exact_depth.recipes.groups.insert(
            format!("d_{index:03}"),
            vec![format!("@d_{:03}", index + 1)],
        );
    }
    exact_depth.recipes.groups.insert(
        format!("d_{:03}", MAX_GROUP_EXPANSION_DEPTH - 1),
        vec![String::from("leaf")],
    );
    assert_eq!(
        exact_depth
            .expand_targets(&[String::from("@d_000")])
            .unwrap(),
        vec![String::from("leaf")]
    );

    let mut deep_chain = Config::new();
    for index in 0..=MAX_GROUP_EXPANSION_DEPTH {
        deep_chain.recipes.groups.insert(
            format!("chain_{index:03}"),
            vec![format!("@chain_{:03}", index + 1)],
        );
    }
    deep_chain.recipes.groups.insert(
        format!("chain_{:03}", MAX_GROUP_EXPANSION_DEPTH + 1),
        vec![String::from("leaf")],
    );
    let depth_error = deep_chain
        .expand_targets(&[String::from("@chain_000")])
        .unwrap_err();
    assert!(matches!(
        depth_error,
        PodError::Config(ref error) if error.kind == ConfigErrorKind::InvalidGroupTarget
    ));
}

#[test]
fn get_recipes_collects_unique_valid_recipes_and_rejects_invalid_entries() {
    let config_input = "\
recipes {
    groups {
        backend db-server cache-server
        frontend web-frontend @backend db-server
    }
}
";
    let config = Config::from_str(config_input).unwrap();
    let expected = BTreeSet::from(["cache-server", "db-server", "web-frontend"]);
    assert_eq!(config.get_recipes(), expected);

    let mut manual_config = Config::new();
    manual_config.recipes.groups.insert(
        String::from("mixed"),
        vec![
            String::from("valid_recipe"),
            String::from("@subgroup"),
            String::from("../escape"),
            String::from(" app "),
            String::from("bad\nname"),
            String::new(),
        ],
    );
    let manual_expected = BTreeSet::from(["valid_recipe"]);
    assert_eq!(manual_config.get_recipes(), manual_expected);
}

#[test]
fn group_and_recipe_sharing_name_remain_distinct_in_parsing_membership_and_expansion() {
    let config_input = "\
general {
    editor vim
    simulate false
}

pods {
    path /pods
}

recipes {
    path ./recipes

    groups {
        all @alpha @stack
        stack media cloud proxy
        alpha beta alpha
    }
}
";
    let config = Config::from_str(config_input).unwrap();
    assert_eq!(config.editor, "vim");
    assert!(!config.simulate);
    assert_eq!(config.pods.path, PathBuf::from("/pods"));
    assert_eq!(config.recipes.path, PathBuf::from("./recipes"));
    assert_eq!(config.recipes.groups.len(), 3);

    assert!(config.has_recipe("alpha"));
    assert!(config.has_recipe("beta"));
    assert!(config.has_recipe("media"));
    assert!(config.has_recipe("cloud"));
    assert!(config.has_recipe("proxy"));
    assert!(!config.has_recipe("@alpha"));
    assert!(!config.has_recipe("@stack"));
    assert!(!config.has_recipe("@all"));
    assert!(!config.has_recipe("stack"));
    assert!(!config.has_recipe("all"));

    let expected_recipes = BTreeSet::from(["alpha", "beta", "cloud", "media", "proxy"]);
    assert_eq!(config.get_recipes(), expected_recipes);

    assert_eq!(
        config.expand_targets(&[String::from("@alpha")]).unwrap(),
        vec![String::from("beta"), String::from("alpha")]
    );
    assert_eq!(
        config.expand_targets(&[String::from("alpha")]).unwrap(),
        vec![String::from("alpha")]
    );
    assert_eq!(
        config
            .expand_targets(&[String::from("alpha"), String::from("@alpha")])
            .unwrap(),
        vec![String::from("alpha"), String::from("beta")]
    );
    assert_eq!(
        config
            .expand_targets(&[String::from("@alpha"), String::from("alpha")])
            .unwrap(),
        vec![String::from("beta"), String::from("alpha")]
    );
    assert_eq!(
        config.expand_targets(&[String::from("@all")]).unwrap(),
        vec![
            String::from("beta"),
            String::from("alpha"),
            String::from("media"),
            String::from("cloud"),
            String::from("proxy"),
        ]
    );

    for group_only in ["stack", "all"] {
        let error = config
            .expand_targets(&[String::from(group_only)])
            .unwrap_err();
        match error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
                assert_eq!(config_error.token.as_deref(), Some(group_only));
            }
            other => panic!("expected RecipeNotFound for {group_only}, got {other:?}"),
        }
    }

    for recipe_only_as_group in ["@beta", "@media", "@cloud", "@proxy"] {
        let error = config
            .expand_targets(&[String::from(recipe_only_as_group)])
            .unwrap_err();
        match error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::UnknownGroup);
                assert_eq!(config_error.token.as_deref(), Some(recipe_only_as_group));
            }
            other => panic!("expected UnknownGroup for {recipe_only_as_group}, got {other:?}"),
        }
    }

    let cyclic_input = "\
recipes {
    path ./recipes

    groups {
        all @alpha @stack
        stack media cloud proxy
        alpha beta @alpha
    }
}
";
    let cyclic_error = Config::from_str(cyclic_input).unwrap_err();
    match cyclic_error {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, ConfigErrorKind::InvalidGroupTarget);
            assert_eq!(config_error.line, Some(7));
            assert_eq!(config_error.token.as_deref(), Some("@alpha"));
            let diagnostic = config_error.diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.line_number, 7);
            assert_eq!(diagnostic.col_number, 20);
            assert_eq!(diagnostic.annotation_carets, "                   ^^^^^^");
        }
        other => panic!("expected InvalidGroupTarget, got {other:?}"),
    }
}
