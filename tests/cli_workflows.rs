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

//! End-to-end CLI workflow integration tests covering installed symlinks, custom configs, non-zero Podman exits, shared-image updates, and image pruning.

#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::os::unix::fs as unix_fs;

use self::common::TestWorkspace;

const ALPHA_RECIPE: &str = "\
pod alpha {
    name \"Alpha Stack\"
    registry docker.io
}
container *app {
    image example/shared:latest
}
";

const BETA_RECIPE: &str = "\
pod beta {
    name \"Beta Stack\"
    registry docker.io
}
container *worker {
    image example/shared:latest
}
";

const GAMMA_RECIPE: &str = "\
pod gamma {
    name \"Gamma Stack\"
    registry docker.io
}
container *static {
    image example/static:latest
}
";

const LIVE_CONFIG: &str = "\
general {
    editor true
    simulate false
}
pods {
    path ./pods
}
recipes {
    path .
    groups {
        all alpha beta gamma
    }
}
";

const PRUNABLE_IMAGES_LIST: &str = "\
a1b2c3d4e5f6\x1fsha256:dangling0001\x1f<none>\x1f<none>\x1f10 MB\x1f2 hours ago\x1e\n\
b2c3d4e5f6a1\x1fsha256:unused0002\x1fdocker.io/library/alpine\x1flatest\x1f5 MB\x1f1 day ago\x1e\n";

#[test]
fn binary_in_usr_local_bin_uses_current_working_directory() {
    let workspace = TestWorkspace::new("symlink_cwd");
    let root_dir = workspace.dir().parent().unwrap();
    let bin_dir = root_dir.join("usr").join("local").join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let installed_pods = bin_dir.join("pods");
    unix_fs::symlink(env!("CARGO_BIN_EXE_pods"), &installed_pods).unwrap();

    let missing_status = workspace.run_binary_with_env(&installed_pods, &["status"], b"", &[]);
    assert!(!missing_status.success);
    assert_eq!(
        missing_status.stderr,
        "ERROR: Config error: configuration file not found: './pods.conf'\n"
    );

    let first_init = workspace.run_binary_with_env(&installed_pods, &["init"], b"", &[]);
    assert!(
        first_init.success,
        "first init failed: {}",
        first_init.stderr
    );
    let canonical_working_dir = fs::canonicalize(workspace.dir()).unwrap();
    assert!(first_init.stderr.contains(&format!(
        "INFO: Initializing workspace '{}'.",
        canonical_working_dir.display()
    )));
    assert!(first_init.stderr.contains("INFO: Created 'recipe.pod'."));
    assert!(first_init.stderr.contains("INFO: Created 'pods.conf'."));

    let second_init = workspace.run_binary_with_env(&installed_pods, &["init"], b"", &[]);
    assert!(second_init.success);
    assert!(
        second_init
            .stderr
            .contains("WARNING: File 'recipe.pod' already exists.")
    );
    assert!(
        second_init
            .stderr
            .contains("WARNING: File 'pods.conf' already exists.")
    );

    let status = workspace.run_binary_with_env(&installed_pods, &["status"], b"", &[]);
    assert!(status.success, "status failed: {}", status.stderr);
    assert!(status.stdout.contains("example-stack-db"));

    let up = workspace.run_binary_with_env(&installed_pods, &["up", "recipe"], b"", &[]);
    assert!(up.success, "up failed: {}", up.stderr);
    assert!(
        up.stdout.contains(
            "podman pod create --name example-stack --publish 8080:80/TCP --userns=keep-id"
        )
    );
    assert!(up.stdout.contains("podman start example-stack-db"));
    assert!(up.stdout.contains("podman start example-stack-app"));

    assert!(
        workspace
            .run_binary_with_env(&installed_pods, &["cfg", "show"], b"", &[])
            .success
    );
    assert!(
        workspace
            .run_binary_with_env(&installed_pods, &["rec", "list"], b"", &[])
            .success
    );

    assert!(workspace.dir().join("recipe.pod").is_file());
    assert!(workspace.dir().join("pods.conf").is_file());
    assert!(!bin_dir.join("recipe.pod").exists());
    assert!(!bin_dir.join("pods.conf").exists());
}

