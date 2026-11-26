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

//! Unit tests for Podman CLI inspection queries in [`super`].

use std::io;

use crate::error::PodError;
use super::{
    FIELD_DELIMITER, IMAGES_FORMAT, MAX_INSPECT_RECORDS, MAX_VERSION_LINES,
    MAX_VERSION_OUTPUT_BYTES, PS_FORMAT, RECORD_DELIMITER, collect_in_use_images_with,
    container_exists_with, container_running_with, is_running_container_status,
    is_supported_version, parse_image_id_output, parse_in_use_output, pod_exists_with,
    query_container_image_id_with, query_image_id_with, query_images_with, query_ps_status_with,
};

#[test]
fn container_running_and_pod_exists_validate_names_with_mock_capture() {
    assert!(is_running_container_status("running"));
    assert!(is_running_container_status("running\n"));
    assert!(is_running_container_status("running\r\n"));
    assert!(!is_running_container_status("exited\n"));
    assert!(!is_running_container_status("running extra\n"));
    assert!(!is_running_container_status("running\nexited\n"));
    assert!(!is_running_container_status(""));

    let mut called_names: Vec<String> = Vec::new();
    let mut running_capture = |arguments: &[&str]| -> Option<String> {
        called_names.push(arguments.last().copied().unwrap_or_default().to_owned());
        Some(String::from("running\n"))
    };

    assert!(container_running_with("MyApp_1", &mut running_capture));
    assert!(container_running_with("simple-1", &mut running_capture));
    assert_eq!(called_names, vec!["MyApp_1", "simple-1"]);

    for invalid in [
        "",
        "--help",
        "-flag",
        "_leading",
        ".hidden",
        "bad name",
        "bad..container",
        "/",
        "*",
    ] {
        assert!(
            !container_running_with(invalid, &mut |_| panic!("must reject invalid name")),
            "expected container_running_with to reject {invalid:?}"
        );
        assert!(
            !container_exists_with(invalid, &mut |_| panic!("must reject invalid name")),
            "expected container_exists_with to reject {invalid:?}"
        );
    }
    assert!(!container_running_with("missing", &mut |_| None));
    assert!(container_exists_with("MyApp_1", &mut |args| {
        assert_eq!(args, &["container", "exists", "MyApp_1"]);
        Some(String::new())
    }));
    assert!(!container_exists_with("MyApp_1", &mut |_| None));

    for invalid_pod in ["", "--help", "bad name", "Upper_Case", "bad..pod"] {
        assert!(
            !pod_exists_with(invalid_pod, &mut |_| panic!("must reject invalid pod")),
            "expected pod_exists_with to reject {invalid_pod:?}"
        );
    }
    assert!(pod_exists_with("valid_pod", &mut |args| {
        assert_eq!(args, &["pod", "exists", "valid_pod"]);
        Some(String::new())
    }));
    assert!(!pod_exists_with("valid_pod", &mut |_| None));
}

