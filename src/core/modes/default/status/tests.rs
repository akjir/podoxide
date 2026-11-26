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

//! Unit tests for `pods status` target resolution, scanning, and table rendering in [`super`].

use std::collections::BTreeMap;
use std::fs as std_fs;

use crate::core::config::Config;
use crate::core::context::Context;
use crate::core::log::MAX_LOG_MESSAGE_BYTES;
use crate::error::{CliErrorKind, ConfigErrorKind, PodError, RecipeErrorKind};
use crate::io::system::fs::{self, TestTempDir};
use super::table::{format_table_row, parse_status_record, update_column_widths};
use super::{
    ExpectedContainer, MAX_STATUS_RECORDS, build_display_rows, collect_expected_containers,
    execute_with, handle,
};

const MOCK_PS_OUTPUT: &str = "\
a1b2c3d4e5f6\x1fnginx:alpine\x1fnginx -g 'daemon off;'\x1f2 hours ago\x1fUp 2 hours (healthy)\x1f0.0.0.0:8080->80/tcp\x1ftest_simple_cnt_pod-app-1\x1ftest_simple_cnt_pod\x1f0\x1e\n\
b2c3d4e5f6a1\x1falpine:latest\x1f/bin/sh\x1f5 mins ago\x1fUp 5 mins\x1f\x1ftest_pod-1\x1ftest_pod\x1f1\x1e\n\
c3d4e5f6a1b2\x1fbusybox\x1fsh\x1f1 day ago\x1fExited (0) 1 day ago\x1f\x1funmanaged_container\x1f\x1f0\x1e\n";

const NAMING_RECIPE_PSL: &str = "\
pod test_pod {
    name     \"Test Pod\"
    registry docker.io
}
container {
    image \"alpine:latest\"
}
container \"*my-db\" {
    image \"postgres:15\"
}
container totally_custom {
    image \"redis:alpine\"
}
";

const NUMERIC_RECIPE_PSL: &str = "\
pod num_pod {
    name     \"Numeric Pod\"
    registry docker.io
}
container 2 {
    image \"alpine:latest\"
}
container *db {
    image \"postgres:15\"
}
";

const SIMPLE_RECIPE_PSL: &str = "\
pod test_simple_cnt_pod {
    name     \"Simple Pod\"
    registry docker.io
}
container \"*app-1\" {
    image \"nginx:alpine\"
}
";

fn create_status_workspace(suffix: &str) -> (TestTempDir, Config) {
    let temp_dir = TestTempDir::new(&format!("status_test_{suffix}"));
    let pods_dir = temp_dir.join("pods");
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&pods_dir).unwrap();
    std_fs::create_dir_all(&recipes_dir).unwrap();

    fs::write_file(
        &recipes_dir.join("recipe_011_simple_container.pod"),
        SIMPLE_RECIPE_PSL,
    )
    .unwrap();
    fs::write_file(
        &recipes_dir.join("recipe_017_container_naming.pod"),
        NAMING_RECIPE_PSL,
    )
    .unwrap();
    fs::write_file(&recipes_dir.join("invalid_recipe.pod"), "invalid {{{ psl").unwrap();

    let mut config = Config::new();
    config.pods.path = pods_dir;
    config.recipes.path = recipes_dir;
    config.recipes.groups.insert(
        String::from("all"),
        vec![
            String::from("recipe_011_simple_container"),
            String::from("recipe_017_container_naming"),
            String::from("missing_recipe"),
            String::from("invalid_recipe"),
        ],
    );
    (temp_dir, config)
}

fn run_status(context: &Context, ps_output: &str) -> String {
    let mut capture = || Some(String::from(ps_output));
    let mut stdout = Vec::new();
    execute_with(context, &mut capture, &mut stdout).unwrap();
    String::from_utf8(stdout).unwrap()
}

fn single_expected_map(
    pod: &str,
    name: &str,
    target: &str,
) -> BTreeMap<String, BTreeMap<String, ExpectedContainer>> {
    BTreeMap::from([(
        String::from(pod),
        BTreeMap::from([(
            String::from(name),
            ExpectedContainer {
                target_name: String::from(target),
            },
        )]),
    )])
}

