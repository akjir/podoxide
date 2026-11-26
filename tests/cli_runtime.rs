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

//! Live runtime CLI integration tests covering status tables, connect, image prune aliases, log tail translation, offline bypasses, editor exit propagation, and stale-container updates.

#![forbid(unsafe_code)]

mod common;

use std::fs;
use std::path::Path;

use self::common::TestWorkspace;

const ALPHA_SHARED_RECIPE: &str = "\
pod alpha-pod {
    name     \"Alpha Pod\"
    registry docker.io
}
container shared-worker {
    image example/worker:latest
}
";

const BETA_SHARED_RECIPE: &str = "\
pod beta-pod {
    name     \"Beta Pod\"
    registry docker.io
}
container shared-worker {
    image example/worker:latest
}
";

const HEX_IMAGE_DANGLING: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const HEX_IMAGE_IN_USE: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const HEX_IMAGE_MULTI: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const HEX_IMAGE_NEW: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HEX_IMAGE_OLD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const LIVE_EDITOR_FALSE_CONFIG: &str = "\
general {
    editor   false
    simulate false
}
pods {
    path ./pods
}
recipes {
    path .
    groups {
        all recipe
    }
}
";

const LIVE_RECIPE_CONFIG: &str = "\
general {
    editor   true
    simulate false
}
pods {
    path ./pods
}
recipes {
    path .
    groups {
        all recipe
    }
}
";

const LIVE_SHARED_CONFIG: &str = "\
general {
    editor   true
    simulate false
}
pods {
    path ./pods
}
recipes {
    path .
    groups {
        all alpha beta
    }
}
";

fn assert_no_trailing_spaces(output: &str, context: &str) {
    for (index, line) in output.lines().enumerate() {
        assert!(
            !line.ends_with(' '),
            "{context}: line {} has trailing spaces: {line:?}",
            index + 1
        );
    }
}

fn read_podman_log(log_file: &Path) -> String {
    fs::read_to_string(log_file).expect("read mock podman call log")
}

#[test]
fn live_connect_handles_running_stopped_and_missing_unmanaged_uppercase_container() {
    let workspace = TestWorkspace::new("runtime_connect");
    fs::write(workspace.dir().join("pods.conf"), LIVE_RECIPE_CONFIG).unwrap();
    let log_file = workspace.dir().join("podman.log");
    let log_path = log_file.to_string_lossy().into_owned();

    let running = workspace.run_pods_with_env(
        &["connect", "MyApp_01"],
        b"",
        &[
            ("MOCK_PODMAN_CONTAINER_EXISTS_EXIT", "0"),
            ("MOCK_PODMAN_CONTAINER_STATUS", "running"),
            ("MOCK_PODMAN_LOG", &log_path),
        ],
    );
    assert!(
        running.success,
        "running connect stderr: {}",
        running.stderr
    );
    let call_log = read_podman_log(&log_file);
    assert!(
        call_log.contains("exec -i MyApp_01 sh -c"),
        "expected piped podman exec for MyApp_01, got:\n{call_log}"
    );

    let stopped = workspace.run_pods_with_env(
        &["connect", "MyApp_01"],
        b"",
        &[
            ("MOCK_PODMAN_CONTAINER_EXISTS_EXIT", "0"),
            ("MOCK_PODMAN_CONTAINER_STATUS", "exited"),
        ],
    );
    assert!(!stopped.success);
    assert!(
        stopped.stderr.contains("is not running"),
        "expected ContainerNotRunning for stopped container, got: {:?}",
        stopped.stderr
    );

    let missing = workspace.run_pods_with_env(
        &["connect", "MyApp_01"],
        b"",
        &[
            ("MOCK_PODMAN_CONTAINER_EXISTS_EXIT", "1"),
            ("MOCK_PODMAN_CONTAINER_STATUS", "exited"),
        ],
    );
    assert!(!missing.success);
    assert!(
        missing.stderr.contains("target does not exist: 'MyApp_01'"),
        "expected TargetNotFound for missing container, got: {:?}",
        missing.stderr
    );
}

