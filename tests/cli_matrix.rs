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

//! Isolated CLI output-matrix integration tests for missing configs, simulate modes, flag overrides, and argument validation.

#![forbid(unsafe_code)]

mod common;

use std::fs;

use self::common::TestWorkspace;

const DANGLING_IMAGE_RECORD: &str =
    "a1b2c3d4e5f6\x1fsha256:dangling0001\x1f<none>\x1f<none>\x1f12.5 MB\x1f2 hours ago\x1e\n";

fn assert_debug_custom_config_outputs(workspace: &TestWorkspace) {
    let custom_conf = workspace.dir().join("custom.conf");
    fs::write(
        &custom_conf,
        "general {\n    editor vim\n    simulate false\n}\nrecipes {\n    groups {\n        all recipe\n    }\n}\n",
    )
    .unwrap();

    let expected_debug_prefix = "DEBUG: Debug mode is enabled.\nDEBUG: Config './custom.conf' is used.\nDEBUG: Image mode is used.\n";

    let empty_prune = workspace.run_pods(&["--debug", "--config=custom", "image", "prune"], b"");
    assert!(empty_prune.success, "stderr: {}", empty_prune.stderr);
    assert_eq!(empty_prune.stderr, expected_debug_prefix);
    assert_eq!(empty_prune.stdout, "No images found to prune.\n");

    let declined_prune = workspace.run_pods_with_env(
        &["--debug", "--config=custom", "image", "prune"],
        b"no\n",
        &[("MOCK_PODMAN_IMAGES", DANGLING_IMAGE_RECORD)],
    );
    assert!(!declined_prune.success);
    assert!(declined_prune.stdout.is_empty());
    let stderr = &declined_prune.stderr;
    assert!(stderr.starts_with(expected_debug_prefix));
    assert!(stderr.contains("Are you sure you want to prune "));
    assert!(stderr.ends_with("? Type 'yes' to proceed: \nERROR: Aborting execution.\n"));

    fs::write(
        &custom_conf,
        "general {\n    editor vim\n    simulate true\n}\nrecipes {\n    groups {\n        all recipe\n    }\n}\n",
    )
    .unwrap();

    let simulated_prune =
        workspace.run_pods(&["--debug", "--config=custom", "image", "prune"], b"");
    assert!(simulated_prune.success);
    assert_eq!(
        simulated_prune.stderr,
        "DEBUG: Debug mode is enabled.\nDEBUG: Config './custom.conf' is used.\nINFO: Simulate mode is active.\nDEBUG: Image mode is used.\n"
    );
    assert_eq!(
        simulated_prune.stdout,
        "Prune images:\npodman image prune -f\n"
    );
}

fn assert_stdout_matrix(
    workspace: &TestWorkspace,
    cases: &[(&[&str], &str)],
    expected_stderr: &str,
) {
    for (arguments, expected_stdout) in cases {
        let output = workspace.run_pods(arguments, b"");
        assert!(
            output.success,
            "expected {arguments:?} to succeed: {}",
            output.stderr
        );
        assert_eq!(
            output.stderr, expected_stderr,
            "unexpected stderr for {arguments:?}"
        );
        assert!(
            output.stdout.contains(expected_stdout),
            "expected {expected_stdout:?} in stdout for {arguments:?}, got {:?}",
            output.stdout
        );
    }
}