#[test]
fn build_display_rows_handles_newline_and_semicolons_in_command_without_row_forgery() {
    let output = "id1\x1fnginx:alpine\x1fsh -c 'a;;;b\nforged_id\x1fimg\x1fcmd\x1fnow\x1fUp\x1f\x1fforged\x1f\x1f0'\x1f2h ago\x1fUp 2h\x1f80/tcp\x1fmy_app\x1fmypod\x1f0\x1e\n";
    let empty = BTreeMap::new();
    let rows = build_display_rows(output, &empty, &[], &[], true);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "my_app");
    assert!(rows[0].command.contains("a;;;b\nforged_id"));
}

#[test]
fn execute_with_filters_explicit_and_numeric_container_targets() {
    let (_temp_dir, mut config) = create_status_workspace("container_targets");
    fs::write_file(
        &config.recipes.path.join("numeric_recipe.pod"),
        NUMERIC_RECIPE_PSL,
    )
    .unwrap();
    config.recipes.groups.insert(
        String::from("numeric"),
        vec![String::from("numeric_recipe")],
    );

    let mut context = Context::new();
    context.config = config;
    context.flags.set_full(true);
    context.parameters = vec![
        String::from("recipe_017_container_naming/*my-db"),
        String::from("recipe_017_container_naming/1"),
        String::from("recipe_017_container_naming/my-db"),
        String::from("recipe_011_simple_container/app-1"),
        String::from("numeric_recipe/1"),
    ];

    let mut capture = || Some(String::from(MOCK_PS_OUTPUT));
    let mut stdout = Vec::new();
    execute_with(&context, &mut capture, &mut stdout).unwrap();
    let output = String::from_utf8(stdout).unwrap();

    assert!(output.contains("TARGET NAME"));
    assert!(output.contains("recipe_017_container_naming/1"));
    assert!(output.contains("recipe_017_container_naming/2"));
    assert!(output.contains("recipe_011_simple_container/1"));
    assert!(output.contains("numeric_recipe/1"));
    assert!(output.contains("num_pod"));
    assert!(!output.contains("num_pod-db"));
    assert!(!output.contains("totally_custom"));
    assert!(!output.contains("recipe_017_container_naming/3"));

    context.parameters = vec![String::from("recipe_017_container_naming/nonexistent")];
    let mut never_capture = || -> Option<String> { panic!("podman ps should not be called") };
    let error = execute_with(&context, &mut never_capture, &mut Vec::new()).unwrap_err();
    match error {
        PodError::Recipe(recipe_error) => {
            assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotFound);
        }
        other => panic!("expected PodError::Recipe(ContainerNotFound), got {other:?}"),
    }

    for mixed in [
        vec![
            String::from("recipe_017_container_naming/1"),
            String::from("recipe_011_simple_container"),
        ],
        vec![
            String::from("recipe_011_simple_container"),
            String::from("recipe_017_container_naming/1"),
        ],
    ] {
        context.parameters = mixed;
        let mixed_error = execute_with(&context, &mut never_capture, &mut Vec::new()).unwrap_err();
        match mixed_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MixedTargets),
            other => panic!("expected PodError::Cli(MixedTargets), got {other:?}"),
        }
    }
}

