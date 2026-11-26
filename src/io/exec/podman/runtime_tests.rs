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

//! Unit tests for Podman CLI runtime operations in [`super`].

use std::io::{self, Write};

use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;
use super::{
    ExecContainer, PodLogs, PullOutcome, SHELL_FALLBACK_COMMAND, build_container_exec_args,
    build_pod_logs_args, exec_container, logs_pod, pull_image, pull_image_with,
    resolve_pull_outcome,
};

#[test]
fn build_exec_and_logs_args_gate_tty_and_color_flags() {
    let exec_tokens = vec![
        String::from("sh"),
        String::from("-c"),
        String::from("echo hello world"),
    ];
    let interactive_request = ExecContainer {
        command: &exec_tokens,
        container_name: "cmd_pod-db",
        interactive: true,
        user: Some("33"),
    };
    assert_eq!(
        build_container_exec_args(&interactive_request, true),
        vec![
            "exec",
            "-it",
            "-u",
            "33",
            "cmd_pod-db",
            "sh",
            "-c",
            "echo hello world",
        ]
    );
    assert_eq!(
        build_container_exec_args(&interactive_request, false),
        vec![
            "exec",
            "-i",
            "-u",
            "33",
            "cmd_pod-db",
            "sh",
            "-c",
            "echo hello world",
        ]
    );

    let non_interactive = ExecContainer {
        interactive: false,
        ..interactive_request
    };
    assert_eq!(
        build_container_exec_args(&non_interactive, true),
        vec![
            "exec",
            "-u",
            "33",
            "cmd_pod-db",
            "sh",
            "-c",
            "echo hello world",
        ]
    );
    assert_eq!(
        build_container_exec_args(&non_interactive, false),
        vec![
            "exec",
            "-u",
            "33",
            "cmd_pod-db",
            "sh",
            "-c",
            "echo hello world",
        ]
    );

    let logs_request = PodLogs {
        container_name: Some("cmd_pod-db"),
        follow: true,
        pod_name: "cmd_pod",
        since: Some("1h"),
        tail: Some("10"),
        timestamps: true,
        until: Some("2h"),
    };
    assert_eq!(
        build_pod_logs_args(&logs_request, true),
        vec![
            "pod",
            "logs",
            "-n",
            "--color",
            "-f",
            "--since",
            "1h",
            "--until",
            "2h",
            "--tail",
            "10",
            "--timestamps",
            "-c",
            "cmd_pod-db",
            "cmd_pod",
        ]
    );
    assert_eq!(
        build_pod_logs_args(&logs_request, false),
        vec![
            "pod",
            "logs",
            "-n",
            "-f",
            "--since",
            "1h",
            "--until",
            "2h",
            "--tail",
            "10",
            "--timestamps",
            "-c",
            "cmd_pod-db",
            "cmd_pod",
        ]
    );
}