#[test]
fn missing_config_bypasses_offline_actions_and_errors_on_operational_actions() {
    let workspace = TestWorkspace::new("missing_config");

    let bypass_cases: &[(&[&str], &str)] = &[
        (&[], "Usage: pods [MODE] [OPTIONS] ACTION [TARGETS]"),
        (&["help"], "Usage: pods [MODE] [OPTIONS] ACTION [TARGETS]"),
        (&["version"], "Podoxide v"),
        (&["cmd"], "Command Mode"),
        (&["cmd", "help"], "Command Mode"),
        (&["cfg", "help"], "Usage: pods config [OPTIONS] [ACTION]"),
        (&["con"], "Connect Mode"),
        (&["con", "help"], "Connect Mode"),
        (&["img"], "Image Mode"),
        (&["img", "help"], "Image Mode"),
        (&["init", "help"], "Usage: pods init [OPTIONS] [ACTION]"),
        (&["log"], "Logs Mode"),
        (&["log", "help"], "Logs Mode"),
        (
            &["rec", "help"],
            "Usage: pods recipe [OPTIONS] [ACTION] [NAME]",
        ),
    ];

    assert_stdout_matrix(&workspace, bypass_cases, "");

    let config_required_cases: &[&[&str]] = &[
        &["status"],
        &["up", "recipe"],
        &["down", "recipe"],
        &["create", "recipe"],
        &["remove", "recipe"],
        &["start", "recipe"],
        &["stop", "recipe"],
        &["restart", "recipe"],
        &["reload", "recipe"],
        &["update", "recipe"],
        &["cmd", "list", "recipe"],
        &["cmd", "exec", "recipe", "migrate"],
        &["cfg"],
        &["cfg", "inspect"],
        &["cfg", "show"],
        &["cfg", "edit"],
        &["con", "recipe/db"],
        &["img", "prune"],
        &["img", "prune", "--preview"],
        &["img", "update", "recipe"],
        &["log", "recipe"],
        &["log", "follow", "recipe"],
        &["rec"],
        &["rec", "list"],
        &["rec", "inspect", "recipe"],
        &["rec", "show", "recipe"],
        &["rec", "edit", "recipe"],
    ];

    for arguments in config_required_cases {
        let output = workspace.run_pods(arguments, b"");
        assert!(
            !output.success,
            "expected {arguments:?} to fail when pods.conf is missing"
        );
        assert!(
            output.stdout.is_empty(),
            "expected empty stdout for failed {arguments:?}, got {:?}",
            output.stdout
        );
        assert_eq!(
            output.stderr, "ERROR: Config error: configuration file not found: './pods.conf'\n",
            "unexpected stderr for {arguments:?}"
        );
    }
}

#[test]
fn pipeline_handles_valid_and_invalid_arguments_and_podman_version_checks() {
    let workspace = TestWorkspace::new("pipeline");
    assert!(workspace.run_pods(&["init"], b"").success);

    let success_cases: &[&[&str]] = &[
        &[],
        &["help"],
        &["version"],
        &["--debug", "--config=pods", "up", "recipe"],
        &["up", "recipe"],
        &["up", "@all"],
        &["up", "recipe/db", "recipe/2"],
        &["cmd", "list", "recipe"],
        &["cfg", "inspect"],
        &["cfg", "show"],
        &["con", "recipe/db"],
        &["img", "prune"],
        &["img", "update", "recipe"],
        &["img", "update", "recipe/db"],
        &["init", "help"],
        &["init", "--simulate"],
        &["init"],
        &["log", "recipe"],
        &["rec", "list"],
        &["rec", "inspect", "recipe"],
    ];
    for arguments in success_cases {
        let output = workspace.run_pods(arguments, b"");
        assert!(
            output.success,
            "expected {arguments:?} to succeed, stderr: {}",
            output.stderr
        );
    }

    let failure_cases: &[&[&str]] = &[
        &["--debug", "--config=staging", "up", "recipe"],
        &["up", "recipe", "recipe/db"],
        &["up", "example-app"],
        &["con", "recipe"],
        &["init", "--config=staging"],
        &["--unknown-flag"],
        &["ini"],
        &["up"],
    ];
    for arguments in failure_cases {
        let output = workspace.run_pods(arguments, b"");
        assert!(!output.success, "expected {arguments:?} to fail");
        assert!(
            output.stderr.contains("ERROR: "),
            "expected ERROR line for {arguments:?}, got {:?}",
            output.stderr
        );
    }

    // Unsupported Podman < 5.8.0 rejects Podman-backed actions while offline actions still succeed.
    for operational in [
        &["status"][..],
        &["up", "recipe"][..],
        &["cmd", "exec", "recipe", "migrate"][..],
    ] {
        let output =
            workspace.run_pods_with_env(operational, b"", &[("MOCK_PODMAN_VERSION", "5.7.9")]);
        assert!(!output.success);
        assert_eq!(
            output.stderr,
            "ERROR: Podman 5.8.0 or higher is required.\n"
        );
    }
    for bypass in [
        &["help"][..],
        &["version"][..],
        &["init", "--simulate"][..],
        &["cfg", "show"][..],
        &["cfg", "inspect"][..],
        &["rec", "list"][..],
        &["cmd", "list", "recipe"][..],
    ] {
        let output = workspace.run_pods_with_env(bypass, b"", &[("MOCK_PODMAN_VERSION", "5.7.9")]);
        assert!(
            output.success,
            "expected {bypass:?} to bypass Podman version check: {}",
            output.stderr
        );
    }
}