#[test]
fn live_editor_nonzero_exit_propagates_failure_for_config_and_recipe_edit() {
    let workspace = TestWorkspace::new("runtime_editor_fail");
    assert!(workspace.run_pods(&["init"], b"").success);
    fs::write(workspace.dir().join("pods.conf"), LIVE_EDITOR_FALSE_CONFIG).unwrap();

    for arguments in [&["config", "edit"][..], &["recipe", "edit", "recipe"][..]] {
        let output = workspace.run_pods(arguments, b"");
        assert!(!output.success, "expected {arguments:?} to exit non-zero");
        assert!(
            output
                .stderr
                .contains("WARNING: Command exited with code '1'."),
            "expected exit-code warning for {arguments:?}, got: {:?}",
            output.stderr
        );
        assert!(
            !output.stderr.contains("ERROR:"),
            "expected no duplicate ERROR line for {arguments:?}, got: {:?}",
            output.stderr
        );
    }
}

#[test]
fn live_image_prune_excludes_in_use_groups_aliases_and_prints_prune_stdout() {
    let workspace = TestWorkspace::new("runtime_prune_aliases");
    fs::write(workspace.dir().join("pods.conf"), LIVE_RECIPE_CONFIG).unwrap();

    let images_file = workspace.dir().join("mock_images.txt");
    let images_payload = format!(
        "111111111111\x1f{HEX_IMAGE_IN_USE}\x1fdocker.io/example/in-use\x1f1.0\x1f50 MB\x1f3 days ago\x1e\n\
         222222222222\x1f{HEX_IMAGE_MULTI}\x1fdocker.io/example/multi\x1f1.0\x1f20 MB\x1f2 days ago\x1e\n\
         222222222222\x1f{HEX_IMAGE_MULTI}\x1fdocker.io/example/multi\x1flatest\x1f20 MB\x1f2 days ago\x1e\n\
         333333333333\x1f{HEX_IMAGE_DANGLING}\x1f<none>\x1f<none>\x1f10 MB\x1f1 hour ago\x1e\n"
    );
    fs::write(&images_file, images_payload).unwrap();
    let images_path = images_file.to_string_lossy().into_owned();
    let in_use_payload = format!("{HEX_IMAGE_IN_USE}\n");

    let preview = workspace.run_pods_with_env(
        &["image", "prune", "--all", "--preview"],
        b"",
        &[
            ("MOCK_PODMAN_IMAGES_FILE", &images_path),
            ("MOCK_PODMAN_IN_USE_IMAGES", &in_use_payload),
        ],
    );
    assert!(preview.success, "preview stderr: {}", preview.stderr);
    assert!(
        !preview.stdout.contains("docker.io/example/in-use"),
        "in-use image must be excluded from preview:\n{}",
        preview.stdout
    );
    assert!(
        preview
            .stdout
            .contains("docker.io/example/multi:1.0, docker.io/example/multi:latest"),
        "multi-tag aliases must appear together in preview:\n{}",
        preview.stdout
    );
    assert!(
        preview.stdout.contains("333333333333"),
        "dangling image must appear in preview:\n{}",
        preview.stdout
    );
    assert!(
        preview.stdout.contains("Total space reclaimable:") && preview.stdout.contains("30.0 MB"),
        "deduplicated reclaimable total must equal 30.0 MB:\n{}",
        preview.stdout
    );

    let confirmed = workspace.run_pods_with_env(
        &["image", "prune", "--all"],
        b"yes\n",
        &[
            ("MOCK_PODMAN_IMAGES_FILE", &images_path),
            ("MOCK_PODMAN_IN_USE_IMAGES", &in_use_payload),
            ("MOCK_PODMAN_PRUNE_STDOUT", "Deleted Images:\n222222222222"),
        ],
    );
    assert!(confirmed.success, "confirmed stderr: {}", confirmed.stderr);
    assert!(
        confirmed
            .stderr
            .contains("Are you sure you want to prune 2 unused images (30.0 MB)?"),
        "unexpected confirmation prompt: {:?}",
        confirmed.stderr
    );
    assert!(
        confirmed.stdout.contains("Deleted Images:\n222222222222"),
        "expected prune stdout passthrough, got: {:?}",
        confirmed.stdout
    );
}

