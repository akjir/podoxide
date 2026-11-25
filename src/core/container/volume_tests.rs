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

//! Unit tests for container volume formatting, host path resolution, and option token splitting in [`super`].

use std::borrow::Cow;
use std::path::Path;

use crate::error::RecipeErrorKind;
use crate::utilities::collection::append_option_tokens;
use super::volume::resolve_host_volume_path;
use super::{VolumeMount, format_volume_mount};

#[test]
fn container_volume_formatting_and_options() {
    let pod_path = Path::new("/my_pod_path");
    assert!(matches!(
        resolve_host_volume_path("/absolute/path", pod_path, 1, "app").unwrap(),
        Cow::Borrowed("/absolute/path")
    ));
    for (raw_host, base, expected) in [
        ("data", "pods", "./pods/data"),
        (".", ".", "."),
        ("data", "/my_pod_path/", "/my_pod_path/data"),
        (".", "/my_pod_path/", "/my_pod_path"),
        (".//data/", "/my_pod_path", "/my_pod_path/data"),
        (".//./data", "/my_pod_path", "/my_pod_path/data"),
        (".//.", "/my_pod_path", "/my_pod_path"),
        (".//", "/my_pod_path", "/my_pod_path"),
    ] {
        assert_eq!(
            resolve_host_volume_path(raw_host, Path::new(base), 1, "app").unwrap(),
            Cow::Owned::<str>(String::from(expected)),
            "host={raw_host}, base={base}"
        );
    }

    let cases = [
        (
            "",
            "/container/dir/anonymous",
            "",
            "/container/dir/anonymous",
        ),
        (
            "named_volume",
            "/container/dir/named",
            "ro",
            "/my_pod_path/named_volume:/container/dir/named:ro",
        ),
        (
            "/absolute/path",
            "/container/dir/absolute",
            "",
            "/absolute/path:/container/dir/absolute",
        ),
        (
            "./relative/path",
            "/container/dir/relative",
            "z",
            "/my_pod_path/relative/path:/container/dir/relative:z",
        ),
        (
            "././relative/path",
            "/container/dir/repeated_dot",
            "ro",
            "/my_pod_path/relative/path:/container/dir/repeated_dot:ro",
        ),
        (
            ".relative/path2",
            "/container/dir/relative2",
            "",
            "/my_pod_path/.relative/path2:/container/dir/relative2",
        ),
        (
            "relative/path3",
            "/container/dir/relative3",
            "",
            "/my_pod_path/relative/path3:/container/dir/relative3",
        ),
        (
            ".config",
            "/container/dir/hidden1",
            "",
            "/my_pod_path/.config:/container/dir/hidden1",
        ),
        (
            "./.config2",
            "/container/dir/hidden2",
            "",
            "/my_pod_path/.config2:/container/dir/hidden2",
        ),
        (
            ".",
            "/container/dir/root",
            "ro",
            "/my_pod_path:/container/dir/root:ro",
        ),
        (
            "./.",
            "/container/dir/dot_root",
            "",
            "/my_pod_path:/container/dir/dot_root",
        ),
        (
            "./my dir",
            "/container/my dir",
            "ro,Z",
            "/my_pod_path/my dir:/container/my dir:ro,Z",
        ),
    ];

    for (host_dir, container_dir, options, expected) in cases {
        let volume = VolumeMount {
            container_dir: String::from(container_dir),
            host_dir: String::from(host_dir),
            options: String::from(options),
        };
        let formatted = format_volume_mount(&volume, pod_path, 1, "volumes-1").unwrap();
        assert_eq!(formatted, expected);
    }

    let relative_volume = VolumeMount {
        container_dir: String::from("/container/dir/data"),
        host_dir: String::from("./data"),
        options: String::from("ro"),
    };
    for invalid_pod_path in [Path::new(""), Path::new("/pods:ro"), Path::new("../escape")] {
        let error = format_volume_mount(&relative_volume, invalid_pod_path, 9, "app").unwrap_err();
        assert_eq!(error.kind, RecipeErrorKind::InvalidVolume);
        assert_eq!(error.line, Some(9));
    }

    let mut options = Vec::new();
    append_option_tokens(
        &mut options,
        &[
            String::from("--unknown thing"),
            String::from("  --trimmed flag  "),
            String::from("--another"),
            String::from("unknown value"),
        ],
    );
    assert_eq!(
        options,
        vec![
            "--unknown",
            "thing",
            "--trimmed",
            "flag",
            "--another",
            "unknown value",
        ]
    );
}
