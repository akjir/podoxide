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

//! Unit tests for Podman CLI lifecycle operations in [`super`].

use std::io;

use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;
use super::{
    CreateContainer, CreatePod, build_container_create_args, build_pod_create_args,
    create_container, create_pod, prune_images, remove_container, remove_pod, start_container,
    stop_container, stop_pod,
};

fn assert_invalid_lifecycle_name(bad_name: &str, stdout: &mut Vec<u8>, stderr: &mut Vec<u8>) {
    for result in [
        remove_container(bad_name, "", true, stdout, stderr),
        remove_pod(bad_name, "", true, stdout, stderr),
        start_container(bad_name, "", true, stdout, stderr),
        stop_container(bad_name, "", true, stdout, stderr),
        stop_pod(bad_name, "", true, stdout, stderr),
    ] {
        assert!(
            matches!(
                result,
                Err(PodError::Io(ref error)) if error.kind() == io::ErrorKind::InvalidInput
            ),
            "expected InvalidInput for bad_name {bad_name:?}"
        );
    }
}

#[test]
fn lifecycle_commands_build_args_and_simulate() {
    let publish = vec![
        String::from("127.0.0.1:42:21/TCP"),
        String::from("127.0.0.1::42"),
    ];
    let options = vec![
        String::from("--unknown thing"),
        String::from("--another"),
        String::from("unknown"),
    ];
    let create_pod_request = CreatePod {
        options: &options,
        pod_name: "si_po",
        publish: &publish,
    };
    assert_eq!(
        build_pod_create_args(&create_pod_request),
        vec![
            "pod",
            "create",
            "--name",
            "si_po",
            "--publish",
            "127.0.0.1:42:21/TCP",
            "--publish",
            "127.0.0.1::42",
            "--unknown",
            "thing",
            "--another",
            "unknown",
        ]
    );

    let volumes = vec![String::from("/host:/container:ro")];
    let commands = vec![String::from("sh"), String::from("-c")];
    let create_container_request = CreateContainer {
        commands: &commands,
        container_name: "si_po-1",
        image: "docker.io/alpine:latest",
        options: &options,
        pod_name: "si_po",
        restart: "on-failure:5",
        volumes: &volumes,
    };
    assert_eq!(
        build_container_create_args(&create_container_request),
        vec![
            "create",
            "--name",
            "si_po-1",
            "--pod",
            "si_po",
            "--restart",
            "on-failure:5",
            "--volume",
            "/host:/container:ro",
            "--unknown",
            "thing",
            "--another",
            "unknown",
            "docker.io/alpine:latest",
            "sh",
            "-c",
        ]
    );

    let no_restart = CreateContainer {
        commands: &[],
        container_name: "si_po-1",
        image: "alpine:latest",
        options: &[],
        pod_name: "si_po",
        restart: "",
        volumes: &[],
    };
    assert!(!build_container_create_args(&no_restart).contains(&String::from("--restart")));

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        create_pod(
            &create_pod_request,
            "Create pod: ",
            true,
            &mut stdout,
            &mut stderr
        )
        .unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        stop_pod("si_po", "Stop pod: ", true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        remove_pod("si_po", "Remove pod: ", true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        create_container(
            &no_restart,
            "Create container: ",
            true,
            &mut stdout,
            &mut stderr,
        )
        .unwrap(),
        ExecOutcome::Succeeded
    );
    let spaced_volumes = [
        String::from("/anon"),
        String::from("/host:/data"),
        String::from("/pods/my pod/data:/data/my dir:ro,Z"),
    ];
    assert_eq!(
        create_container(
            &CreateContainer {
                volumes: &spaced_volumes,
                ..no_restart
            },
            "",
            true,
            &mut Vec::new(),
            &mut stderr,
        )
        .unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        start_container("si_po-1", "Start: ", true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        stop_container("si_po-1", "Stop: ", true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        remove_container("si_po-1", "Remove: ", true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        prune_images(false, "Prune: ", true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        prune_images(true, "Prune all: ", true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );

    let rendered = String::from_utf8(stdout).unwrap();
    assert!(rendered.contains("podman pod create --name si_po"));
    assert!(rendered.contains("podman pod stop si_po"));
    assert!(rendered.contains("podman pod rm si_po"));
    assert!(rendered.contains("podman create --name si_po-1 --pod si_po alpine:latest"));
    assert!(rendered.contains("podman start si_po-1"));
    assert!(rendered.contains("podman stop si_po-1"));
    assert!(rendered.contains("podman rm si_po-1"));
    assert!(rendered.contains("podman image prune -f\n"));
    assert!(rendered.contains("podman image prune -f -a\n"));
    assert!(stderr.is_empty());
}

#[test]
fn lifecycle_rejects_flag_injection_and_malformed_inputs() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    for bad_name in ["", "--all", "-fa", "..", "*", "Upper_Case", "a/b"] {
        assert_invalid_lifecycle_name(bad_name, &mut stdout, &mut stderr);
    }

    let bad_publish = [String::from("--privileged")];
    let bad_options = [String::from("-- evil")];
    for bad_pod in [
        CreatePod {
            options: &[],
            pod_name: "--help",
            publish: &[],
        },
        CreatePod {
            options: &[],
            pod_name: "si_po",
            publish: &bad_publish,
        },
        CreatePod {
            options: &bad_options,
            pod_name: "si_po",
            publish: &[],
        },
    ] {
        assert!(create_pod(&bad_pod, "", true, &mut stdout, &mut stderr).is_err());
    }

    let bad_volumes = [String::from("--privileged")];
    let bad_commands = [String::from("sh\n-c")];
    let base_container = CreateContainer {
        commands: &[],
        container_name: "si_po-1",
        image: "alpine:latest",
        options: &[],
        pod_name: "si_po",
        restart: "",
        volumes: &[],
    };
    for bad_container in [
        CreateContainer {
            container_name: "--privileged",
            ..base_container
        },
        CreateContainer {
            pod_name: "--net=host",
            ..base_container
        },
        CreateContainer {
            image: "--privileged",
            ..base_container
        },
        CreateContainer {
            restart: "--privileged",
            ..base_container
        },
        CreateContainer {
            volumes: &bad_volumes,
            ..base_container
        },
        CreateContainer {
            options: &bad_options,
            ..base_container
        },
        CreateContainer {
            commands: &bad_commands,
            ..base_container
        },
    ] {
        assert!(create_container(&bad_container, "", true, &mut stdout, &mut stderr).is_err());
    }

    for invalid_volume_spec in [
        "",
        "relative/target",
        "../../etc/shadow:/etc/shadow",
        "/:/mnt",
        "/host:relative",
        "/host:/container:",
        "/host:/container:-ro",
        "/host:/container:ro:extra",
        "/host:/container:ro/rw",
        "/host:/container:..",
        "/host:/container:ro z",
    ] {
        let invalid_volumes = [String::from(invalid_volume_spec)];
        let invalid_request = CreateContainer {
            volumes: &invalid_volumes,
            ..base_container
        };
        assert!(
            create_container(&invalid_request, "", true, &mut stdout, &mut stderr).is_err(),
            "expected invalid volume spec {invalid_volume_spec:?} to fail"
        );
    }

    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}
