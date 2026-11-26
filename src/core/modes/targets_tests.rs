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

//! Unit tests for shared target resolution and recipe discovery helpers in [`super`].

use std::collections::BTreeSet;
use std::env;
use std::fs as std_fs;
use std::path::Path;
use std::process;
use std::str::FromStr;

use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
use crate::core::container;
use crate::core::context::Context;
use crate::core::pod;
use crate::error::{CliErrorKind, ConfigErrorKind, PodError, RecipeErrorKind};
use crate::io::exec::command::ExecOutcome;
use crate::io::system::fs;
use super::{
    RecipeInspection, RecipeStatus, collect_container_specs, execute_lifecycle_action,
    inspect_recipe, list_unreferenced_recipes, resolve_container_targets,
    split_recipe_container_spec,
};

const ALPHA_RECIPE_PSL: &str = "\
pod alpha_pod {
    name \"Alpha Pod\"
    registry docker.io
}
container *app {
    image alpine:latest
}
";

const BETA_RECIPE_PSL: &str = "\
pod beta_pod {
    name \"Beta Pod\"
    registry docker.io
}
container *app {
    image alpine:latest
}
";

#[test]
fn execute_lifecycle_action_distinguishes_group_and_recipe_sharing_name() {
    let temp_dir = env::temp_dir().join(format!("podoxide_targets_same_name_{}", process::id()));
    let _ = std_fs::remove_dir_all(&temp_dir);
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&recipes_dir).unwrap();

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
    config.simulate = true;
    config.recipes.path = recipes_dir.clone();

    fs::write_file(&recipes_dir.join("alpha.pod"), ALPHA_RECIPE_PSL).unwrap();

    let mut context = Context::new();
    context.config = config;

    context.parameters = vec![String::from("alpha")];
    assert!(execute_lifecycle_action(&context, pod::up, container::up).is_ok());

    context.parameters = vec![String::from("@alpha")];
    let missing_beta = execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
    match missing_beta {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::NotFound);
        }
        other => panic!("expected PodError::Recipe(NotFound) for beta.pod, got {other:?}"),
    }

    fs::write_file(&recipes_dir.join("beta.pod"), BETA_RECIPE_PSL).unwrap();
    assert!(execute_lifecycle_action(&context, pod::up, container::up).is_ok());

    context.parameters = vec![String::from("stack")];
    let stack_as_recipe = execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
    match stack_as_recipe {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            assert_eq!(config_error.token.as_deref(), Some("stack"));
        }
        other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
    }

    let _ = std_fs::remove_dir_all(&temp_dir);
}

#[test]
fn execute_lifecycle_action_handles_container_targets_and_rejects_invalid_specs() {
    let mut context = Context::new();
    context.config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

    context.parameters = vec![
        String::from("recipe/1"),
        String::from("recipe/*db"),
        String::from("recipe/db"),
        String::from("recipe/example-stack-db"),
        String::from("recipe/app"),
    ];
    let specs = collect_container_specs(&context).unwrap();
    let (recipes, resolved) = resolve_container_targets(&context.config, &specs).unwrap();
    assert_eq!(resolved, vec![("recipe", 0), ("recipe", 1)]);
    assert_eq!(recipes["recipe"].containers[0].name, "example-stack-db");
    assert_eq!(recipes["recipe"].containers[1].name, "example-stack-app");
    assert!(execute_lifecycle_action(&context, pod::up, container::up).is_ok());

    context.parameters = vec![String::from("unknown_recipe/1")];
    let unknown_recipe = execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
    match unknown_recipe {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
        }
        other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
    }

    context.parameters = vec![String::from("recipe/missing")];
    let unknown_container = execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
    match unknown_container {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotFound);
        }
        other => panic!("expected PodError::Recipe(ContainerNotFound), got {other:?}"),
    }

    context.parameters = vec![String::from("recipe"), String::from("recipe/1")];
    let mixed_pod_first = execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
    match mixed_pod_first {
        PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MixedTargets),
        other => panic!("expected PodError::Cli(MixedTargets), got {other:?}"),
    }

    context.parameters = vec![String::from("recipe/1"), String::from("recipe")];
    let mixed_container_first =
        execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
    match mixed_container_first {
        PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MixedTargets),
        other => panic!("expected PodError::Cli(MixedTargets), got {other:?}"),
    }

    let invalid_targets = [
        "recipe/",
        "/1",
        "./1",
        "recipe/..",
        "-recipe/1",
        "recipe/-flag",
        "@group/1",
        "recipe/db/extra",
    ];
    for target in invalid_targets {
        context.parameters = vec![String::from(target)];
        let Err(PodError::Cli(error)) = execute_lifecycle_action(&context, pod::up, container::up)
        else {
            panic!("expected PodError::Cli(InvalidTarget) for {target}");
        };
        assert_eq!(error.kind, CliErrorKind::InvalidTarget);
    }
}

