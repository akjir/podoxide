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

//! Unit tests for `.pod` recipe semantic validation, command completeness, and zero-line errors in [`super`].

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::str::FromStr;

use crate::core::config::Config;
use crate::core::container::Container;
use crate::core::log;
use crate::core::pod::{Pod, PodCommand};
use crate::error::{PodError, RecipeErrorKind};
use crate::io::system::fs as system_fs;
use super::{Recipe, validation};

fn assert_validate_error(
    recipe: &mut Recipe,
    config: &Config,
    file_name: &str,
    expected_kind: RecipeErrorKind,
) {
    let error = validation::validate_recipe_inner(recipe, config, file_name).unwrap_err();
    assert_eq!(error.kind, expected_kind, "case: {file_name}");
}

#[test]
fn recipe_validation_rejects_incomplete_commands_at_command_line_and_zero_line_omits_line_zero() {
    let temp_dir = env::temp_dir().join(format!("podoxide_cmd_line_test_{}", process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let missing_container_path = temp_dir.join("missing_container.pod");
    system_fs::write_file(
        &missing_container_path,
        "\
pod app {
    name App
    path /pods/app
    registry docker.io
    command migrate {
        execute \"sh migrate.sh\"
    }
}
container *db {
    image postgres:16
}
",
    )
    .unwrap();

    let missing_execute_path = temp_dir.join("missing_execute.pod");
    system_fs::write_file(
        &missing_execute_path,
        "\
pod app {
    name App
    path /pods/app
    registry docker.io
    command migrate {
        container *db
    }
}
container *db {
    image postgres:16
}
",
    )
    .unwrap();

    let mut temp_config = Config::new();
    temp_config.recipes.path = temp_dir.clone();

    for target in ["missing_container", "missing_execute"] {
        let error = Recipe::inspect(&temp_config, target).unwrap_err();
        match error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidCommand);
                assert_eq!(
                    recipe_error.line,
                    Some(5),
                    "expected command block line 5 for {target}, got {:?}",
                    recipe_error.line
                );
            }
            other => panic!("expected InvalidCommand at line 5 for {target}, got {other:?}"),
        }
    }
    let _ = fs::remove_dir_all(&temp_dir);

    let mut zero_pod = Pod::new();
    zero_pod.id = String::from("-bad");
    zero_pod.line = 0;
    let zero_pod_err = zero_pod.validate_create().unwrap_err();
    assert_eq!(zero_pod_err.line, None);
    assert!(!zero_pod_err.to_string().contains("line 0:"));
    assert!(!zero_pod_err.to_string().contains("line 1:"));

    let mut zero_container = Container::new();
    zero_container.name = String::from("app");
    zero_container.line = 0;
    let zero_container_err = zero_container.validate().unwrap_err();
    assert_eq!(zero_container_err.line, None);
    assert!(!zero_container_err.to_string().contains("line 0:"));
    assert!(!zero_container_err.to_string().contains("line 1:"));
}