#[test]
fn execute_with_handles_groups_empty_results_bounds_and_errors() {
    let (temp_dir, config) = create_status_workspace("bounds_and_errors");
    let mut context = Context::new();
    context.config = config;

    context.parameters = vec![String::from("unknown_recipe")];
    assert!(handle(&context).is_err());
    let mut called = false;
    let mut never_capture = || -> Option<String> {
        called = true;
        None
    };
    let target_error = execute_with(&context, &mut never_capture, &mut Vec::new()).unwrap_err();
    assert!(!called);
    match target_error {
        PodError::Config(config_error) => {
            assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
        }
        other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
    }

    context.parameters = vec![String::from("@all")];
    let mut group_stdout = Vec::new();
    let mut mock_capture = || Some(String::from(MOCK_PS_OUTPUT));
    execute_with(&context, &mut mock_capture, &mut group_stdout).unwrap();
    let group_output = String::from_utf8(group_stdout).unwrap();
    assert!(group_output.contains("test_simple_cnt_pod-app-1"));
    assert!(group_output.contains("test_pod-my-db"));
    assert!(group_output.contains("invalid_recipe"));
    assert!(group_output.contains("Recipe Error"));

    // Invalid recipe renders Recipe Error row and singular inspect hint.
    context.parameters = vec![String::from("invalid_recipe")];
    let mut invalid_stdout = Vec::new();
    execute_with(&context, &mut mock_capture, &mut invalid_stdout).unwrap();
    let invalid_output = String::from_utf8(invalid_stdout).unwrap();
    assert!(invalid_output.contains("invalid_recipe   Recipe Error"));
    assert!(invalid_output.contains(
        "\nRecipe 'invalid_recipe' has errors! Run 'pods recipe inspect invalid_recipe' for details.\n"
    ));

    // Missing .pod file renders Not Found row without a `pods recipe inspect` hint.
    context.parameters = vec![String::from("missing_recipe")];
    let mut missing_stdout = Vec::new();
    execute_with(&context, &mut mock_capture, &mut missing_stdout).unwrap();
    let missing_output = String::from_utf8(missing_stdout).unwrap();
    assert!(missing_output.contains("missing_recipe   Not Found"));
    assert!(!missing_output.contains("Recipe Error"));
    assert!(!missing_output.contains("pods recipe inspect"));

    let mut fail_capture = || -> Option<String> { None };
    let ps_error = execute_with(&context, &mut fail_capture, &mut Vec::new()).unwrap_err();
    assert!(
        ps_error
            .to_string()
            .contains("failed to query Podman status")
    );

    let mut empty_context = Context::new();
    empty_context.config.pods.path = temp_dir.join("pods");
    empty_context.config.recipes.path = temp_dir.join("recipes");
    let mut unmanaged_only_capture = || {
        Some(String::from(
            "c3d4e5f6a1b2\x1fbusybox\x1fsh\x1f1 day ago\x1fExited (0) 1 day ago\x1f\x1funmanaged\x1f\x1f0\x1e\n",
        ))
    };
    let mut empty_stdout = Vec::new();
    execute_with(
        &empty_context,
        &mut unmanaged_only_capture,
        &mut empty_stdout,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(empty_stdout).unwrap(),
        "No containers found.\n"
    );

    assert!(parse_status_record("").is_none());
    assert!(parse_status_record("too\x1ffew\x1ffields").is_none());
    assert!(
        parse_status_record("\x1fimg\x1f\"sh\"\x1fnow\x1fUp\x1f\x1fname\x1fpod\x1f0").is_none()
    );
    assert!(parse_status_record("id\x1fimg\x1f\"sh\"\x1fnow\x1fUp\x1f\x1f\x1fpod\x1f0").is_none());
    let json_row = parse_status_record(
        "id\x1fimg\x1f\"sh -c \\\"a\\u001eb\\u001fc\\u003c\\u003e\\u0026\\uD83D\\uDE00\\\"\"\x1fcreated\x1fstatus\x1fports\x1fname\x1fpod\x1f3",
    )
    .unwrap();
    assert_eq!(json_row.command, "sh -c \"a\x1eb\x1fc<>&\u{1F600}\"");
    assert_eq!(json_row.created, "created");
    assert_eq!(json_row.restarts, "3");
    assert_eq!(
        parse_status_record("id\x1fimg\x1f\"\"\x1fnow\x1fUp\x1f\x1fname\x1f\x1f0")
            .unwrap()
            .command,
        ""
    );
    assert_eq!(
        parse_status_record("id\x1fimg\x1f\"bad\\q\"\x1fnow\x1fUp\x1f\x1fname\x1f\x1f0")
            .unwrap()
            .command,
        "\"bad\\q\""
    );
    let duplicate_ps = "\
1111\x1fimg\x1f\"sh\"\x1fnow\x1fUp\x1f\x1fcnt\x1fpod\x1f0\x1e\n\
2222\x1fimg\x1f\"sh\"\x1fnow\x1fUp\x1f\x1fcnt\x1fpod\x1f0\x1e\n";
    let dup_expected = single_expected_map("pod", "cnt", "r/1");
    assert_eq!(
        build_display_rows(duplicate_ps, &dup_expected, &[], &[], false).len(),
        1
    );

    // Scans past 1024 ps records to find managed containers while capping displayed rows at MAX_STATUS_RECORDS.
    let mut oversized_ps = String::new();
    let mut oversized_pod = BTreeMap::new();
    for index in 0..(MAX_STATUS_RECORDS + 10) {
        oversized_ps.push_str(&format!(
            "id_{index}\x1fimg\x1fcmd\x1fnow\x1fUp 1h\x1f80\x1fcnt_{index}\x1fpod\x1f0\x1e\n"
        ));
        oversized_pod.insert(
            format!("missing_{index}"),
            ExpectedContainer {
                target_name: format!("recipe/{}", index + 1),
            },
        );
    }
    let oversized_expected = BTreeMap::from([(String::from("pod"), oversized_pod)]);
    let empty_expected = BTreeMap::new();
    let bounded_ps_rows = build_display_rows(&oversized_ps, &empty_expected, &[], &[], true);
    assert_eq!(bounded_ps_rows.len(), MAX_STATUS_RECORDS);
    let bounded_missing_rows = build_display_rows("", &oversized_expected, &[], &[], false);
    assert_eq!(bounded_missing_rows.len(), MAX_STATUS_RECORDS);

    let single_expected =
        single_expected_map("pod", &format!("cnt_{MAX_STATUS_RECORDS}"), "recipe/1");
    let found_after_1024 = build_display_rows(&oversized_ps, &single_expected, &[], &[], false);
    assert_eq!(found_after_1024.len(), 1);
    assert_eq!(found_after_1024[0].status, "Up 1h");

    let oversized_cell = "x".repeat(MAX_LOG_MESSAGE_BYTES + 100);
    let mut widths = [0usize; 1];
    update_column_widths(&mut widths, &[&oversized_cell]);
    assert_eq!(widths[0], MAX_LOG_MESSAGE_BYTES);
    let clamped_cell_row = format_table_row(&[&oversized_cell], &widths);
    assert_eq!(clamped_cell_row.len(), MAX_LOG_MESSAGE_BYTES);
    let clamped_padded_row = format_table_row(&["a", "b"], &[usize::MAX, 1]);
    assert_eq!(clamped_padded_row.len(), MAX_LOG_MESSAGE_BYTES + 4);
    assert_eq!(
        format_table_row(&["ID", "IMAGE"], &[14, 12]),
        "ID               IMAGE"
    );
    assert_eq!(
        format_table_row(&["id", "cmd", ""], &[4, 10, 5]),
        "id     cmd"
    );
    assert_eq!(
        format_table_row(&["id", "cmd\u{202E}", "\x1f"], &[4, 10, 5]),
        "id     cmd"
    );

    let empty_targets = collect_expected_containers(&context.config, &[]);
    assert!(empty_targets.expected.is_empty());
    assert!(empty_targets.invalid_recipes.is_empty());
    assert!(empty_targets.missing_recipes.is_empty());
}

