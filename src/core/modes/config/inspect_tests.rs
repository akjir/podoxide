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

//! Unit tests for `pods config inspect` group expansion and rendering in [`super`].

use std::fs as std_fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::str::FromStr;

use crate::core::config::Config;
use crate::core::context::Context;
use crate::error::PodError;
use crate::io::system::fs::{self, TestTempDir};
use super::{MAX_GROUP_OUTPUT_LINES, build_groups_state, handle, inspect_with};

const VALID_RECIPE: &str = "\
pod test-pod {
    name \"Test Pod\"
    registry docker.io
    path /pods/test-pod
}
container *app {
    image alpine:latest
}
";

#[test]
fn inspect_with_clamps_diamond_group_dag_output_emits_truncation_notice_and_validates_recipes() {
    let temp_dir = TestTempDir::new("config_inspect_diamond");
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&recipes_dir).unwrap();

    let mut config = Config::new();
    config.recipes.path = recipes_dir;
    for i in 0..30 {
        let group_name = format!("g{i:02}");
        let next_target = format!("@g{:02}", i + 1);
        config
            .recipes
            .groups
            .insert(group_name, vec![next_target.clone(), next_target]);
    }
    config
        .recipes
        .groups
        .insert(String::from("g30"), vec![String::from("leaf_recipe")]);

    let state = build_groups_state(&config);
    assert_eq!(state.output_lines.len(), MAX_GROUP_OUTPUT_LINES);
    assert!(state.truncated);
    assert!(state.referenced_recipes.contains("leaf_recipe"));
    assert!(state.missing_recipes.contains("leaf_recipe"));

    let mut stdout = Vec::new();
    inspect_with(&config, &mut stdout).unwrap();
    let output = String::from_utf8(stdout).unwrap();
    assert!(output.contains("  Pods:         -"));
    assert!(output.contains("[NOT SET]\n"));
    assert!(output.contains("[OK, 0 known recipes found]\n"));
    assert!(output.contains(&format!(
        "... (output truncated at {MAX_GROUP_OUTPUT_LINES} lines)\n"
    )));
    assert!(!output.contains("Unreferenced Recipes:"));
    assert!(output.contains("Validation Summary:\n"));
    assert!(output.contains("- Recipe file for 'leaf_recipe' not found!\n"));
}

#[test]
fn inspect_with_displays_settings_directories_and_clean_config() {
    let temp_dir = TestTempDir::new("config_inspect_clean");
    let pods_dir = temp_dir.join("pods");
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&pods_dir).unwrap();
    std_fs::create_dir_all(&recipes_dir).unwrap();

    fs::write_file(&recipes_dir.join("app_one.pod"), VALID_RECIPE).unwrap();
    fs::write_file(&recipes_dir.join("ignored.txt"), "not a recipe").unwrap();

    let mut config = Config::new();
    config.editor = String::from("vim");
    config.path = temp_dir.join("pods.conf");
    config.pods.path = pods_dir;
    config.recipes.path = recipes_dir;
    config.simulate = true;
    config
        .recipes
        .groups
        .insert(String::from("all"), vec![String::from("app_one")]);

    let mut stdout = Vec::new();
    inspect_with(&config, &mut stdout).unwrap();
    let output = String::from_utf8(stdout).unwrap();

    assert!(output.contains("Configuration: "));
    assert!(output.contains("pods.conf\n"));
    assert!(output.contains("Settings:\n  Editor:       vim\n  Simulate:     true\n"));
    assert!(output.contains("Directories:\n"));
    assert!(output.contains("[OK]\n"));
    assert!(output.contains("[OK, 1 known recipe found]\n"));
    assert!(output.contains("Groups:\n  • all\n"));
    assert!(output.contains("    └── app_one                             [OK]\n"));
    assert!(!output.contains("truncated"));
    assert!(!output.contains("Unreferenced Recipes:"));
    assert!(!output.contains("Validation Summary:"));

    let mut context = Context::new();
    context.config = config;
    assert!(handle(&context).is_ok());
}