#[test]
fn exec_logs_and_pull_validate_inputs_and_support_uppercase_unmanaged_names() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    assert_eq!(
        exec_container(
            &ExecContainer {
                command: &SHELL_FALLBACK_COMMAND,
                container_name: "MyApp_1",
                interactive: true,
                user: None,
            },
            "Connect: ",
            true,
            &mut stdout,
            &mut stderr,
        )
        .unwrap(),
        ExecOutcome::Succeeded
    );
    assert!(
        String::from_utf8(stdout)
            .unwrap()
            .contains("podman exec -it MyApp_1 sh -c")
    );
    assert!(stderr.is_empty());

    for bad_exec in [
        ExecContainer {
            command: &["sh"],
            container_name: "--privileged",
            interactive: false,
            user: None,
        },
        ExecContainer {
            command: &["sh"],
            container_name: "_leading_underscore",
            interactive: false,
            user: None,
        },
        ExecContainer {
            command: &["sh"],
            container_name: "si_po-1",
            interactive: false,
            user: Some("--privileged"),
        },
        ExecContainer {
            command: &[],
            container_name: "si_po-1",
            interactive: false,
            user: None,
        },
        ExecContainer {
            command: &["--privileged", "sh"],
            container_name: "si_po-1",
            interactive: false,
            user: None,
        },
        ExecContainer {
            command: &["  sh"],
            container_name: "si_po-1",
            interactive: false,
            user: None,
        },
        ExecContainer {
            command: &["sh  "],
            container_name: "si_po-1",
            interactive: false,
            user: None,
        },
        ExecContainer {
            command: &["sh", "-c", "echo\nhello"],
            container_name: "si_po-1",
            interactive: false,
            user: None,
        },
    ] {
        assert!(
            exec_container(&bad_exec, "", true, &mut Vec::new(), &mut Vec::new()).is_err(),
            "expected exec_container to fail for {bad_exec:?}"
        );
    }

    let base_logs = PodLogs {
        container_name: None,
        follow: false,
        pod_name: "cmd_pod",
        since: None,
        tail: None,
        timestamps: false,
        until: None,
    };
    for bad_logs in [
        PodLogs {
            pod_name: "--latest",
            ..base_logs
        },
        PodLogs {
            container_name: Some("--latest"),
            ..base_logs
        },
        PodLogs {
            since: Some("-f"),
            ..base_logs
        },
        PodLogs {
            until: Some("-f"),
            ..base_logs
        },
        PodLogs {
            tail: Some("-f"),
            ..base_logs
        },
        PodLogs {
            tail: Some("abc"),
            ..base_logs
        },
    ] {
        assert!(logs_pod(&bad_logs, "", true, &mut Vec::new(), &mut Vec::new()).is_err());
    }

    let spaced_logs = PodLogs {
        since: Some("2026-01-01 12:00:00"),
        tail: Some("all"),
        until: Some("2026-01-01 13:00:00"),
        ..base_logs
    };
    let mut spaced_out = Vec::new();
    assert_eq!(
        logs_pod(&spaced_logs, "Logs: ", true, &mut spaced_out, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert!(
        String::from_utf8(spaced_out)
            .unwrap()
            .contains("--since '2026-01-01 12:00:00' --until '2026-01-01 13:00:00' --tail all")
    );

    let mut logs_out = Vec::new();
    assert_eq!(
        logs_pod(&base_logs, "Logs: ", true, &mut logs_out, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert!(
        String::from_utf8(logs_out)
            .unwrap()
            .contains("podman pod logs -n")
    );

    for bad_image in [
        "",
        "--all-tags",
        "-q",
        "alpine:latest --tls-verify=false",
        "alpine\n",
    ] {
        let result = pull_image(bad_image, "", true, &mut Vec::new(), &mut Vec::new());
        assert!(
            matches!(
                result,
                Err(PodError::Io(ref error)) if error.kind() == io::ErrorKind::InvalidInput
            ),
            "expected InvalidInput for bad_image {bad_image:?}"
        );
    }
}

#[test]
fn pull_image_resolves_outcomes_and_prefixes_without_live_podman() {
    assert_eq!(PullOutcome::Failed.spinner_suffix(), "Failed.");
    assert_eq!(PullOutcome::Simulated.spinner_suffix(), "Simulated.");
    assert_eq!(PullOutcome::UpToDate.spinner_suffix(), "Up to date.");
    assert_eq!(PullOutcome::Updated.spinner_suffix(), "Updated.");

    assert_eq!(
        resolve_pull_outcome(ExecOutcome::Failed, Some("old"), Some("new")),
        PullOutcome::Failed
    );
    assert_eq!(
        resolve_pull_outcome(ExecOutcome::Succeeded, None, Some("new")),
        PullOutcome::Updated
    );
    assert_eq!(
        resolve_pull_outcome(ExecOutcome::Succeeded, Some("old"), Some("new")),
        PullOutcome::Updated
    );
    assert_eq!(
        resolve_pull_outcome(ExecOutcome::Succeeded, Some("same"), Some("same")),
        PullOutcome::UpToDate
    );
    assert_eq!(
        resolve_pull_outcome(ExecOutcome::Succeeded, None, None),
        PullOutcome::UpToDate
    );

    let mut sim_out = Vec::new();
    let mut sim_err = Vec::new();
    assert_eq!(
        pull_image(
            "docker.io/alpine:latest",
            "",
            true,
            &mut sim_out,
            &mut sim_err
        )
        .unwrap(),
        PullOutcome::Simulated
    );
    assert_eq!(
        String::from_utf8(sim_out).unwrap(),
        "podman pull docker.io/alpine:latest\n"
    );

    let mut id_calls = 0usize;
    let mut mock_query = |_: &str| {
        id_calls += 1;
        if id_calls == 1 {
            Some(String::from("sha256:old"))
        } else {
            Some(String::from("sha256:new"))
        }
    };
    let mut mock_updated_spinner = |arguments: &[&str],
                                    line_prefix: &str,
                                    is_simulated: bool,
                                    resolve: &mut dyn FnMut(ExecOutcome) -> &'static str,
                                    out: &mut Vec<u8>,
                                    _: &mut Vec<u8>|
     -> Result<(), PodError> {
        assert!(!is_simulated);
        assert_eq!(arguments, &["pull", "docker.io/alpine:latest"]);
        let suffix = resolve(ExecOutcome::Succeeded);
        let _ = writeln!(out, "{line_prefix}{suffix}");
        Ok(())
    };
    let mut updated_out = Vec::new();
    let updated_outcome = pull_image_with(
        "docker.io/alpine:latest",
        "",
        false,
        &mut mock_query,
        &mut mock_updated_spinner,
        &mut updated_out,
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(updated_outcome, PullOutcome::Updated);
    assert_eq!(
        String::from_utf8(updated_out).unwrap(),
        "Trying to pull docker.io/alpine:latest... Updated.\n"
    );

    let mut mock_fail_spinner = |_: &[&str],
                                 line_prefix: &str,
                                 _: bool,
                                 resolve: &mut dyn FnMut(ExecOutcome) -> &'static str,
                                 out: &mut Vec<u8>,
                                 _: &mut Vec<u8>|
     -> Result<(), PodError> {
        let suffix = resolve(ExecOutcome::Failed);
        let _ = writeln!(out, "{line_prefix}{suffix}");
        Ok(())
    };
    let mut failed_out = Vec::new();
    let failed_outcome = pull_image_with(
        "docker.io/alpine:latest",
        "  Pull 'app'... ",
        false,
        &mut |_| None,
        &mut mock_fail_spinner,
        &mut failed_out,
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(failed_outcome, PullOutcome::Failed);
    assert_eq!(
        String::from_utf8(failed_out).unwrap(),
        "  Pull 'app'... Failed.\n"
    );
}
