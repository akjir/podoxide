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

//! Unit tests for container lifecycle execution, pull caching, and stale-container detection in [`super`].

use std::borrow::Cow;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::core::log::{self, MAX_LOG_MESSAGE_BYTES};
use crate::core::pod::Pod;
use crate::core::update::{ContainerUpdateResult, PullCache, PullRunner, UpdateAction, UpdateRunners};
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::PullOutcome;
use super::lifecycle::create_with_pod_check;
use super::{
    Container, VolumeMount, create, create_with, down, down_with, pull, pull_with, pull_with_cache,
    reload, reload_with, remove, remove_with, resolve_image_reference, restart, restart_with,
    start, start_with, stop, stop_with, up, up_with, update, update_with, update_with_cache,
};

fn run_update_no_inspect<P>(
    container: &Container,
    pod: &Pod,
    pull_image: &mut P,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
) -> Result<ContainerUpdateResult, PodError>
where
    P: FnMut(&str, &str, bool, &mut Vec<u8>, &mut Vec<u8>) -> Result<PullOutcome, PodError>,
{
    let mut cache = PullCache::new();
    let mut no_container = |_: &str| None;
    let mut no_image = |_: &str| None;
    let mut runners = UpdateRunners {
        cache: &mut cache,
        pull_image,
        query_container_image: &mut no_container,
        query_image: &mut no_image,
    };
    update_with_cache(container, pod, true, &mut runners, stdout, stderr)
}

struct BrokenPipeWriter;

impl Write for BrokenPipeWriter {
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
    }

    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
    }
}