#[test]
fn live_logs_tail_all_forwards_negative_one_to_podman() {
    let workspace = TestWorkspace::new("runtime_logs_tail");
    assert!(workspace.run_pods(&["init"], b"").success);
    fs::write(workspace.dir().join("pods.conf"), LIVE_RECIPE_CONFIG).unwrap();
    let log_file = workspace.dir().join("podman.log");
    let log_path = log_file.to_string_lossy().into_owned();

    let pod_logs = workspace.run_pods_with_env(
        &["logs", "--tail=all", "show", "recipe"],
        b"",
        &[("MOCK_PODMAN_LOG", &log_path)],
    );
    assert!(pod_logs.success, "logs stderr: {}", pod_logs.stderr);

    let container_logs = workspace.run_pods_with_env(
        &["logs", "--tail=ALL", "recipe/db"],
        b"",
        &[("MOCK_PODMAN_LOG", &log_path)],
    );
    assert!(
        container_logs.success,
        "container logs stderr: {}",
        container_logs.stderr
    );

    let call_log = read_podman_log(&log_file);
    assert!(
        call_log.contains("pod logs -n --tail -1 example-stack"),
        "expected --tail -1 for pod logs, got:\n{call_log}"
    );
    assert!(
        call_log.contains("pod logs -n --tail -1 -c example-stack-db example-stack"),
        "expected --tail -1 for container logs, got:\n{call_log}"
    );
}