#[test]
fn execute_lifecycle_action_handles_numeric_container_names_and_upfront_pod_validation() {
    let temp_dir = env::temp_dir().join(format!("podoxide_targets_numeric_{}", process::id()));
    let _ = std_fs::remove_dir_all(&temp_dir);
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&recipes_dir).unwrap();

    fs::write_file(
        &recipes_dir.join("num_two.pod"),
        "pod num_two_pod {\n    name \"Numeric Two\"\n    path /pods/num_two\n    registry docker.io\n}\ncontainer 2 {\n    image alpine:latest\n}\ncontainer *db {\n    image postgres:16\n}\n",
    )
    .unwrap();
    fs::write_file(
        &recipes_dir.join("num_five.pod"),
        "pod num_five_pod {\n    name \"Numeric Five\"\n    path /pods/num_five\n    registry docker.io\n}\ncontainer 5 {\n    image alpine:latest\n}\n",
    )
    .unwrap();
    fs::write_file(&recipes_dir.join("valid.pod"), ALPHA_RECIPE_PSL).unwrap();
    fs::write_file(&recipes_dir.join("broken.pod"), "invalid {{{ psl").unwrap();

    let mut config = Config::new();
    config.simulate = true;
    config.pods.path = temp_dir.join("pods");
    config.recipes.path = recipes_dir;
    config.recipes.groups.insert(
        String::from("all"),
        vec![
            String::from("num_two"),
            String::from("num_five"),
            String::from("valid"),
            String::from("broken"),
        ],
    );

    let mut context = Context::new();
    context.config = config;

    let mut recorded = Vec::new();
    context.parameters = vec![String::from("num_two/1")];
    execute_lifecycle_action(&context, pod::up, |item, _, _| {
        recorded.push(item.name.clone());
        Ok(ExecOutcome::Succeeded)
    })
    .unwrap();
    assert_eq!(
        recorded,
        vec![String::from("2")],
        "num_two/1 must target container #1 named '2', not container #2"
    );

    recorded.clear();
    context.parameters = vec![String::from("num_five/1")];
    execute_lifecycle_action(&context, pod::up, |item, _, _| {
        recorded.push(item.name.clone());
        Ok(ExecOutcome::Succeeded)
    })
    .unwrap();
    assert_eq!(
        recorded,
        vec![String::from("5")],
        "num_five/1 must target container #1 named '5'"
    );

    let mut pod_calls = 0usize;
    context.parameters = vec![String::from("valid"), String::from("broken")];
    assert!(
        execute_lifecycle_action(
            &context,
            |_, _| {
                pod_calls += 1;
                Ok(ExecOutcome::Succeeded)
            },
            container::up,
        )
        .is_err()
    );
    assert_eq!(
        pod_calls, 0,
        "broken second recipe must abort before invoking pod_action on first recipe"
    );

    let _ = std_fs::remove_dir_all(&temp_dir);
}

#[test]
fn execute_lifecycle_action_rejects_empty_and_unknown_targets() {
    let mut context = Context::new();
    context.config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

    let empty_error = execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
    match empty_error {
        PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
        other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
    }

    context.parameters = vec![String::from("unknown_recipe")];
    let unknown_error = execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
    match unknown_error {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
        }
        other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
    }
}

#[test]
fn inspect_recipe_split_spec_and_list_unreferenced_cover_edge_cases() {
    assert_eq!(
        split_recipe_container_spec("MyApp_1").unwrap(),
        ("MyApp_1", None)
    );
    assert_eq!(
        split_recipe_container_spec("recipe/1").unwrap(),
        ("recipe", Some("1"))
    );
    assert!(split_recipe_container_spec("recipe/").is_err());

    let temp_dir = env::temp_dir().join(format!("podoxide_targets_inspect_{}", process::id()));
    let _ = std_fs::remove_dir_all(&temp_dir);
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&recipes_dir).unwrap();
    fs::write_file(&recipes_dir.join("alpha.pod"), ALPHA_RECIPE_PSL).unwrap();
    fs::write_file(&recipes_dir.join("broken.pod"), "invalid {{{ psl").unwrap();
    fs::write_file(&recipes_dir.join("orphan.pod"), ALPHA_RECIPE_PSL).unwrap();

    let mut config = Config::new();
    config.pods.path = temp_dir.join("pods");
    config.recipes.path = recipes_dir.clone();

    let ok_inspected = inspect_recipe(&config, "alpha");
    assert!(matches!(ok_inspected, RecipeInspection::Ok(_)));
    assert_eq!(ok_inspected.status(), RecipeStatus::Ok);
    assert_eq!(ok_inspected.status().badge(), "[OK]");

    let err_inspected = inspect_recipe(&config, "broken");
    assert!(matches!(err_inspected, RecipeInspection::Error));
    assert_eq!(err_inspected.status().badge(), "[ERROR]");

    let missing_inspected = inspect_recipe(&config, "missing");
    assert!(matches!(missing_inspected, RecipeInspection::NotFound));
    assert_eq!(missing_inspected.status().badge(), "[NOT FOUND]");

    let referenced = BTreeSet::from(["alpha", "broken"]);
    let (total, unreferenced) = list_unreferenced_recipes(&recipes_dir, &referenced)
        .unwrap()
        .unwrap();
    assert_eq!(total, 3);
    assert_eq!(unreferenced, vec![String::from("orphan.pod")]);

    assert!(
        list_unreferenced_recipes(&recipes_dir.join("..").join("recipes"), &referenced)
            .unwrap()
            .is_none()
    );
    assert!(
        list_unreferenced_recipes(&temp_dir.join("nonexistent"), &referenced)
            .unwrap()
            .is_none()
    );

    let _ = std_fs::remove_dir_all(&temp_dir);
}