#[test]
fn container_lifecycle_commands_and_simulation() {
    let mut pod = Pod::new();
    pod.id = String::from("commandos");
    pod.name = String::from("Commandos");
    pod.path = PathBuf::from("/my_pod_path");
    pod.registry = String::from("bel.io");

    let mut container = Container::new();
    container.name = String::from("commandos-1");
    container.image = String::from("squad:1998");
    container.restart = String::from("on-failure:5");
    container.options = vec![
        String::from("--unknown thing"),
        String::from("--another"),
        String::from("unknown"),
    ];
    container.volumes.push(VolumeMount {
        container_dir: String::from("/container/dir/relative"),
        host_dir: String::from("./relative/path"),
        options: String::from("z"),
    });
    container.commands = vec![
        String::from("O'Hara"),
        String::from("Hancock 2"),
        String::from("Woolridge3"),
        String::from("Brooklyn:4"),
        String::from("Blackwood=5"),
        String::from("Duchamp-6"),
    ];

    assert_eq!(
        resolve_image_reference(&container, &pod),
        "bel.io/squad:1998"
    );
    container.registry = String::from("bestRegEver.io");
    assert_eq!(
        resolve_image_reference(&container, &pod),
        "bestRegEver.io/squad:1998"
    );
    container.registry.clear();
    assert!(matches!(
        resolve_image_reference(&container, &Pod::new()),
        Cow::Borrowed("squad:1998")
    ));

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        create_with(&container, &pod, true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        start_with(&container, true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        stop_with(&container, true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        remove_with(&container, true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    let pull_record = pull_with(&container, &pod, "", true, &mut stdout, &mut stderr).unwrap();
    assert_eq!(pull_record.outcome, PullOutcome::Simulated);
    assert_eq!(pull_record.container_name, "commandos-1");
    assert_eq!(pull_record.image, "bel.io/squad:1998");

    let output = String::from_utf8(stdout).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines,
        vec![
            "Create pod 'Commandos' ('commandos'):",
            "podman pod create --name commandos",
            "Create container 'commandos-1':",
            "podman create --name commandos-1 --pod commandos --restart on-failure:5 --volume /my_pod_path/relative/path:/container/dir/relative:z --unknown thing --another unknown bel.io/squad:1998 'O'\\''Hara' 'Hancock 2' Woolridge3 Brooklyn:4 Blackwood=5 Duchamp-6",
            "Start container 'commandos-1':",
            "podman start commandos-1",
            "Stop container 'commandos-1':",
            "podman stop commandos-1",
            "Remove container 'commandos-1':",
            "podman rm commandos-1",
            "Trying to pull bel.io/squad:1998 (commandos-1):",
            "podman pull bel.io/squad:1998",
        ]
    );
    assert!(stderr.is_empty());

    let mut existing_pod = Pod::new();
    existing_pod.id = String::from("existing_pod");
    existing_pod.name = String::from("Existing Pod");
    existing_pod.path = PathBuf::from("/my_pod_path");
    existing_pod.registry = String::from("bel.io");
    let mut existing_out = Vec::new();
    assert_eq!(
        create_with_pod_check(
            &container,
            &existing_pod,
            true,
            |_| true,
            &mut existing_out,
            &mut stderr,
        )
        .unwrap(),
        ExecOutcome::Succeeded
    );
    let existing_lines: Vec<&str> = std::str::from_utf8(&existing_out)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(existing_lines.len(), 2);
    assert_eq!(existing_lines[0], "Create container 'commandos-1':");

    let mut composite_out = Vec::new();
    assert_eq!(
        up_with(&container, &pod, true, &mut composite_out, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        down_with(&container, true, &mut composite_out, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        restart_with(&container, true, &mut composite_out, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        reload_with(&container, &pod, true, &mut composite_out, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    update_with(&container, &pod, true, &mut composite_out, &mut stderr).unwrap();
    let composite_lines: Vec<&str> = std::str::from_utf8(&composite_out)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(composite_lines.len(), 30);

    let mut spaced_pod = pod.clone();
    spaced_pod.path = PathBuf::from("/pods/my pod");
    let mut spaced_container = container.clone();
    spaced_container.volumes = vec![VolumeMount {
        container_dir: String::from("/container/my dir"),
        host_dir: String::from("./my host"),
        options: String::from("ro"),
    }];
    let mut spaced_out = Vec::new();
    reload_with(
        &spaced_container,
        &spaced_pod,
        true,
        &mut spaced_out,
        &mut stderr,
    )
    .unwrap();
    assert!(
        String::from_utf8(spaced_out)
            .unwrap()
            .contains("--volume '/pods/my pod/my host:/container/my dir:ro'")
    );

    assert_eq!(
        create(&container, &pod, true).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(start(&container, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(stop(&container, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(remove(&container, true).unwrap(), ExecOutcome::Succeeded);
    assert!(pull(&container, &pod, true).is_ok());
    assert_eq!(up(&container, &pod, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(down(&container, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(restart(&container, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(
        reload(&container, &pod, true).unwrap(),
        ExecOutcome::Succeeded
    );
    assert!(update(&container, &pod, true).is_ok());
}

#[test]
fn container_update_detects_stale_image_on_pre_pulled_up_to_date() {
    let mut pod = Pod::new();
    pod.id = String::from("db_pod");
    pod.path = PathBuf::from("/pods/db_pod");
    pod.registry = String::from("docker.io");

    let mut container = Container::new();
    container.name = String::from("db_pod-db");
    container.image = String::from("postgres:16");

    let mut up_to_date_pull = |_: &str,
                               prefix: &str,
                               _: bool,
                               out: &mut Vec<u8>,
                               _: &mut Vec<u8>|
     -> Result<PullOutcome, PodError> {
        log::write_print(out, &format!("{prefix}Up to date."));
        Ok(PullOutcome::UpToDate)
    };

    // Container running sha256:old while pulled image is sha256:new -> detected as stale & reloaded.
    let mut stale_cache = PullCache::new();
    let mut stale_out = Vec::new();
    let mut stale_err = Vec::new();
    let mut query_stale_container = |name: &str| {
        assert_eq!(name, "db_pod-db");
        Some(String::from("sha256:old"))
    };
    let mut query_new_image = |reference: &str| {
        assert_eq!(reference, "docker.io/postgres:16");
        Some(String::from("sha256:new"))
    };
    let mut stale_runners = UpdateRunners {
        cache: &mut stale_cache,
        pull_image: &mut up_to_date_pull,
        query_container_image: &mut query_stale_container,
        query_image: &mut query_new_image,
    };
    let stale_result = update_with_cache(
        &container,
        &pod,
        true,
        &mut stale_runners,
        &mut stale_out,
        &mut stale_err,
    )
    .unwrap();
    assert_eq!(stale_result.action, UpdateAction::Reloaded);
    assert_eq!(stale_result.record.outcome, PullOutcome::Updated);
    let stale_text = String::from_utf8(stale_out).unwrap();
    let notice = "Container 'db_pod-db' is running an older image; reloading.";
    let notice_pos = stale_text
        .find(notice)
        .expect("expected stale-container notice before reload");
    let stop_pos = stale_text
        .find("Stop container 'db_pod-db':")
        .expect("expected Stop container output");
    assert!(
        notice_pos < stop_pos,
        "stale notice must appear before reload output:\n{stale_text}"
    );
    assert!(stale_text.contains("Create container 'db_pod-db':\n"));
    assert!(stale_text.contains("Start container 'db_pod-db':\n"));

    // Container running sha256:new matching pulled image sha256:new -> UpToDate, no reload.
    let mut current_cache = PullCache::new();
    let mut current_out = Vec::new();
    let mut current_err = Vec::new();
    let mut query_current_container = |_: &str| Some(String::from("sha256:new"));
    let mut query_same_image = |_: &str| Some(String::from("sha256:new"));
    let mut current_runners = UpdateRunners {
        cache: &mut current_cache,
        pull_image: &mut up_to_date_pull,
        query_container_image: &mut query_current_container,
        query_image: &mut query_same_image,
    };
    let current_result = update_with_cache(
        &container,
        &pod,
        true,
        &mut current_runners,
        &mut current_out,
        &mut current_err,
    )
    .unwrap();
    assert_eq!(current_result.action, UpdateAction::UpToDate);
    assert_eq!(current_result.record.outcome, PullOutcome::UpToDate);
    let current_text = String::from_utf8(current_out).unwrap();
    assert!(current_text.contains("Container 'db_pod-db' is up to date."));
    assert!(!current_text.contains("Stop container 'db_pod-db'"));
}

#[test]
fn container_update_with_sanitizes_errors_and_tolerates_broken_pipe() {
    let mut pod = Pod::new();
    pod.id = String::from("app_pod");
    pod.path = PathBuf::from("/pods/app_pod");

    let mut container = Container::new();
    container.name = String::from("app_1");
    container.image = String::from("alpine:latest");

    let mut broken_stdout = BrokenPipeWriter;
    let mut stderr = Vec::new();
    assert!(pull_with(&container, &pod, "", true, &mut broken_stdout, &mut stderr).is_ok());

    let mut injected = Container::new();
    injected.name = format!("bad\x1b[31m\n\u{202E}{}", "a".repeat(MAX_LOG_MESSAGE_BYTES));
    injected.image = String::from("alpine:latest");

    let mut captured_stdout = Vec::new();
    let update_error =
        pull_with(&injected, &pod, "", true, &mut captured_stdout, &mut stderr).unwrap_err();
    assert!(captured_stdout.is_empty());
    let formatted_error = update_error.to_string();
    assert!(!formatted_error.contains('\x1b'));
    assert!(!formatted_error.contains('\u{202E}'));
    assert_eq!(formatted_error.lines().count(), 1);

    let mut unchanged_out = Vec::new();
    let mut up_to_date_pull = |_: &str,
                               prefix: &str,
                               _: bool,
                               out: &mut Vec<u8>,
                               _: &mut Vec<u8>|
     -> Result<PullOutcome, PodError> {
        log::write_print(out, &format!("{prefix}Up to date."));
        Ok(PullOutcome::UpToDate)
    };
    let up_to_date_result = run_update_no_inspect(
        &container,
        &pod,
        &mut up_to_date_pull,
        &mut unchanged_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(up_to_date_result.action, UpdateAction::UpToDate);
    assert_eq!(
        String::from_utf8(unchanged_out)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        vec![
            "Trying to pull alpine:latest (app_1): Up to date.",
            "Container 'app_1' is up to date.",
        ]
    );

    let mut failed_out = Vec::new();
    let mut failed_pull = |_: &str,
                           prefix: &str,
                           _: bool,
                           out: &mut Vec<u8>,
                           _: &mut Vec<u8>|
     -> Result<PullOutcome, PodError> {
        log::write_print(out, &format!("{prefix}Failed."));
        Ok(PullOutcome::Failed)
    };
    let failed_result = run_update_no_inspect(
        &container,
        &pod,
        &mut failed_pull,
        &mut failed_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(failed_result.action, UpdateAction::Failed);
    assert_eq!(
        String::from_utf8(failed_out)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        vec![
            "Trying to pull alpine:latest (app_1): Failed.",
            "Container 'app_1' update failed.",
        ]
    );

    let mut direct_out = Vec::new();
    let mut direct_cache = PullCache::new();
    let mut direct_runner = PullRunner {
        cache: &mut direct_cache,
        pull_image: &mut failed_pull,
    };
    let pull_only = pull_with_cache(
        &container,
        &pod,
        "  ",
        true,
        &mut direct_runner,
        &mut direct_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(pull_only.outcome, PullOutcome::Failed);
    assert!(
        String::from_utf8(direct_out)
            .unwrap()
            .starts_with("  Trying to pull alpine:latest (app_1): ")
    );

    let mut changed_out = Vec::new();
    let mut updated_pull = |_: &str,
                            prefix: &str,
                            _: bool,
                            out: &mut Vec<u8>,
                            _: &mut Vec<u8>|
     -> Result<PullOutcome, PodError> {
        log::write_print(out, &format!("{prefix}Updated."));
        Ok(PullOutcome::Updated)
    };
    let changed_result = run_update_no_inspect(
        &container,
        &pod,
        &mut updated_pull,
        &mut changed_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(changed_result.action, UpdateAction::Reloaded);
    let changed_lines: Vec<&str> = std::str::from_utf8(&changed_out).unwrap().lines().collect();
    assert!(changed_lines.contains(&"Stop container 'app_1':"));
    assert!(changed_lines.contains(&"Create container 'app_1':"));
    assert!(changed_lines.contains(&"Start container 'app_1':"));
    assert!(!changed_lines.contains(&"Container 'app_1' is up to date."));
}