#[test]
fn simulate_false_and_flag_override_matrix_runs_deterministically() {
    let workspace = TestWorkspace::new("simulate_false");
    assert!(workspace.run_pods(&["init"], b"").success);

    fs::write(
        workspace.dir().join("pods.conf"),
        "general {\n    editor true\n    simulate false\n}\npods {\n    path ./pods\n}\nrecipes {\n    path .\n    groups {\n        all recipe\n    }\n}\n",
    )
    .unwrap();

    for prune_args in [&["image", "prune"][..], &["image", "prune", "--all"][..]] {
        let empty = workspace.run_pods(prune_args, b"");
        assert!(empty.success, "stderr: {}", empty.stderr);
        assert!(empty.stderr.is_empty());
        assert_eq!(empty.stdout, "No images found to prune.\n");

        let declined = workspace.run_pods_with_env(
            prune_args,
            b"no\n",
            &[("MOCK_PODMAN_IMAGES", DANGLING_IMAGE_RECORD)],
        );
        assert!(!declined.success);
        assert!(declined.stdout.is_empty());
        let stderr = &declined.stderr;
        assert!(stderr.starts_with("Are you sure you want to prune "));
        assert!(stderr.ends_with("? Type 'yes' to proceed: \nERROR: Aborting execution.\n"));
    }

    for (arguments, expected_stdout) in [
        (&["cfg", "inspect"][..], "  Simulate:     false\n"),
        (&["cfg", "show"][..], "  1: general {\n"),
        (&["cfg", "edit"][..], ""),
        (&["rec", "list"][..], "Recipes:\n"),
        (
            &["rec", "inspect", "recipe"][..],
            "Recipe 'recipe' is valid.\n",
        ),
        (&["rec", "show", "recipe"][..], "  6: pod example-stack {\n"),
        (&["rec", "edit", "recipe"][..], ""),
        (&["status"][..], "example-stack-app"),
    ] {
        let output = workspace.run_pods(arguments, b"");
        assert!(
            output.success,
            "expected {arguments:?} to succeed: {}",
            output.stderr
        );
        assert!(
            output.stderr.is_empty(),
            "expected empty stderr for {arguments:?}, got {:?}",
            output.stderr
        );
        assert!(
            output.stdout.contains(expected_stdout),
            "expected {expected_stdout:?} in stdout for {arguments:?}, got {:?}",
            output.stdout
        );
    }

    let flag_override_cases: &[(&[&str], &str)] = &[
        (
            &["--simulate", "image", "prune"],
            "Prune images:\npodman image prune -f\n",
        ),
        (
            &["--simulate", "image", "prune", "--all"],
            "Prune images:\npodman image prune -f -a\n",
        ),
        (&["--simulate", "cfg", "edit"], "true ./pods.conf\n"),
        (
            &["--simulate", "rec", "edit", "recipe"],
            "true ./recipe.pod\n",
        ),
        (
            &["--simulate", "up", "recipe"],
            "podman pod create --name example-stack --publish 8080:80/TCP --userns=keep-id\n",
        ),
        (
            &["--simulate", "cmd", "exec", "recipe", "migrate"],
            "podman exec -it -u dbuser example-stack-db db-cli -U dbuser -d app -f /migrations/run.sql\n",
        ),
        (
            &["--simulate", "con", "recipe/db"],
            "podman exec -it example-stack-db sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
        (
            &["--simulate", "log", "show", "recipe"],
            "Execute log command:\npodman pod logs -n --color example-stack\n",
        ),
        (
            &["--simulate", "img", "update", "recipe"],
            "podman pull docker.io/example/db-server:latest\n",
        ),
    ];

    assert_stdout_matrix(
        &workspace,
        flag_override_cases,
        "INFO: Simulate mode is active.\n",
    );

    assert_stdout_matrix(
        &workspace,
        &[
            (&["--simulate", "cfg", "inspect"], "  Simulate:     true\n"),
            (&["--simulate", "cfg", "show"], "  1: general {\n"),
            (
                &["--simulate", "img", "prune", "--preview"],
                "No images found to prune.\n",
            ),
        ],
        "",
    );

    assert_debug_custom_config_outputs(&workspace);
}

#[test]
fn simulate_true_matrix_verifies_init_and_all_modes() {
    let workspace = TestWorkspace::new("simulate_true");

    let sim_init = workspace.run_pods(&["init", "--simulate"], b"");
    assert!(
        sim_init.success,
        "init --simulate failed: {}",
        sim_init.stderr
    );
    assert!(sim_init.stdout.is_empty());
    assert!(sim_init.stderr.contains("INFO: Simulate mode is active.\n"));
    assert!(sim_init.stderr.contains("INFO: Created 'recipe.pod'.\n"));
    assert!(sim_init.stderr.contains("INFO: Created 'pods.conf'.\n"));
    assert!(!workspace.dir().join("recipe.pod").exists());
    assert!(!workspace.dir().join("pods.conf").exists());

    let init = workspace.run_pods(&["init"], b"");
    assert!(init.success, "init failed: {}", init.stderr);
    assert!(init.stdout.is_empty());
    assert!(init.stderr.contains("INFO: Created 'recipe.pod'.\n"));
    assert!(init.stderr.contains("INFO: Created 'pods.conf'.\n"));
    assert!(!init.stderr.contains("Simulate mode is active."));

    let simulate_true_cases: &[(&[&str], &str)] = &[
        (
            &["up", "recipe"],
            "podman pod create --name example-stack --publish 8080:80/TCP --userns=keep-id\n",
        ),
        (&["down", "recipe"], "podman pod rm example-stack\n"),
        (
            &["create", "recipe"],
            "podman create --name example-stack-db --pod example-stack",
        ),
        (
            &["create", "recipe/db"],
            "podman pod create --name example-stack --publish 8080:80/TCP --userns=keep-id\nCreate container 'example-stack-db':\npodman create --name example-stack-db --pod example-stack",
        ),
        (&["remove", "recipe"], "podman rm example-stack-db\n"),
        (&["start", "recipe"], "podman start example-stack-db\n"),
        (&["stop", "recipe"], "podman pod stop example-stack\n"),
        (&["stop", "recipe/db"], "podman stop example-stack-db\n"),
        (&["stop", "recipe/1"], "podman stop example-stack-db\n"),
        (&["stop", "recipe/*app"], "podman stop example-stack-app\n"),
        (
            &["stop", "recipe/example-stack-app"],
            "podman stop example-stack-app\n",
        ),
        (&["restart", "recipe"], "podman start example-stack-app\n"),
        (&["reload", "recipe"], "podman pod rm example-stack\n"),
        (
            &["update", "recipe"],
            "Update Summary:\n  Simulated (1):\n    - example-stack (Example Web Stack)\n\nSimulated 1 pod (2 containers).\n",
        ),
        (
            &["update", "recipe/db"],
            "Update Summary:\n  Simulated (1):\n    - example-stack-db (docker.io/example/db-server:latest)\n\nSimulated 1 container.\n",
        ),
        (
            &["cmd", "exec", "recipe", "migrate"],
            "podman exec -it -u dbuser example-stack-db db-cli -U dbuser -d app -f /migrations/run.sql\n",
        ),
        (&["cfg", "edit"], "vim ./pods.conf\n"),
        (
            &["con", "recipe/db"],
            "podman exec -it example-stack-db sh -c 'if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh'\n",
        ),
        (&["img", "prune"], "Prune images:\npodman image prune -f\n"),
        (
            &["img", "prune", "--all"],
            "Prune images:\npodman image prune -f -a\n",
        ),
        (
            &["img", "update", "recipe"],
            "Update Summary:\n  Simulated (1):\n    - example-stack (Example Web Stack)\n\nSimulated 1 pod (2 images).\n",
        ),
        (
            &["img", "update", "recipe/db"],
            "Update Summary:\n  Simulated (1):\n    - example-stack-db (docker.io/example/db-server:latest)\n\nSimulated 1 image.\n",
        ),
        (
            &["log", "show", "recipe"],
            "Execute log command:\npodman pod logs -n --color example-stack\n",
        ),
        (
            &["log", "follow", "recipe/db"],
            "Execute log command:\npodman pod logs -n --color -f -c example-stack-db example-stack\n",
        ),
        (&["rec", "edit", "recipe"], "vim ./recipe.pod\n"),
    ];

    assert_stdout_matrix(
        &workspace,
        simulate_true_cases,
        "INFO: Simulate mode is active.\n",
    );

    assert_stdout_matrix(
        &workspace,
        &[
            (&["cmd", "list", "recipe"], "Commands for recipe 'recipe':"),
            (&["cfg", "inspect"], "  Simulate:     true\n"),
            (&["cfg", "show"], "  5: general {\n"),
            (&["img", "prune", "--preview"], "No images found to prune."),
            (&["status"], "example-stack-app"),
            (&["rec", "list"], "Recipes:\n"),
            (&["rec", "inspect", "recipe"], "Recipe 'recipe' is valid.\n"),
            (&["rec", "show", "recipe"], "  6: pod example-stack {\n"),
        ],
        "",
    );
}