#[test]
fn custom_pods_conf_logs_debug_path_and_broken_config_shows_lines() {
    let workspace = TestWorkspace::new("custom_conf");
    let sub_dir = workspace.dir().join("nested");
    fs::create_dir_all(&sub_dir).unwrap();

    let nested_conf = sub_dir.join("pods.conf");
    fs::write(&nested_conf, "broken {{{ syntax\nline two\n").unwrap();
    let config_arg = format!("--config={}", nested_conf.display());

    let show = workspace.run_pods(&["--debug", &config_arg, "cfg", "show"], b"");
    assert!(
        show.success,
        "expected cfg show to succeed on broken pods.conf: {}",
        show.stderr
    );
    assert!(show.stdout.contains("  1: broken {{{ syntax\n"));
    assert!(show.stdout.contains("  2: line two\n"));
    assert!(
        show.stderr.contains(&format!(
            "DEBUG: Config '{}' is used.\n",
            nested_conf.display()
        )),
        "expected debug log for custom pods.conf path, got: {:?}",
        show.stderr
    );
}

#[test]
fn live_image_prune_handles_preview_abort_confirm_and_podman_failure() {
    let workspace = TestWorkspace::new("live_prune");
    fs::write(workspace.dir().join("pods.conf"), LIVE_CONFIG).unwrap();
    let log_file = workspace.dir().join("podman.log");
    let log_path = log_file.to_string_lossy().into_owned();

    // 1. Preview mode lists prunable images and reclaimable space without executing `image prune`.
    let preview = workspace.run_pods_with_env(
        &["image", "prune", "--all", "--preview"],
        b"",
        &[
            ("MOCK_PODMAN_IMAGES", PRUNABLE_IMAGES_LIST),
            ("MOCK_PODMAN_LOG", &log_path),
        ],
    );
    assert!(preview.success, "preview stderr: {}", preview.stderr);
    assert!(preview.stderr.is_empty());
    assert!(preview.stdout.contains("docker.io/library/alpine"));
    assert!(preview.stdout.contains("Total space reclaimable:"));
    let preview_log = fs::read_to_string(&log_file).unwrap();
    assert!(
        !preview_log.contains("image prune"),
        "preview must not invoke podman image prune: {preview_log}"
    );

    // 2. Declining or EOF aborts cleanly on stderr without invoking `podman image prune`.
    fs::write(&log_file, "").unwrap();
    for stdin_input in [&b"no\n"[..], &b""[..]] {
        let aborted = workspace.run_pods_with_env(
            &["image", "prune"],
            stdin_input,
            &[
                ("MOCK_PODMAN_IMAGES", PRUNABLE_IMAGES_LIST),
                ("MOCK_PODMAN_LOG", &log_path),
            ],
        );
        assert!(!aborted.success);
        assert!(aborted.stdout.is_empty());
        assert!(aborted.stderr.contains("Are you sure you want to prune "));
        assert!(
            aborted
                .stderr
                .ends_with("? Type 'yes' to proceed: \nERROR: Aborting execution.\n")
        );
    }
    let abort_log = fs::read_to_string(&log_file).unwrap();
    assert!(!abort_log.contains("image prune"));

    // 3. Confirming with `yes` executes `podman image prune -f -a` and succeeds.
    fs::write(&log_file, "").unwrap();
    let confirmed = workspace.run_pods_with_env(
        &["image", "prune", "--all"],
        b"yes\n",
        &[
            ("MOCK_PODMAN_IMAGES", PRUNABLE_IMAGES_LIST),
            ("MOCK_PODMAN_LOG", &log_path),
        ],
    );
    assert!(confirmed.success, "confirmed stderr: {}", confirmed.stderr);
    assert!(confirmed.stderr.contains("? Type 'yes' to proceed: "));
    assert!(!confirmed.stderr.contains("ERROR:"));
    let confirm_log = fs::read_to_string(&log_file).unwrap();
    assert!(confirm_log.contains("image prune -f -a"));

    // 4. Non-zero exit from `podman image prune` propagates failure without duplicate `ERROR:`.
    let failed_prune = workspace.run_pods_with_env(
        &["image", "prune"],
        b"yes\n",
        &[
            ("MOCK_PODMAN_IMAGES", PRUNABLE_IMAGES_LIST),
            ("MOCK_PODMAN_FAIL_CMD", "image prune"),
            ("MOCK_PODMAN_FAIL_CODE", "125"),
        ],
    );
    assert!(!failed_prune.success);
    assert!(
        failed_prune
            .stderr
            .contains("WARNING: Command exited with code '125'.")
    );
    assert!(
        !failed_prune.stderr.contains("ERROR:"),
        "ExecOutcome::Failed must not emit duplicate ERROR line: {:?}",
        failed_prune.stderr
    );
}