#[test]
fn execute_with_renders_filtered_and_full_status_tables_without_trailing_spaces() {
    let (_temp_dir, config) = create_status_workspace("explicit_target");
    let mut context = Context::new();
    context.config = config;
    context.parameters = vec![String::from("recipe_011_simple_container")];

    let mut query_count = 0usize;
    let mut capture = || -> Option<String> {
        query_count += 1;
        Some(String::from(MOCK_PS_OUTPUT))
    };

    let mut stdout = Vec::new();
    execute_with(&context, &mut capture, &mut stdout).unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let expected_header = "ID             POD                   NAME                        STATUS                 RESTARTS   CREATED       IMAGE\n";
    let expected_row = "a1b2c3d4e5f6   test_simple_cnt_pod   test_simple_cnt_pod-app-1   Up 2 hours (healthy)   0          2 hours ago   nginx:alpine\n";
    assert_eq!(output, format!("{expected_header}{expected_row}"));

    context.flags.set_full(true);
    let mut full_stdout = Vec::new();
    execute_with(&context, &mut capture, &mut full_stdout).unwrap();
    assert_eq!(query_count, 2);
    let full_output = String::from_utf8(full_stdout).unwrap();
    let expected_full_header = "ID             POD                   NAME                        TARGET NAME                     STATUS                 RESTARTS   CREATED       IMAGE          COMMAND                  PORTS\n";
    let expected_full_row = "a1b2c3d4e5f6   test_simple_cnt_pod   test_simple_cnt_pod-app-1   recipe_011_simple_container/1   Up 2 hours (healthy)   0          2 hours ago   nginx:alpine   nginx -g 'daemon off;'   0.0.0.0:8080->80/tcp\n";
    assert_eq!(
        full_output,
        format!("{expected_full_header}{expected_full_row}")
    );
}

