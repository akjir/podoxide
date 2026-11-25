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

//! Unit tests for container validation and container name resolution in [`super`].

use std::path::{Path, PathBuf};

use crate::core::pod::Pod;
use crate::error::{PodError, RecipeErrorKind};
use super::validation::container_label;
use super::{
    Container, VolumeMount, create_with, format_volume_mount, is_valid_volume_mount, pull_with,
    reload_with, remove_with, resolve_container_name_value, start_with, stop_with, update_with,
};

#[test]
fn container_and_volume_validation() {
    let mut container = Container::new();
    container.name = String::from("app");
    container.line = 12;

    let missing_image_error = container.validate().unwrap_err();
    assert_eq!(
        missing_image_error.kind,
        RecipeErrorKind::MissingContainerImage
    );

    for bad_image in ["-invalid:latest", "/alpine:latest", "alpine:latest/"] {
        container.image = String::from(bad_image);
        assert_eq!(
            container.validate().unwrap_err().kind,
            RecipeErrorKind::InvalidImage,
            "expected InvalidImage for {bad_image:?}"
        );
    }

    container.image = String::from("alpine:latest");
    assert!(container.validate().is_ok());

    let valid_anonymous = VolumeMount {
        container_dir: String::from("/container/dir/anonymous"),
        host_dir: String::new(),
        options: String::new(),
    };
    assert!(is_valid_volume_mount(&valid_anonymous));

    let valid_named = VolumeMount {
        container_dir: String::from("/container/dir/named"),
        host_dir: String::from("named_volume"),
        options: String::from("ro,Z"),
    };
    assert!(is_valid_volume_mount(&valid_named));

    let empty_target = VolumeMount::new();
    assert!(!is_valid_volume_mount(&empty_target));
    container.volumes.push(empty_target);
    assert_eq!(
        container.validate().unwrap_err().kind,
        RecipeErrorKind::InvalidVolume
    );

    for invalid_volume in [
        VolumeMount {
            container_dir: String::from("my_vol"),
            host_dir: String::new(),
            options: String::new(),
        },
        VolumeMount {
            container_dir: String::from("/etc"),
            host_dir: String::new(),
            options: String::from("/mnt"),
        },
        VolumeMount {
            container_dir: String::from("/data"),
            host_dir: String::new(),
            options: String::from("../../etc/shadow"),
        },
        VolumeMount {
            container_dir: String::from("/data"),
            host_dir: String::from("../secret"),
            options: String::new(),
        },
        VolumeMount {
            container_dir: String::from("/data"),
            host_dir: String::from("./data"),
            options: String::from("ro:rw"),
        },
        VolumeMount {
            container_dir: String::from("/data"),
            host_dir: String::from("./data"),
            options: String::from("-ro"),
        },
        VolumeMount {
            container_dir: String::from("/data"),
            host_dir: String::new(),
            options: String::from("ro"),
        },
    ] {
        assert!(!is_valid_volume_mount(&invalid_volume));
        assert_eq!(
            format_volume_mount(&invalid_volume, Path::new("/pods"), 4, "app")
                .unwrap_err()
                .kind,
            RecipeErrorKind::InvalidVolume
        );
    }
}

#[test]
fn container_defaults_and_name_resolution() {
    let mut container = Container::new();
    assert!(container.name.is_empty());
    assert_eq!(container_label(&container.name), "container");

    let unnamed_missing_image = container.validate().unwrap_err();
    assert_eq!(
        unnamed_missing_image.kind,
        RecipeErrorKind::MissingContainerImage
    );
    assert_eq!(unnamed_missing_image.line, None);
    assert!(!unnamed_missing_image.to_string().contains("line 0:"));
    assert_eq!(unnamed_missing_image.token.as_deref(), Some("container"));

    assert_eq!(
        container.ensure_name("", 1).unwrap_err().kind,
        RecipeErrorKind::InvalidContainerName
    );
    assert_eq!(
        container.ensure_name("--bad", 1).unwrap_err().kind,
        RecipeErrorKind::InvalidContainerName
    );
    assert_eq!(
        container.ensure_name("my_pod", 0).unwrap_err().kind,
        RecipeErrorKind::InvalidContainerName
    );

    container.ensure_name("my_pod", 1).unwrap();
    assert_eq!(container.name, "my_pod-1");
    assert_eq!(container_label(&container.name), "my_pod-1");

    assert_eq!(
        resolve_container_name_value("  supR App ", "si_po", 1, 5).unwrap(),
        "supr_app"
    );
    assert_eq!(
        resolve_container_name_value(" *Relative ", "con_name", 2, 8).unwrap(),
        "con_name-relative"
    );
    assert_eq!(
        resolve_container_name_value("*_db", "con_name", 2, 8).unwrap(),
        "con_name-_db"
    );

    for invalid_pod_id in ["", "--bad", "-p", "_internal", "bad\npod"] {
        assert_eq!(
            resolve_container_name_value("", invalid_pod_id, 1, 6)
                .unwrap_err()
                .kind,
            RecipeErrorKind::InvalidContainerName
        );
        assert_eq!(
            resolve_container_name_value("*web", invalid_pod_id, 1, 6)
                .unwrap_err()
                .kind,
            RecipeErrorKind::InvalidContainerName
        );
    }

    for invalid in [
        "*",
        "*-bad",
        "*.bad",
        "-bad",
        "bad/name",
        "bad\nname",
        "bad\u{202E}name",
    ] {
        let error = resolve_container_name_value(invalid, "pod", 1, 10).unwrap_err();
        assert_eq!(error.kind, RecipeErrorKind::InvalidContainerName);
        assert_eq!(error.line, Some(10));
    }
}