#[test]
fn query_in_use_images_and_format_templates_use_atomic_ps_and_control_delimiters() {
    assert_eq!(FIELD_DELIMITER, '\x1f');
    assert_eq!(RECORD_DELIMITER, '\x1e');
    assert!(!IMAGES_FORMAT.contains(";;;"));
    assert!(!PS_FORMAT.contains(";;;"));
    assert!(IMAGES_FORMAT.contains(r#"\x1f"#));
    assert!(IMAGES_FORMAT.contains(r#"\x1e"#));
    assert!(PS_FORMAT.contains(r#"\x1f"#));
    assert!(PS_FORMAT.contains(r#"\x1e"#));
    assert!(
        PS_FORMAT.contains("{{json .Command}}"),
        "PS_FORMAT must JSON-escape Command to prevent record/field delimiter injection"
    );
    assert!(!PS_FORMAT.contains("{{.Command}}"));

    let mut captured_calls: Vec<Vec<String>> = Vec::new();
    let mut capture = |arguments: &[&str]| -> Option<String> {
        captured_calls.push(arguments.iter().map(|item| (*item).to_owned()).collect());
        match arguments.first().copied() {
            Some("ps") if arguments.contains(&"{{.ImageID}}") => Some(String::from(
                "  long2  \n\n  long2  \n--bad\ntwo words\nlong3\n",
            )),
            Some("ps") => Some(String::from("ps_row\x1e\n")),
            Some("images") => Some(String::from("img_row\x1e\n")),
            _ => None,
        }
    };

    let in_use = collect_in_use_images_with(&mut capture).unwrap();
    assert_eq!(in_use.len(), 2);
    assert!(in_use.contains("long2"));
    assert!(in_use.contains("long3"));
    assert_eq!(
        query_images_with(false, &mut capture).unwrap(),
        "img_row\x1e\n"
    );
    assert_eq!(
        query_images_with(true, &mut capture).unwrap(),
        "img_row\x1e\n"
    );
    assert_eq!(query_ps_status_with(&mut capture).unwrap(), "ps_row\x1e\n");

    assert_eq!(
        captured_calls[0],
        vec!["ps", "-a", "--no-trunc", "--format", "{{.ImageID}}"]
    );
    assert_eq!(
        captured_calls[1],
        vec![
            "images",
            "--filter",
            "dangling=true",
            "--format",
            IMAGES_FORMAT,
        ]
    );
    assert_eq!(captured_calls[2], vec!["images", "--format", IMAGES_FORMAT]);
    assert_eq!(captured_calls[3], vec!["ps", "-a", "--format", PS_FORMAT]);

    let ps_failure = collect_in_use_images_with(&mut |_: &[&str]| None).unwrap_err();
    assert!(matches!(ps_failure, PodError::Io(_)));
    assert!(
        ps_failure
            .to_string()
            .contains("failed to query in-use Podman images"),
        "unexpected error wording: {ps_failure}"
    );
    assert!(
        collect_in_use_images_with(&mut |_: &[&str]| Some(String::new()))
            .unwrap()
            .is_empty()
    );

    let mut exact_limit_ps = String::new();
    for index in 0..MAX_INSPECT_RECORDS {
        exact_limit_ps.push_str(&format!("used_{index}\n"));
    }
    exact_limit_ps.push_str("\nused_0\n");
    assert_eq!(
        parse_in_use_output(&exact_limit_ps).unwrap().len(),
        MAX_INSPECT_RECORDS
    );

    let mut oversized_ps = String::new();
    for index in 0..(MAX_INSPECT_RECORDS + 10) {
        oversized_ps.push_str(&format!("used_{index}\n"));
    }
    assert!(matches!(
        parse_in_use_output(&oversized_ps),
        Err(PodError::Io(ref error)) if error.kind() == io::ErrorKind::InvalidData
    ));

    let duplicate_flood = format!(
        "{}trailing_image\n",
        "same_image\n".repeat(MAX_INSPECT_RECORDS)
    );
    let duplicate_set = parse_in_use_output(&duplicate_flood).unwrap();
    assert_eq!(duplicate_set.len(), 2);
    assert!(duplicate_set.contains("same_image"));
    assert!(duplicate_set.contains("trailing_image"));

    assert_eq!(
        parse_image_id_output("  sha256:abcd1234  \n"),
        Some(String::from("sha256:abcd1234"))
    );
    for bad_id in ["", "   \n", "--flag\n", "two words\n", "line1\nline2\n"] {
        assert!(parse_image_id_output(bad_id).is_none());
    }

    let mut inspect_calls: Vec<Vec<String>> = Vec::new();
    let mut inspect_capture = |arguments: &[&str]| -> Option<String> {
        inspect_calls.push(arguments.iter().map(|item| (*item).to_owned()).collect());
        Some(String::from("sha256:feedface\n"))
    };
    assert_eq!(
        query_image_id_with("docker.io/alpine:latest", &mut inspect_capture),
        Some(String::from("sha256:feedface"))
    );
    assert!(query_image_id_with("--invalid", &mut inspect_capture).is_none());
    assert_eq!(inspect_calls.len(), 1);
    assert_eq!(
        inspect_calls[0],
        vec![
            "image",
            "inspect",
            "-f",
            "{{.Id}}",
            "docker.io/alpine:latest",
        ]
    );
    assert!(query_image_id_with("alpine:latest", &mut |_: &[&str]| None).is_none());

    let mut container_inspect_calls: Vec<Vec<String>> = Vec::new();
    let mut container_inspect_capture = |arguments: &[&str]| -> Option<String> {
        container_inspect_calls.push(arguments.iter().map(|item| (*item).to_owned()).collect());
        Some(String::from("sha256:oldimage\n"))
    };
    assert_eq!(
        query_container_image_id_with("stack-db", &mut container_inspect_capture),
        Some(String::from("sha256:oldimage"))
    );
    assert!(query_container_image_id_with("--invalid", &mut container_inspect_capture).is_none());
    assert_eq!(container_inspect_calls.len(), 1);
    assert_eq!(
        container_inspect_calls[0],
        vec!["container", "inspect", "-f", "{{.Image}}", "stack-db"]
    );
    assert!(query_container_image_id_with("stack-db", &mut |_: &[&str]| None).is_none());
}

#[test]
fn query_in_use_images_scans_past_infra_lines_and_fails_closed_above_limit() {
    let mut infra_plus_valid = String::new();
    for _ in 0..1200 {
        infra_plus_valid.push_str("\n   \n");
    }
    for index in 0..300 {
        infra_plus_valid.push_str(&format!("sha256:valid_{index}\n"));
    }
    let collected =
        collect_in_use_images_with(&mut |_: &[&str]| Some(infra_plus_valid.clone())).unwrap();
    assert_eq!(
        collected.len(),
        300,
        "all 300 valid image IDs past line 1024 must be collected"
    );
    assert!(collected.contains("sha256:valid_0"));
    assert!(collected.contains("sha256:valid_299"));

    let mut over_limit = String::new();
    for index in 0..=MAX_INSPECT_RECORDS {
        over_limit.push_str(&format!("sha256:distinct_{index}\n"));
    }
    let over_limit_err =
        collect_in_use_images_with(&mut |_: &[&str]| Some(over_limit.clone())).unwrap_err();
    assert!(
        matches!(
            over_limit_err,
            PodError::Io(ref error) if error.kind() == io::ErrorKind::InvalidData
        ),
        "exceeding MAX_INSPECT_RECORDS distinct IDs must fail closed, got {over_limit_err:?}"
    );
}

#[test]
fn version_parser_validates_semver_spoofing_and_bounds() {
    for supported in [
        "podman version 5.8.0\n",
        "podman version 5.8.1",
        "podman version 5.9.0-rc1",
        "podman version 5.8.0+build.123",
        "podman version 6.0.0",
        "WARN[0000] cgroup version 2 check\npodman version 5.8.0\n",
    ] {
        assert!(
            is_supported_version(supported),
            "expected supported version for {supported:?}"
        );
    }

    for unsupported in [
        "podman version 5.7.9",
        "podman version 4.9.3",
        "podman version 5.8",
        "podman version 5.8.x",
        "podman version x.8.0",
        "podman version 5.x.0",
        "podman version",
        "podman 5.8.0",
        "podman version5.8.0",
        "docker version 24.0.0",
        "podman version 5.8.0 extra",
        "podman version +5.8.0",
        "podman version 5.+8.0",
        "podman version 5.8.+0",
        "podman version 5.8.0.1",
        "podman version 5.8.0..1",
        "podman version 5.8.0bad",
        "podman version 5.8.0ä",
        "podman version 5.8.0-",
        "podman version 5.8.0+",
        "podman version 5.8.0-rc1;id",
        "podman version 5.8.99999999999999999999999999999",
        "",
    ] {
        assert!(
            !is_supported_version(unsupported),
            "expected rejection for {unsupported:?}"
        );
    }

    let exact_max_lines = format!(
        "{}podman version 5.8.0\n",
        "noise\n".repeat(MAX_VERSION_LINES - 1)
    );
    assert!(is_supported_version(&exact_max_lines));

    let too_many_lines = format!(
        "{}podman version 5.8.0\n",
        "noise\n".repeat(MAX_VERSION_LINES)
    );
    assert!(!is_supported_version(&too_many_lines));

    let version_line = "podman version 5.8.0";
    let exact_max_bytes = format!(
        "{}{version_line}",
        " ".repeat(MAX_VERSION_OUTPUT_BYTES - version_line.len())
    );
    assert!(is_supported_version(&exact_max_bytes));

    let one_over_max_bytes = format!(
        "{}{version_line}",
        " ".repeat(MAX_VERSION_OUTPUT_BYTES - version_line.len() + 1)
    );
    assert!(!is_supported_version(&one_over_max_bytes));
}