#[test]
fn execute_with_synthesizes_not_found_and_recipe_error_rows_with_all_and_full_flags() {
    let (_temp_dir, config) = create_status_workspace("missing_and_all");
    let mut context = Context::new();
    context.config = config;
    context.parameters = vec![String::from("recipe_017_container_naming")];

    let output = run_status(&context, MOCK_PS_OUTPUT);

    let expected_table = "\
ID             POD        NAME             STATUS      RESTARTS   CREATED      IMAGE\n\
b2c3d4e5f6a1   test_pod   test_pod-1       Up 5 mins   1          5 mins ago   alpine:latest\n\
-              test_pod   test_pod-my-db   Not Found   -          -            -\n\
-              test_pod   totally_custom   Not Found   -          -            -\n";
    assert_eq!(output, expected_table);

    context.parameters.clear();
    context.flags.set_all(true);
    let all_output = run_status(&context, MOCK_PS_OUTPUT);

    let expected_all_table = "\
ID             POD                   NAME                        STATUS                 RESTARTS   CREATED       IMAGE\n\
c3d4e5f6a1b2                         unmanaged_container         Exited (0) 1 day ago   0          1 day ago     busybox\n\
-              -                     invalid_recipe              Recipe Error           -          -             -\n\
-              -                     missing_recipe              Not Found              -          -             -\n\
b2c3d4e5f6a1   test_pod              test_pod-1                  Up 5 mins              1          5 mins ago    alpine:latest\n\
-              test_pod              test_pod-my-db              Not Found              -          -             -\n\
-              test_pod              totally_custom              Not Found              -          -             -\n\
a1b2c3d4e5f6   test_simple_cnt_pod   test_simple_cnt_pod-app-1   Up 2 hours (healthy)   0          2 hours ago   nginx:alpine\n\
\n\
Recipe 'invalid_recipe' has errors! Run 'pods recipe inspect invalid_recipe' for details.\n";
    assert_eq!(all_output, expected_all_table);

    context.flags.set_full(true);
    let all_full_output = run_status(&context, MOCK_PS_OUTPUT);
    assert!(all_full_output.contains("unmanaged_container         -"));
    assert!(all_full_output.contains("invalid_recipe              invalid_recipe"));
    assert!(all_full_output.contains("missing_recipe              missing_recipe"));
    assert!(all_full_output.contains("test_pod-1                  recipe_017_container_naming/1"));
    assert!(all_full_output.contains("test_pod-my-db              recipe_017_container_naming/2"));
    assert!(all_full_output.contains("totally_custom              recipe_017_container_naming/3"));
    assert!(all_full_output.contains("test_simple_cnt_pod-app-1   recipe_011_simple_container/1"));
    assert!(!all_full_output.lines().any(|line| line.ends_with(' ')));

    context.flags.set_full(false);
    let empty_image_output = run_status(
        &context,
        "d4e5f6a1b2c3\x1f\x1fsh\x1f1 day ago\x1fExited (0) 1 day ago\x1f\x1fno_image_cnt\x1f\x1f0\x1e\n",
    );
    assert!(empty_image_output.contains("no_image_cnt"));
    assert!(!empty_image_output.lines().any(|line| line.ends_with(' ')));
}

#[test]
fn execute_with_keeps_same_container_name_distinct_across_pods() {
    let temp_dir = TestTempDir::new("status_same_name_pods");
    let pods_dir = temp_dir.join("pods");
    let recipes_dir = temp_dir.join("recipes");
    std_fs::create_dir_all(&pods_dir).unwrap();
    std_fs::create_dir_all(&recipes_dir).unwrap();

    fs::write_file(
        &recipes_dir.join("dev.pod"),
        "pod dev_pod {\n    name \"Dev\"\n    registry docker.io\n}\ncontainer redis {\n    image redis:7\n}\n",
    )
    .unwrap();
    fs::write_file(
        &recipes_dir.join("staging.pod"),
        "pod staging_pod {\n    name \"Staging\"\n    registry docker.io\n}\ncontainer redis {\n    image redis:7\n}\n",
    )
    .unwrap();

    let mut context = Context::new();
    context.config.pods.path = pods_dir;
    context.config.recipes.path = recipes_dir;
    context.config.recipes.groups.insert(
        String::from("all"),
        vec![String::from("dev"), String::from("staging")],
    );
    context.flags.set_full(true);

    let output = run_status(
        &context,
        "1111aaaa2222\x1fredis:7\x1fredis-server\x1f1h ago\x1fUp 1h\x1f6379/tcp\x1fredis\x1fdev_pod\x1f0\x1e\n",
    );
    assert!(output.contains("dev_pod       redis   dev/1         Up 1h"));
    assert!(output.contains("staging_pod   redis   staging/1     Not Found"));
}