#[test]
fn inspect_with_distinguishes_subgroup_from_same_named_recipe_leaf() {
    let temp_dir = TestTempDir::new("config_inspect_same_named");
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&recipes_dir).unwrap();

    fs::write_file(&recipes_dir.join("alpha.pod"), VALID_RECIPE).unwrap();
    fs::write_file(&recipes_dir.join("beta.pod"), VALID_RECIPE).unwrap();
    fs::write_file(&recipes_dir.join("stack.pod"), VALID_RECIPE).unwrap();

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
    let mut config = Config::from_str(config_input).unwrap();
    config.recipes.path = recipes_dir;

    let mut stdout = Vec::new();
    inspect_with(&config, &mut stdout).unwrap();
    let output = String::from_utf8(stdout).unwrap();

    assert!(!output.lines().any(|line| line == "    "));
    assert!(output.contains("Settings:\n  Editor:       vim\n  Simulate:     false\n"));
    assert!(output.contains("[OK, 2 known and 1 unreferenced recipe found]\n"));
    assert!(output.contains(
        "  • all\n    ├── @alpha\n    │   ├── beta                            [OK]\n    │   └── alpha                           [OK]\n    └── @stack\n        ├── media                           [NOT FOUND]\n        ├── cloud                           [NOT FOUND]\n        └── proxy                           [NOT FOUND]\n"
    ));
    assert!(output.contains(
        "  • alpha\n    ├── beta                                [OK]\n    └── alpha                               [OK]\n"
    ));
    assert!(output.contains(
        "  • stack\n    ├── media                               [NOT FOUND]\n    ├── cloud                               [NOT FOUND]\n    └── proxy                               [NOT FOUND]\n"
    ));
    assert!(!output.contains("[MISSING GROUP]"));
    assert!(output.contains(
        "Unreferenced Recipes:\n  • stack                                   [UNREFERENCED]\n"
    ));
    assert!(output.contains(
        "Validation Summary:\n- Recipe files for 'cloud', 'media' and 'proxy' not found!\n- 1 unreferenced recipe found!\n"
    ));
}

#[test]
fn inspect_with_handles_missing_invalid_directories_empty_editor_and_singular_warnings() {
    let temp_dir = TestTempDir::new("config_inspect_singular");
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&recipes_dir).unwrap();
    fs::write_file(&recipes_dir.join("orphan.pod"), "invalid {{{ psl").unwrap();
    fs::write_file(&recipes_dir.join("broken_one.pod"), "invalid {{{ psl").unwrap();

    let mut config = Config::new();
    config.editor = String::new();
    config.path = PathBuf::new();
    config.pods.path = PathBuf::from("/pods");
    config.recipes.path = recipes_dir.clone();
    config.recipes.groups.insert(
        String::from("all"),
        vec![String::from("missing_one"), String::from("broken_one")],
    );

    let mut stdout = Vec::new();
    inspect_with(&config, &mut stdout).unwrap();
    let output = String::from_utf8(stdout).unwrap();

    assert!(output.contains("Configuration: \n"));
    assert!(output.contains("  Editor:       -\n"));
    assert!(output.contains("  Pods:"));
    assert!(output.contains("[NOT FOUND]\n"));
    assert!(output.contains("[OK, 1 known and 1 unreferenced recipe found]\n"));
    assert!(output.contains("    ├── missing_one                         [NOT FOUND]\n"));
    assert!(output.contains("    └── broken_one                          [ERROR]\n"));
    assert!(output.contains(
        "Unreferenced Recipes:\n  • orphan                                  [UNREFERENCED]\n"
    ));
    assert!(output.contains("Validation Summary:\n"));
    assert!(output.contains("- Recipe file for 'missing_one' not found!\n"));
    assert!(output.contains(
        "- Recipe 'broken_one' has errors! Run 'pods recipe inspect broken_one' for details.\n"
    ));
    assert!(output.contains("- 1 unreferenced recipe found!\n"));

    // Nonexistent recipes.path renders [NOT FOUND].
    config.recipes.path = temp_dir.join("nonexistent_recipes");
    let mut missing_recipes_dir_out = Vec::new();
    inspect_with(&config, &mut missing_recipes_dir_out).unwrap();
    let missing_dir_text = String::from_utf8(missing_recipes_dir_out).unwrap();
    assert!(missing_dir_text.contains("Recipes:"));
    assert!(missing_dir_text.contains("[NOT FOUND]\n"));
    assert!(!missing_dir_text.contains("Unreferenced Recipes:"));

    // Invalid recipes.path containing `..` is rejected via is_valid_directory and never listed.
    config.recipes.path = recipes_dir.join("..").join("recipes");
    let mut bad_path_out = Vec::new();
    inspect_with(&config, &mut bad_path_out).unwrap();
    let bad_path_text = String::from_utf8(bad_path_out).unwrap();
    assert!(bad_path_text.contains("[NOT FOUND]\n"));
    assert!(!bad_path_text.contains("[OK, "));
    assert!(!bad_path_text.contains("Unreferenced Recipes:"));

    // Unreadable recipes directory must fail without emitting partial stdout (unless running as root).
    let unreadable_dir = temp_dir.join("unreadable_recipes");
    std_fs::create_dir_all(&unreadable_dir).unwrap();
    std_fs::set_permissions(&unreadable_dir, std_fs::Permissions::from_mode(0o000)).unwrap();
    let blocked = std_fs::read_dir(&unreadable_dir).is_err();
    config.recipes.path = unreadable_dir.clone();
    let mut error_out = Vec::new();
    let error_result = inspect_with(&config, &mut error_out);
    std_fs::set_permissions(&unreadable_dir, std_fs::Permissions::from_mode(0o700)).unwrap();
    if blocked {
        assert!(matches!(error_result, Err(PodError::Io(_))));
        assert!(
            error_out.is_empty(),
            "failed config inspect must not write partial stdout, got {:?}",
            String::from_utf8_lossy(&error_out)
        );
    }
}