#[test]
fn recipe_validation_rejects_missing_and_invalid_fields() {
    let mut config_no_pod_path = Config::new();
    config_no_pod_path.pods.path = PathBuf::new();

    let mut config_with_pod_path = Config::new();
    config_with_pod_path.pods.path = PathBuf::from("/pods");

    let mut no_pod = Recipe::from_str("container {\n    image alpine:latest\n}\n").unwrap();
    assert_validate_error(
        &mut no_pod,
        &config_with_pod_path,
        "recipe_002_no_pod",
        RecipeErrorKind::MissingPodBlock,
    );
    assert_validate_error(
        &mut no_pod,
        &config_with_pod_path,
        "../unsafe_name",
        RecipeErrorKind::MissingPodBlock,
    );
    let missing_pod_error =
        validation::validate_recipe_inner(&mut no_pod, &config_with_pod_path, "missing_pod")
            .unwrap_err();
    assert_eq!(missing_pod_error.kind, RecipeErrorKind::MissingPodBlock);
    assert_eq!(missing_pod_error.line, None);
    assert!(!missing_pod_error.to_string().contains("line 0:"));

    let mut no_name = Recipe::from_str(
        "pod test {\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
    )
    .unwrap();
    assert_validate_error(
        &mut no_name,
        &config_with_pod_path,
        "recipe_001_empty",
        RecipeErrorKind::MissingPodName,
    );
    for control_name in ["   ", "\nApp\n", "\tApp\t", "\u{0085}App\u{0085}"] {
        no_name.pod.name = String::from(control_name);
        assert_validate_error(
            &mut no_name,
            &config_with_pod_path,
            "control_pod_name",
            RecipeErrorKind::InvalidPodName,
        );
    }
    no_name.pod.name = String::from("  Trimmed Name  ");
    validation::validate_recipe_inner(&mut no_name, &config_with_pod_path, "trimmed_pod_name")
        .unwrap();
    assert_eq!(no_name.pod.name, "Trimmed Name");

    let mut no_registry =
        Recipe::from_str("pod test {\n    name Test\n}\ncontainer {\n    image alpine:latest\n}\n")
            .unwrap();
    assert_validate_error(
        &mut no_registry,
        &config_with_pod_path,
        "recipe_004_no_registry",
        RecipeErrorKind::MissingPodRegistry,
    );

    let mut no_path = Recipe::from_str(
        "pod nopathpod {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
    )
    .unwrap();
    assert_validate_error(
        &mut no_path,
        &config_no_pod_path,
        "recipe_005_no_path",
        RecipeErrorKind::MissingPodPath,
    );
    for invalid_fallback in ["/", "-flag", "   ", "../escape"] {
        let mut config_invalid_fallback = Config::new();
        config_invalid_fallback.pods.path = PathBuf::from(invalid_fallback);
        let mut fallback_recipe = no_path.clone();
        assert_validate_error(
            &mut fallback_recipe,
            &config_invalid_fallback,
            "invalid_fallback_pods_path",
            RecipeErrorKind::InvalidPath,
        );
    }

    let base_valid = Recipe::from_str(
        "pod test {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
    )
    .unwrap();

    let mut invalid_description = base_valid.clone();
    invalid_description.pod.description = String::from("   ");
    assert_validate_error(
        &mut invalid_description,
        &config_with_pod_path,
        "invalid_description",
        RecipeErrorKind::InvalidDescription,
    );

    let mut invalid_publish = base_valid.clone();
    invalid_publish.pod.publish.push(String::from("-8080:80"));
    assert_validate_error(
        &mut invalid_publish,
        &config_with_pod_path,
        "invalid_publish",
        RecipeErrorKind::InvalidPublish,
    );

    let mut invalid_option = base_valid.clone();
    invalid_option.pod.options.push(String::from("   "));
    assert_validate_error(
        &mut invalid_option,
        &config_with_pod_path,
        "invalid_option",
        RecipeErrorKind::InvalidOption,
    );

    let mut no_containers =
        Recipe::from_str("pod nocontainers {\n    name Test\n    registry docker.io\n}\n").unwrap();
    assert_validate_error(
        &mut no_containers,
        &config_with_pod_path,
        "recipe_006_no_containers",
        RecipeErrorKind::MissingContainersBlock,
    );

    let mut missing_image = Recipe::from_str(
        "pod test {\n    name Test\n    registry docker.io\n}\ncontainer app {\n    restart never\n}\n",
    )
    .unwrap();
    assert_validate_error(
        &mut missing_image,
        &config_with_pod_path,
        "missing_image",
        RecipeErrorKind::MissingContainerImage,
    );

    let mut duplicate_containers = Recipe::from_str(
        "\
pod my_pod {
    name Test
    registry docker.io
}
container my_pod-2 {
    image alpine:1
}
container *2 {
    image alpine:2
}
",
    )
    .unwrap();
    assert_validate_error(
        &mut duplicate_containers,
        &config_with_pod_path,
        "dup_containers",
        RecipeErrorKind::InvalidContainerName,
    );

    let mut control_pod_id = base_valid.clone();
    control_pod_id.pod.id = String::from("bad\x07id");
    assert_validate_error(
        &mut control_pod_id,
        &config_with_pod_path,
        "control_pod_id",
        RecipeErrorKind::InvalidPodId,
    );

    let mut invalid_pod_id = base_valid.clone();
    invalid_pod_id.pod.id = String::from("-invalid");
    assert_validate_error(
        &mut invalid_pod_id,
        &config_with_pod_path,
        "invalid_pod_id",
        RecipeErrorKind::InvalidPodId,
    );

    for bad_registry in ["-bad-registry", "docker.io/", "https://docker.io"] {
        let mut invalid_registry = base_valid.clone();
        invalid_registry.pod.registry = String::from(bad_registry);
        assert_validate_error(
            &mut invalid_registry,
            &config_with_pod_path,
            "invalid_registry",
            RecipeErrorKind::InvalidRegistry,
        );
    }

    let mut invalid_pod_path = base_valid.clone();
    invalid_pod_path.pod.path = PathBuf::from("../escape");
    assert_validate_error(
        &mut invalid_pod_path,
        &config_with_pod_path,
        "invalid_pod_path",
        RecipeErrorKind::InvalidPath,
    );

    let mut valid_command = PodCommand::new();
    valid_command.name = String::from("migrate");
    valid_command.container = String::from("*db");
    valid_command.execute = String::from("sh migrate.sh");

    let mut mismatched_command_key = base_valid.clone();
    mismatched_command_key
        .pod
        .commands
        .insert(String::from("other"), valid_command.clone());
    assert_validate_error(
        &mut mismatched_command_key,
        &config_with_pod_path,
        "mismatched_command_key",
        RecipeErrorKind::InvalidCommand,
    );

    let mut empty_command = base_valid.clone();
    let mut blank_command = PodCommand::new();
    blank_command.name = String::from("migrate");
    empty_command
        .pod
        .commands
        .insert(String::from("migrate"), blank_command);
    assert_validate_error(
        &mut empty_command,
        &config_with_pod_path,
        "empty_command",
        RecipeErrorKind::InvalidCommand,
    );

    let mut invalid_command_container = base_valid.clone();
    let mut bad_container_command = valid_command.clone();
    bad_container_command.container = String::from("   ");
    invalid_command_container
        .pod
        .commands
        .insert(String::from("migrate"), bad_container_command);
    assert_validate_error(
        &mut invalid_command_container,
        &config_with_pod_path,
        "invalid_command_container",
        RecipeErrorKind::InvalidCommand,
    );

    let mut invalid_command_description = base_valid.clone();
    let mut bad_description_command = valid_command.clone();
    bad_description_command.description = String::from("bad\x07desc");
    invalid_command_description
        .pod
        .commands
        .insert(String::from("migrate"), bad_description_command);
    assert_validate_error(
        &mut invalid_command_description,
        &config_with_pod_path,
        "invalid_command_description",
        RecipeErrorKind::InvalidCommand,
    );

    let mut invalid_command_execute = base_valid.clone();
    let mut bad_execute_command = valid_command.clone();
    bad_execute_command.execute = String::from("   ");
    invalid_command_execute
        .pod
        .commands
        .insert(String::from("migrate"), bad_execute_command);
    assert_validate_error(
        &mut invalid_command_execute,
        &config_with_pod_path,
        "invalid_command_execute",
        RecipeErrorKind::InvalidCommand,
    );

    let mut invalid_command_user = base_valid.clone();
    let mut bad_user_command = valid_command;
    bad_user_command.user = Some(String::from("-root"));
    invalid_command_user
        .pod
        .commands
        .insert(String::from("migrate"), bad_user_command);
    assert_validate_error(
        &mut invalid_command_user,
        &config_with_pod_path,
        "invalid_command_user",
        RecipeErrorKind::InvalidCommand,
    );

    let mut debug_fallback_recipe = base_valid;
    log::set_debug_enabled(true);
    let debug_result = validation::validate_recipe_inner(
        &mut debug_fallback_recipe,
        &config_with_pod_path,
        "debug_fallback",
    );
    log::set_debug_enabled(false);
    debug_result.unwrap();
    assert_eq!(debug_fallback_recipe.pod.path, PathBuf::from("/pods/test"));

    let temp_dir = env::temp_dir().join(format!("podoxide_validate_diag_{}", process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    let broken_path = temp_dir.join("broken.pod");
    system_fs::write_file(
        &broken_path,
        "pod broken {\n    name Broken\n}\ncontainer {\n    image alpine:latest\n}\n",
    )
    .unwrap();
    let mut config_temp = Config::new();
    config_temp.pods.path = PathBuf::from("/pods");
    config_temp.recipes.path = temp_dir.clone();
    let broken_error = Recipe::inspect(&config_temp, "broken").unwrap_err();
    match broken_error {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::MissingPodRegistry);
            assert!(recipe_error.diagnostic.is_some());
        }
        other => panic!("expected MissingPodRegistry with diagnostic, got {other:?}"),
    }
    let _ = fs::remove_dir_all(&temp_dir);
}