#[test]
fn live_status_trims_trailing_spaces_and_keeps_shared_container_names_distinct() {
    let workspace = TestWorkspace::new("runtime_status");
    fs::write(workspace.dir().join("pods.conf"), LIVE_SHARED_CONFIG).unwrap();
    fs::write(workspace.dir().join("alpha.pod"), ALPHA_SHARED_RECIPE).unwrap();
    fs::write(workspace.dir().join("beta.pod"), BETA_SHARED_RECIPE).unwrap();

    // Include literal backslash in COMMAND to verify mock podman does not interpret escapes,
    // empty PORTS on the running worker, and empty IMAGE + PORTS on the unmanaged infra row.
    let ps_records = "\
aaaa11112222\x1fdocker.io/example/worker:latest\x1fsh -c \"printf \\\\n\"\x1f2 hours ago\x1fUp 2 hours\x1f\x1fshared-worker\x1falpha-pod\x1f0\x1e\n\
ffff99990000\x1f\x1f/catatonit -P\x1f2 hours ago\x1fUp 2 hours\x1f\x1falpha-pod-infra\x1falpha-pod\x1f0\x1e\n";

    let status = workspace.run_pods_with_env(&["status"], b"", &[("MOCK_PODMAN_PS", ps_records)]);
    assert!(status.success, "status stderr: {}", status.stderr);
    assert_no_trailing_spaces(&status.stdout, "pods status");
    assert!(
        status.stdout.contains("alpha-pod") && status.stdout.contains("Up 2 hours"),
        "expected running alpha-pod/shared-worker:\n{}",
        status.stdout
    );
    assert!(
        status.stdout.contains("beta-pod") && status.stdout.contains("Not Found"),
        "expected distinct missing beta-pod/shared-worker:\n{}",
        status.stdout
    );

    let ps_file = workspace.dir().join("mock_ps.txt");
    fs::write(&ps_file, ps_records).unwrap();
    let ps_file_path = ps_file.to_string_lossy().into_owned();

    let full_all = workspace.run_pods_with_env(
        &["status", "--all", "--full"],
        b"",
        &[("MOCK_PODMAN_PS_FILE", &ps_file_path)],
    );
    assert!(
        full_all.success,
        "status --all --full stderr: {}",
        full_all.stderr
    );
    assert_no_trailing_spaces(&full_all.stdout, "pods status --all --full");
    assert!(
        full_all.stdout.contains("alpha/1") && full_all.stdout.contains("beta/1"),
        "expected distinct TARGET NAME entries:\n{}",
        full_all.stdout
    );
    assert!(
        full_all.stdout.contains("alpha-pod-infra"),
        "expected unmanaged infra row in --all output:\n{}",
        full_all.stdout
    );
    assert!(
        full_all.stdout.contains(r#"sh -c "printf \\n""#),
        "expected literal backslashes preserved by mock podman:\n{}",
        full_all.stdout
    );
}

#[test]
fn live_update_reloads_stale_container_and_respects_pod_exists() {
    let workspace = TestWorkspace::new("runtime_stale_update");
    assert!(workspace.run_pods(&["init"], b"").success);
    fs::write(workspace.dir().join("pods.conf"), LIVE_RECIPE_CONFIG).unwrap();
    let log_file = workspace.dir().join("podman.log");
    let log_path = log_file.to_string_lossy().into_owned();

    // Case 1: Cached tag already at HEX_IMAGE_NEW (pull UpToDate), container runs HEX_IMAGE_OLD,
    // and parent pod already exists -> stale notice triggers container reload without `pod create`.
    let stale_existing_pod = workspace.run_pods_with_env(
        &["update", "recipe/db"],
        b"",
        &[
            ("MOCK_PODMAN_CONTAINER_IMAGE_ID", HEX_IMAGE_OLD),
            ("MOCK_PODMAN_IMAGE_ID_BEFORE", HEX_IMAGE_NEW),
            ("MOCK_PODMAN_LOG", &log_path),
            ("MOCK_PODMAN_POD_EXISTS_EXIT", "0"),
        ],
    );
    assert!(
        stale_existing_pod.success,
        "stale update stderr: {}",
        stale_existing_pod.stderr
    );
    assert!(
        stale_existing_pod
            .stdout
            .contains("Container 'example-stack-db' is running an older image; reloading."),
        "expected stale-container reload notice:\n{}",
        stale_existing_pod.stdout
    );
    assert!(
        stale_existing_pod.stdout.contains("Reloaded (1):"),
        "expected Reloaded summary:\n{}",
        stale_existing_pod.stdout
    );
    let existing_pod_log = read_podman_log(&log_file);
    assert!(existing_pod_log.contains("pod exists example-stack"));
    assert!(
        !existing_pod_log.contains("pod create"),
        "existing pod must not be recreated:\n{existing_pod_log}"
    );
    assert!(existing_pod_log.contains("create --name example-stack-db"));

    // Case 2: Pull updates image from HEX_IMAGE_OLD to HEX_IMAGE_NEW while parent pod is absent
    // -> container reload automatically creates missing parent pod.
    fs::write(&log_file, "").unwrap();
    let state_dir = workspace.dir().join("state");
    let state_path = state_dir.to_string_lossy().into_owned();
    let missing_pod_update = workspace.run_pods_with_env(
        &["update", "recipe/db"],
        b"",
        &[
            ("MOCK_PODMAN_CONTAINER_IMAGE_ID", HEX_IMAGE_OLD),
            ("MOCK_PODMAN_IMAGE_ID_AFTER", HEX_IMAGE_NEW),
            ("MOCK_PODMAN_IMAGE_ID_BEFORE", HEX_IMAGE_OLD),
            ("MOCK_PODMAN_LOG", &log_path),
            ("MOCK_PODMAN_POD_EXISTS_EXIT", "1"),
            ("MOCK_PODMAN_STATE_DIR", &state_path),
        ],
    );
    assert!(
        missing_pod_update.success,
        "missing-pod update stderr: {}",
        missing_pod_update.stderr
    );
    let missing_pod_log = read_podman_log(&log_file);
    assert!(missing_pod_log.contains("pod exists example-stack"));
    assert!(
        missing_pod_log.contains("pod create --name example-stack"),
        "missing parent pod must be created automatically:\n{missing_pod_log}"
    );
    assert!(missing_pod_log.contains("create --name example-stack-db"));
}

#[test]
fn offline_actions_succeed_when_podman_version_command_fails() {
    let workspace = TestWorkspace::new("runtime_offline");
    assert!(workspace.run_pods(&["init"], b"").success);

    let offline_cases: &[&[&str]] = &[
        &["cfg", "show"],
        &["cfg", "inspect"],
        &["rec", "list"],
        &["rec", "inspect", "recipe"],
        &["rec", "show", "recipe"],
        &["cmd", "list", "recipe"],
    ];
    for arguments in offline_cases {
        let output =
            workspace.run_pods_with_env(arguments, b"", &[("MOCK_PODMAN_VERSION_FAIL", "1")]);
        assert!(
            output.success,
            "expected offline action {arguments:?} to succeed without podman: {}",
            output.stderr
        );
    }

    let online =
        workspace.run_pods_with_env(&["status"], b"", &[("MOCK_PODMAN_VERSION_FAIL", "1")]);
    assert!(!online.success);
    assert_eq!(
        online.stderr,
        "ERROR: Podman 5.8.0 or higher is required.\n"
    );
}