#[test]
fn inspect_with_renders_tree_hierarchy_cyclic_and_missing_groups_and_plural_summary() {
    let temp_dir = TestTempDir::new("config_inspect_hierarchy");
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&recipes_dir).unwrap();

    fs::write_file(&recipes_dir.join("recipe_001_empty.pod"), "").unwrap();
    fs::write_file(&recipes_dir.join("recipe_002_no_pod.pod"), "").unwrap();
    fs::write_file(&recipes_dir.join("recipe_003_extra.pod"), "").unwrap();
    fs::write_file(&recipes_dir.join("broken_second.pod"), "pod {\n").unwrap();

    let mut config = Config::new();
    config.editor = String::from("vim");
    config.simulate = false;
    config.pods.path = temp_dir.join("non_existent_pods_dir");
    config.recipes.path = recipes_dir;
    config
        .recipes
        .groups
        .insert(String::from("all"), vec![String::from("missing_recipe")]);
    config
        .recipes
        .groups
        .insert(String::from("cycle_a"), vec![String::from("@cycle_b")]);
    config
        .recipes
        .groups
        .insert(String::from("cycle_b"), vec![String::from("@cycle_a")]);
    config.recipes.groups.insert(
        String::from("database"),
        vec![
            String::from("recipe_001_empty"),
            String::from("broken_second"),
            String::from("very_long_missing_recipe_name_exceeding_column_forty_four"),
        ],
    );
    config
        .recipes
        .groups
        .insert(String::from("meta"), vec![String::from("@stack")]);
    config.recipes.groups.insert(
        String::from("stack"),
        vec![
            String::from("@missing_mid"),
            String::from("@database"),
            String::from("@web"),
            String::from("@missing_end"),
        ],
    );
    config
        .recipes
        .groups
        .insert(String::from("web"), vec![String::from("frontend")]);

    let mut stdout = Vec::new();
    inspect_with(&config, &mut stdout).unwrap();
    let output = String::from_utf8(stdout).unwrap();

    assert!(
        !output.lines().any(|line| line == "    "),
        "group separator blank lines must have zero trailing spaces"
    );
    assert!(output.contains("[OK, 2 known and 2 unreferenced recipes found]\n"));
    assert!(output.contains("  • all\n"));
    assert!(output.contains("    └── missing_recipe"));
    assert!(output.contains(
        "  • cycle_a\n    └── @cycle_b\n        └── @cycle_a\n            └── [CYCLIC GROUP]\n"
    ));
    assert!(output.contains("  • database\n"));
    assert!(output.contains("    ├── recipe_001_empty"));
    assert!(output.contains("[ERROR]\n"));
    assert!(output.contains(
        "  • meta\n    └── @stack\n        ├── @missing_mid\n        │   └── [MISSING GROUP]\n"
    ));
    assert!(output.contains("        ├── @database\n        │   ├── recipe_001_empty"));
    assert!(output.contains("  • stack\n"));
    assert!(output.contains("    ├── @missing_mid\n    │   └── [MISSING GROUP]\n"));
    assert!(output.contains("    ├── @database\n    │   ├── recipe_001_empty"));
    assert!(output.contains("    ├── @web\n    │   └── frontend"));
    assert!(output.contains("    └── @missing_end\n        └── [MISSING GROUP]\n"));
    assert!(output.contains("  • web\n"));
    assert!(output.contains(
        "Unreferenced Recipes:\n  • recipe_002_no_pod                       [UNREFERENCED]\n  • recipe_003_extra                        [UNREFERENCED]\n"
    ));
    assert!(output.contains("Validation Summary:\n"));
    assert!(!output.contains("'@"));
    assert!(output.contains(
        "- Recipe files for 'frontend', 'missing_recipe' and 'very_long_missing_recipe_name_exceeding_column_forty_four' not found!\n"
    ));
    assert!(output.contains(
        "- Recipes 'broken_second' and 'recipe_001_empty' have errors! Run 'pods recipe inspect <recipe>' for details.\n"
    ));
    assert!(output.contains("- 2 unreferenced recipes found!\n"));
}
