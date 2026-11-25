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

//! Unit tests for pod upfront validation, error sanitization, and broken-pipe tolerance in [`super`].

use std::io::{self, Write};
use std::path::PathBuf;

use crate::core::container::VolumeMount;
use crate::core::log::MAX_LOG_MESSAGE_BYTES;
use crate::core::recipe::Recipe;
use crate::error::{PodError, RecipeErrorKind};
use super::lifecycle_tests::sample_multi_container_recipe;
use super::{
    Pod, create_with, down_with, pull_with, reload_with, remove_with, restart_with, start_with,
    stop_with, up_with, update_with,
};

struct BrokenPipeWriter;

impl Write for BrokenPipeWriter {
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
    }

    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
    }
}

fn assert_create_lifecycles_reject(
    recipe: &Recipe,
    expected_kind: RecipeErrorKind,
    expected_line: usize,
) {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert!(matches!(
        create_with(recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == expected_kind && error.line == Some(expected_line)
    ));
    assert!(matches!(
        up_with(recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == expected_kind && error.line == Some(expected_line)
    ));
    assert!(matches!(
        reload_with(recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == expected_kind && error.line == Some(expected_line)
    ));
    assert!(matches!(
        update_with(recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == expected_kind && error.line == Some(expected_line)
    ));
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

fn assert_invalid_pod_id_rejected(recipe: &Recipe, invalid_id: &str) {
    let mut bad_recipe = recipe.clone();
    bad_recipe.pod.id = String::from(invalid_id);
    bad_recipe.pod.line = 7;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert!(matches!(
        bad_recipe.pod.create_with(true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
    ));
    assert!(matches!(
        bad_recipe.pod.remove_with(true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
    ));
    assert!(matches!(
        start_with(&bad_recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
    ));
    assert!(matches!(
        bad_recipe.pod.stop_with(true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
    ));
    assert!(matches!(
        pull_with(&bad_recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
    ));
    assert!(matches!(
        remove_with(&bad_recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
    ));
    assert!(matches!(
        stop_with(&bad_recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
    ));
    assert!(matches!(
        down_with(&bad_recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
    ));
    assert!(matches!(
        restart_with(&bad_recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
    ));
    assert!(matches!(
        reload_with(&bad_recipe, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
    ));
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());
}

#[test]
fn pod_lifecycles_reject_invalid_fields_up_front_without_partial_execution() {
    let base_recipe = sample_multi_container_recipe();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    let mut bad_publish = base_recipe.clone();
    bad_publish.pod.line = 11;
    bad_publish.pod.publish = vec![String::from("--privileged")];
    assert_create_lifecycles_reject(&bad_publish, RecipeErrorKind::InvalidPublish, 11);

    let mut bad_option = base_recipe.clone();
    bad_option.pod.line = 13;
    bad_option.pod.options = vec![String::from("--flag\nspoof")];
    assert_create_lifecycles_reject(&bad_option, RecipeErrorKind::InvalidOption, 13);

    let mut bad_image = base_recipe.clone();
    bad_image.containers[1].line = 24;
    bad_image.containers[1].image = String::from("--help");
    assert_create_lifecycles_reject(&bad_image, RecipeErrorKind::InvalidImage, 24);
    assert!(matches!(
        pull_with(&bad_image, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidImage && error.line == Some(24)
    ));
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());

    let mut bad_name = base_recipe.clone();
    bad_name.containers[1].line = 25;
    bad_name.containers[1].name = String::from("--all");
    assert_create_lifecycles_reject(&bad_name, RecipeErrorKind::InvalidContainerName, 25);
    assert!(matches!(
        pull_with(&bad_name, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
    ));
    assert!(matches!(
        start_with(&bad_name, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
    ));
    assert!(matches!(
        stop_with(&bad_name, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
    ));
    assert!(matches!(
        remove_with(&bad_name, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
    ));
    assert!(matches!(
        down_with(&bad_name, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
    ));
    assert!(matches!(
        restart_with(&bad_name, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
    ));
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());

    let mut empty_name = base_recipe.clone();
    empty_name.containers[1].line = 26;
    empty_name.containers[1].name.clear();
    assert_create_lifecycles_reject(&empty_name, RecipeErrorKind::InvalidContainerName, 26);
    assert!(matches!(
        pull_with(&empty_name, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(26)
    ));
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());

    let mut bad_registry = base_recipe.clone();
    bad_registry.pod.line = 12;
    bad_registry.pod.registry = String::from("--tls-verify=false");
    assert_create_lifecycles_reject(&bad_registry, RecipeErrorKind::InvalidRegistry, 12);
    assert!(matches!(
        pull_with(&bad_registry, true, &mut stdout, &mut stderr),
        Err(PodError::Recipe(error))
            if error.kind == RecipeErrorKind::InvalidRegistry && error.line == Some(12)
    ));
    assert!(stdout.is_empty());
    assert!(stderr.is_empty());

    let mut bad_volume_path = base_recipe;
    bad_volume_path.pod.path = PathBuf::new();
    bad_volume_path.containers[1].line = 27;
    bad_volume_path.containers[1].volumes.push(VolumeMount {
        container_dir: String::from("/data"),
        host_dir: String::from("./data"),
        options: String::new(),
    });
    assert_create_lifecycles_reject(&bad_volume_path, RecipeErrorKind::InvalidVolume, 27);
}

#[test]
fn pod_pull_with_sanitizes_name_and_tolerates_broken_pipe() {
    let recipe = sample_multi_container_recipe();
    let mut broken_stdout = BrokenPipeWriter;
    let mut stderr = Vec::new();

    assert!(
        recipe
            .pod
            .create_with(true, &mut broken_stdout, &mut stderr)
            .is_ok()
    );
    assert!(start_with(&recipe, true, &mut broken_stdout, &mut stderr).is_ok());
    assert!(
        recipe
            .pod
            .stop_with(true, &mut broken_stdout, &mut stderr)
            .is_ok()
    );
    assert!(
        recipe
            .pod
            .remove_with(true, &mut broken_stdout, &mut stderr)
            .is_ok()
    );
    assert!(pull_with(&recipe, true, &mut broken_stdout, &mut stderr).is_ok());
    assert!(update_with(&recipe, true, &mut broken_stdout, &mut stderr).is_ok());

    let mut injected = recipe;
    injected.pod.name = format!(
        "Bad\nINFO: Spoof\r\x1b[31m\0\u{009b}\u{202E}{}",
        "ä".repeat(MAX_LOG_MESSAGE_BYTES)
    );
    let mut stdout = Vec::new();
    pull_with(&injected, true, &mut stdout, &mut stderr).unwrap();
    let output = String::from_utf8(stdout).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 7);
    let first_line = lines[0];
    assert!(first_line.starts_with("Update pod 'Bad INFO: Spoof"));
    assert!(first_line.len() <= MAX_LOG_MESSAGE_BYTES);
    assert!(!first_line.contains('\r'));
    assert!(!first_line.contains('\x1b'));
    assert!(!first_line.contains('\0'));
    assert!(!first_line.contains('\u{009b}'));
    assert!(!first_line.contains('\u{202E}'));
}

#[test]
fn pod_validation_rejects_malformed_id_publish_options_and_zero_line() {
    let recipe = sample_multi_container_recipe();
    let base_pod = recipe.pod.clone();

    for invalid_id in ["", "--all", "-f", "bad\nname", "bad\u{202E}name"] {
        assert_invalid_pod_id_rejected(&recipe, invalid_id);
    }

    for invalid_publish in [
        "",
        "--privileged",
        "-p",
        "80:80\nspoof",
        "80:80\u{202E}",
        "8080:80/BOGUS",
        "foo:bar",
    ] {
        let mut bad_pod = base_pod.clone();
        bad_pod.line = 11;
        bad_pod.publish = vec![String::from(invalid_publish)];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert!(matches!(
            bad_pod.create_with(true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPublish && error.line == Some(11)
        ));
        assert!(stdout.is_empty());
    }

    for invalid_option in [
        "",
        "   ",
        "--flag\nspoof",
        "--flag\u{202E}spoof",
        "--flag\0spoof",
    ] {
        let mut bad_pod = base_pod.clone();
        bad_pod.line = 13;
        bad_pod.options = vec![String::from(invalid_option)];
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert!(matches!(
            bad_pod.create_with(true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidOption && error.line == Some(13)
        ));
        assert!(stdout.is_empty());
    }

    let mut zero_line_pod = Pod::new();
    zero_line_pod.id = String::from("-bad");
    zero_line_pod.line = 0;
    let err = zero_line_pod.validate_create().unwrap_err();
    assert_eq!(err.line, None);
    assert!(!err.to_string().contains("line 0:"));
    assert!(!err.to_string().contains("line 1:"));
}