#[test]
fn container_validate_and_lifecycle_reject_malformed_fields() {
    let mut pod = Pod::new();
    pod.id = String::from("app_pod");
    pod.path = PathBuf::from("/pods/app_pod");

    let mut base = Container::new();
    base.name = String::from("app_1");
    base.image = String::from("alpine:latest");
    base.line = 15;

    assert!(base.validate_create(&pod).is_ok());
    let mut no_restart_out = Vec::new();
    create_with(&base, &pod, true, &mut no_restart_out, &mut Vec::new()).unwrap();
    assert!(
        !String::from_utf8(no_restart_out)
            .unwrap()
            .contains("--restart")
    );

    let mut bad_name = base.clone();
    bad_name.name = String::from("--all");
    assert_eq!(
        bad_name.validate().unwrap_err().kind,
        RecipeErrorKind::InvalidContainerName
    );

    for invalid_registry in ["--privileged", "docker.io/", "https://docker.io"] {
        let mut bad_registry = base.clone();
        bad_registry.registry = String::from(invalid_registry);
        assert_eq!(
            bad_registry.validate().unwrap_err().kind,
            RecipeErrorKind::InvalidRegistry
        );
    }

    let mut bad_restart = base.clone();
    bad_restart.restart = String::from("--privileged");
    assert_eq!(
        bad_restart.validate().unwrap_err().kind,
        RecipeErrorKind::InvalidRestartPolicy
    );

    for invalid_option in ["", "   ", "--env\nBAD=1", "--env\u{202E}BAD=1"] {
        let mut bad_option = base.clone();
        bad_option.options = vec![String::from(invalid_option)];
        assert_eq!(
            bad_option.validate().unwrap_err().kind,
            RecipeErrorKind::InvalidOption
        );
    }

    for invalid_command in ["", "   ", "sh\0-c", "sh\u{202E}-c"] {
        let mut bad_command = base.clone();
        bad_command.commands = vec![String::from(invalid_command)];
        assert_eq!(
            bad_command.validate().unwrap_err().kind,
            RecipeErrorKind::InvalidCommand
        );
    }

    for invalid_name in ["", "--all", "-f", "bad\nname"] {
        let mut target = base.clone();
        target.name = String::from(invalid_name);
        assert_eq!(
            target.validate_create(&pod).unwrap_err().kind,
            RecipeErrorKind::InvalidContainerName
        );
        assert!(matches!(
            create_with(&target, &pod, true, &mut Vec::new(), &mut Vec::new()),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidContainerName
        ));
        assert!(matches!(
            pull_with(&target, &pod, "", true, &mut Vec::new(), &mut Vec::new()),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidContainerName
        ));
        assert!(matches!(
            start_with(&target, true, &mut Vec::new(), &mut Vec::new()),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidContainerName
        ));
        assert!(matches!(
            stop_with(&target, true, &mut Vec::new(), &mut Vec::new()),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidContainerName
        ));
        assert!(matches!(
            remove_with(&target, true, &mut Vec::new(), &mut Vec::new()),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidContainerName
        ));
    }

    let mut bad_pod_id = pod.clone();
    bad_pod_id.id = String::from("--all");
    assert_eq!(
        base.validate_create(&bad_pod_id).unwrap_err().kind,
        RecipeErrorKind::InvalidPodId
    );
    assert!(matches!(
        create_with(&base, &bad_pod_id, true, &mut Vec::new(), &mut Vec::new()),
        Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidPodId
    ));

    let mut bad_pod_publish = pod.clone();
    bad_pod_publish.publish = vec![String::from("--privileged")];
    assert_eq!(
        base.validate_create(&bad_pod_publish).unwrap_err().kind,
        RecipeErrorKind::InvalidPublish
    );

    let mut bad_pod_option = pod.clone();
    bad_pod_option.options = vec![String::from("--flag\nspoof")];
    assert_eq!(
        base.validate_create(&bad_pod_option).unwrap_err().kind,
        RecipeErrorKind::InvalidOption
    );

    let mut bad_pod_registry = pod.clone();
    bad_pod_registry.registry = String::from("--privileged");
    assert_eq!(
        base.validate_create(&bad_pod_registry).unwrap_err().kind,
        RecipeErrorKind::InvalidRegistry
    );
    assert!(matches!(
        create_with(&base, &bad_pod_registry, true, &mut Vec::new(), &mut Vec::new()),
        Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidRegistry
    ));
    assert!(matches!(
        pull_with(&base, &bad_pod_registry, "", true, &mut Vec::new(), &mut Vec::new()),
        Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidRegistry
    ));
    assert!(matches!(
        update_with(&base, &bad_pod_registry, true, &mut Vec::new(), &mut Vec::new()),
        Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidRegistry
    ));

    let mut with_relative_vol = base;
    with_relative_vol.volumes.push(VolumeMount {
        container_dir: String::from("/data"),
        host_dir: String::from("./data"),
        options: String::new(),
    });
    let mut bad_pod_path = pod;
    bad_pod_path.path = PathBuf::from("../escape");
    assert_eq!(
        with_relative_vol
            .validate_create(&bad_pod_path)
            .unwrap_err()
            .kind,
        RecipeErrorKind::InvalidVolume
    );
    assert!(matches!(
        reload_with(
            &with_relative_vol,
            &bad_pod_path,
            true,
            &mut Vec::new(),
            &mut Vec::new(),
        ),
        Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidVolume
    ));
    assert!(matches!(
        update_with(
            &with_relative_vol,
            &bad_pod_path,
            true,
            &mut Vec::new(),
            &mut Vec::new(),
        ),
        Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidVolume
    ));
}