#[test]
fn live_podman_nonzero_exit_propagates_failure_without_duplicate_error_line() {
    let workspace = TestWorkspace::new("nonzero_exit");
    fs::write(workspace.dir().join("pods.conf"), LIVE_CONFIG).unwrap();
    fs::write(workspace.dir().join("alpha.pod"), ALPHA_RECIPE).unwrap();

    let failed_start = workspace.run_pods_with_env(
        &["start", "alpha"],
        b"",
        &[
            ("MOCK_PODMAN_FAIL_CMD", "start"),
            ("MOCK_PODMAN_FAIL_CODE", "125"),
            ("MOCK_PODMAN_FAIL_MSG", "Error: container does not exist"),
        ],
    );
    assert!(!failed_start.success);
    assert!(
        failed_start
            .stderr
            .contains("WARNING: Command exited with code '125'.")
    );
    assert!(
        !failed_start.stderr.contains("ERROR:"),
        "expected no duplicate ERROR line on ExecOutcome::Failed, got {:?}",
        failed_start.stderr
    );

    let failed_up = workspace.run_pods_with_env(
        &["up", "alpha"],
        b"",
        &[
            ("MOCK_PODMAN_FAIL_CMD", "pod create"),
            ("MOCK_PODMAN_FAIL_CODE", "125"),
        ],
    );
    assert!(!failed_up.success);
    assert!(
        failed_up
            .stderr
            .contains("WARNING: Command exited with code '125'.")
    );
    assert!(
        !failed_up.stderr.contains("ERROR:"),
        "expected no duplicate ERROR line on failed pod create, got {:?}",
        failed_up.stderr
    );
}

#[test]
fn live_update_deduplicates_shared_image_pull_and_reloads_both_pods() {
    let workspace = TestWorkspace::new("shared_update");
    fs::write(workspace.dir().join("pods.conf"), LIVE_CONFIG).unwrap();
    fs::write(workspace.dir().join("alpha.pod"), ALPHA_RECIPE).unwrap();
    fs::write(workspace.dir().join("beta.pod"), BETA_RECIPE).unwrap();
    fs::write(workspace.dir().join("gamma.pod"), GAMMA_RECIPE).unwrap();

    let state_dir = workspace.dir().join("mock_state");
    let log_file = workspace.dir().join("podman.log");
    let state_path = state_dir.to_string_lossy().into_owned();
    let log_path = log_file.to_string_lossy().into_owned();

    let update = workspace.run_pods_with_env(
        &["update", "alpha", "beta", "gamma"],
        b"",
        &[
            ("MOCK_PODMAN_LOG", &log_path),
            ("MOCK_PODMAN_STATE_DIR", &state_path),
            (
                "MOCK_PODMAN_UPDATE_IMAGE",
                "docker.io/example/shared:latest",
            ),
        ],
    );
    assert!(
        update.success,
        "live update failed: stdout={:?}, stderr={:?}",
        update.stdout, update.stderr
    );
    assert!(
        update.stderr.is_empty(),
        "expected clean stderr on live update, got {:?}",
        update.stderr
    );
    assert!(
        update.stdout.contains(
            "Update Summary:\n  Reloaded (2):\n    - alpha (Alpha Stack): alpha-app (docker.io/example/shared:latest)\n    - beta (Beta Stack): beta-worker (docker.io/example/shared:latest)\n\n  Up to date (1):\n    - gamma (Gamma Stack)\n\nChecked 3 pods (3 containers): 2 reloaded, 1 up to date, 0 failed."
        ),
        "unexpected update summary:\n{}",
        update.stdout
    );

    let log_contents = fs::read_to_string(&log_file).unwrap();
    let shared_pulls = log_contents
        .lines()
        .filter(|line| *line == "pull docker.io/example/shared:latest")
        .count();
    assert_eq!(
        shared_pulls, 1,
        "shared image must be pulled only once across alpha and beta:\n{log_contents}"
    );
    assert!(
        log_contents.contains("pod rm alpha"),
        "alpha must reload when shared image updates:\n{log_contents}"
    );
    assert!(
        log_contents.contains("pod rm beta"),
        "beta must reload via stale container image ID detection:\n{log_contents}"
    );
    assert!(
        !log_contents.contains("pod rm gamma"),
        "gamma must not reload when its image is up to date:\n{log_contents}"
    );
}
