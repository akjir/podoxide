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

//! Individual directive argument parsers and capacity guards for `.pod` recipe blocks.

use std::path::PathBuf;

use crate::core::container::validation as container_validation;
use crate::core::container::{self, VolumeMount};
use crate::error::{RecipeError, RecipeErrorKind};
use crate::io::psl::{PslBlock, PslDirective};
use crate::utilities::{collection, path, string};
use super::validation;

/// Maximum repeated directive or sub-block entries permitted in a single recipe block.
pub(super) const MAX_ENTRIES_PER_BLOCK: usize = 1024;
pub(super) const SEEN_COMMAND_CONTAINER: u8 = 1 << 0;
pub(super) const SEEN_COMMAND_DESCRIPTION: u8 = 1 << 1;
pub(super) const SEEN_COMMAND_EXECUTE: u8 = 1 << 2;
pub(super) const SEEN_COMMAND_USER: u8 = 1 << 3;
pub(super) const SEEN_CONTAINER_IMAGE: u8 = 1 << 0;
pub(super) const SEEN_CONTAINER_REGISTRY: u8 = 1 << 1;
pub(super) const SEEN_CONTAINER_RESTART: u8 = 1 << 2;
pub(super) const SEEN_POD_DESCRIPTION: u8 = 1 << 0;
pub(super) const SEEN_POD_NAME: u8 = 1 << 1;
pub(super) const SEEN_POD_PATH: u8 = 1 << 2;
pub(super) const SEEN_POD_REGISTRY: u8 = 1 << 3;

/// Verifies that appending `incoming` entries will not exceed [`MAX_ENTRIES_PER_BLOCK`].
pub(super) fn ensure_entry_capacity(
    current_len: usize,
    incoming: usize,
    line: usize,
    token: &str,
    kind: RecipeErrorKind,
) -> Result<(), RecipeError> {
    if current_len.saturating_add(incoming) > MAX_ENTRIES_PER_BLOCK {
        return Err(RecipeError::with_token(kind, line, token));
    }
    Ok(())
}

/// Extracts and validates the optional `<id>` argument from a `container [<id>]` block header.
pub(super) fn parse_container_name_arg(block: &PslBlock<'_>) -> Result<String, RecipeError> {
    if block.arguments.len() > 1 {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            block.line,
            block.name,
        ));
    }
    let Some(raw_argument) = block.arguments.first() else {
        return Ok(String::new());
    };
    let raw_name = raw_argument.as_ref();
    if !string::is_valid_text(raw_name) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidContainerName,
            block.line,
            raw_name,
        ));
    }
    let normalized = string::normalize_name(raw_name);
    let is_valid = match normalized.strip_prefix('*') {
        Some(suffix) => string::is_valid_container_suffix(suffix),
        None => string::is_valid_pod_or_container_name(&normalized),
    };
    if !is_valid {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidContainerName,
            block.line,
            raw_name,
        ));
    }
    Ok(normalized)
}

/// Appends `1..N` validated text arguments from `directive` into `target`.
pub(super) fn parse_multi_text_directive(
    target: &mut Vec<String>,
    directive: &PslDirective<'_>,
    invalid_kind: RecipeErrorKind,
) -> Result<(), RecipeError> {
    if directive.arguments.is_empty() {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            directive.line,
            directive.name,
        ));
    }
    ensure_entry_capacity(
        target.len(),
        directive.arguments.len(),
        directive.line,
        directive.name,
        invalid_kind,
    )?;
    let additional = collection::bounded_capacity(
        directive.arguments.len(),
        collection::MAX_COLLECTION_PREALLOC,
    );
    target.reserve(additional);
    for (index, argument) in directive.arguments.iter().enumerate() {
        let value = argument.as_ref();
        if !string::is_valid_text(value) {
            return Err(RecipeError::with_token(invalid_kind, directive.line, value)
                .with_arg_index(index + 1));
        }
        target.push(value.to_owned());
    }
    Ok(())
}

/// Appends `1..N` validated `option` directive arguments from `directive` into `target`.
pub(super) fn parse_option_directive(
    target: &mut Vec<String>,
    directive: &PslDirective<'_>,
) -> Result<(), RecipeError> {
    if directive.arguments.is_empty() {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            directive.line,
            directive.name,
        ));
    }
    ensure_entry_capacity(
        target.len(),
        directive.arguments.len(),
        directive.line,
        directive.name,
        RecipeErrorKind::InvalidOption,
    )?;
    let additional = collection::bounded_capacity(
        directive.arguments.len(),
        collection::MAX_COLLECTION_PREALLOC,
    );
    target.reserve(additional);
    for (index, argument) in directive.arguments.iter().enumerate() {
        let value = argument.as_ref();
        if !collection::is_valid_option_value(value) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidOption,
                directive.line,
                value,
            )
            .with_arg_index(index + 1));
        }
        target.push(value.to_owned());
    }
    Ok(())
}

/// Extracts, normalizes, and validates the `<id>` argument from a `pod <id>` block header.
pub(super) fn parse_pod_id_arg(block: &PslBlock<'_>) -> Result<String, RecipeError> {
    let [argument] = block.arguments.as_slice() else {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            block.line,
            block.name,
        ));
    };
    let raw_id = argument.as_ref();
    if string::contains_control_or_bidi(raw_id) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPodId,
            block.line,
            raw_id,
        ));
    }
    let normalized = string::normalize_name(raw_id);
    if !string::is_valid_pod_or_container_name(&normalized) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPodId,
            block.line,
            raw_id,
        ));
    }
    Ok(normalized)
}

/// Marks `SEEN_POD_NAME` as seen and parses the `pod.name` display name directive.
pub(super) fn parse_pod_name_directive(
    directive: &PslDirective<'_>,
    seen: &mut u8,
) -> Result<String, RecipeError> {
    mark_seen(seen, SEEN_POD_NAME, directive)?;
    let value = parse_single_argument(directive)?;
    if !string::is_valid_text(value) {
        let token = if value.is_empty() {
            directive.name
        } else {
            value
        };
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPodName,
            directive.line,
            token,
        ));
    }
    Ok(value.trim().to_owned())
}

/// Marks `SEEN_POD_PATH` as seen and parses the `pod.path` directory path directive.
pub(super) fn parse_pod_path_directive(
    directive: &PslDirective<'_>,
    seen: &mut u8,
) -> Result<PathBuf, RecipeError> {
    mark_seen(seen, SEEN_POD_PATH, directive)?;
    let value = parse_single_argument(directive)?;
    if !path::is_valid_directory_path(value) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPath,
            directive.line,
            value,
        ));
    }
    Ok(PathBuf::from(value))
}

/// Parses and validates a `publish` directive (`1..=3` arguments) into a `--publish` value string.
pub(super) fn parse_publish_directive(directive: &PslDirective<'_>) -> Result<String, RecipeError> {
    match directive.arguments.as_slice() {
        [single] => {
            let value = single.as_ref();
            validation::normalize_publish_spec(value)
                .ok_or_else(|| invalid_publish_error(value, 1, directive))
        }
        [first, second] => format_two_arg_publish(first.as_ref(), second.as_ref(), directive),
        [first, second, third] => {
            format_three_arg_publish(first.as_ref(), second.as_ref(), third.as_ref(), directive)
        }
        _ => Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            directive.line,
            directive.name,
        )),
    }
}

/// Marks `SEEN_CONTAINER_IMAGE` as seen and validates a container `image` reference.
pub(super) fn parse_unique_image_directive(
    directive: &PslDirective<'_>,
    seen: &mut u8,
) -> Result<String, RecipeError> {
    mark_seen(seen, SEEN_CONTAINER_IMAGE, directive)?;
    let value = parse_single_argument(directive)?;
    if !container_validation::is_valid_image_value(value) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidImage,
            directive.line,
            value,
        ));
    }
    Ok(value.to_owned())
}

/// Marks `flag` as seen and validates a `registry` hostname without URL schemes or trailing `/`.
pub(super) fn parse_unique_registry_directive(
    directive: &PslDirective<'_>,
    seen: &mut u8,
    flag: u8,
) -> Result<String, RecipeError> {
    mark_seen(seen, flag, directive)?;
    let value = parse_single_argument(directive)?;
    if !container_validation::is_valid_registry_value(value) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidRegistry,
            directive.line,
            value,
        ));
    }
    Ok(value.to_owned())
}

/// Marks `flag` as seen and parses a single-argument text directive.
pub(super) fn parse_unique_text_directive(
    directive: &PslDirective<'_>,
    seen: &mut u8,
    flag: u8,
    invalid_kind: RecipeErrorKind,
) -> Result<String, RecipeError> {
    mark_seen(seen, flag, directive)?;
    parse_text_directive(directive, invalid_kind)
}

/// Marks `flag` as seen and parses a single-argument token directive.
pub(super) fn parse_unique_token_directive(
    directive: &PslDirective<'_>,
    seen: &mut u8,
    flag: u8,
    invalid_kind: RecipeErrorKind,
) -> Result<String, RecipeError> {
    mark_seen(seen, flag, directive)?;
    parse_token_directive(directive, invalid_kind)
}

/// Parses and validates a `volume` directive (`2..=3` arguments) into a [`VolumeMount`].
pub(super) fn parse_volume_directive(
    directive: &PslDirective<'_>,
) -> Result<VolumeMount, RecipeError> {
    let volume = match directive.arguments.as_slice() {
        [host_dir, container_dir] => VolumeMount {
            container_dir: container_dir.as_ref().to_owned(),
            host_dir: host_dir.as_ref().to_owned(),
            options: String::new(),
        },
        [host_dir, container_dir, options] => VolumeMount {
            container_dir: container_dir.as_ref().to_owned(),
            host_dir: host_dir.as_ref().to_owned(),
            options: options.as_ref().to_owned(),
        },
        _ => {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidArgumentCount,
                directive.line,
                directive.name,
            ));
        }
    };
    if !container::is_valid_volume_mount(&volume) {
        return Err(invalid_volume_error(&volume, directive));
    }
    Ok(volume)
}

/// Validates and formats a 3-argument `publish` directive into `<host>:<container>/<TCP|UDP>`.
fn format_three_arg_publish(
    first: &str,
    second: &str,
    third: &str,
    directive: &PslDirective<'_>,
) -> Result<String, RecipeError> {
    if !validation::is_valid_host_bind_spec(first) {
        return Err(invalid_publish_error(first, 1, directive));
    }
    if !validation::is_valid_port_or_range(second) {
        return Err(invalid_publish_error(second, 2, directive));
    }
    let Some(protocol) = validation::normalize_protocol(third) else {
        return Err(invalid_publish_error(third, 3, directive));
    };
    Ok(format!("{first}:{second}/{protocol}"))
}

/// Validates and formats a 2-argument `publish` directive into a single Podman `--publish` string.
fn format_two_arg_publish(
    first: &str,
    second: &str,
    directive: &PslDirective<'_>,
) -> Result<String, RecipeError> {
    if first.is_empty() {
        return validation::normalize_publish_spec(second)
            .ok_or_else(|| invalid_publish_error(second, 2, directive));
    }
    if let Some(protocol) = validation::normalize_protocol(second) {
        if !validation::is_valid_publish_endpoint(first) {
            return Err(invalid_publish_error(first, 1, directive));
        }
        return Ok(format!("{first}/{protocol}"));
    }
    if !validation::is_valid_host_bind_spec(first) {
        return Err(invalid_publish_error(first, 1, directive));
    }
    if !validation::is_valid_port_or_range(second) {
        return Err(invalid_publish_error(second, 2, directive));
    }
    Ok(format!("{first}:{second}"))
}

/// Builds an [`RecipeErrorKind::InvalidPublish`] error pinned to `arg_index` when `candidate` is non-empty.
#[must_use]
fn invalid_publish_error(
    candidate: &str,
    arg_index: usize,
    directive: &PslDirective<'_>,
) -> RecipeError {
    if candidate.is_empty() {
        return RecipeError::with_token(
            RecipeErrorKind::InvalidPublish,
            directive.line,
            directive.name,
        );
    }
    RecipeError::with_token(RecipeErrorKind::InvalidPublish, directive.line, candidate)
        .with_arg_index(arg_index)
}

/// Builds an [`RecipeErrorKind::InvalidVolume`] error highlighting the exact offending argument position.
#[must_use]
fn invalid_volume_error(volume: &VolumeMount, directive: &PslDirective<'_>) -> RecipeError {
    if !volume.host_dir.is_empty() && !path::is_valid_directory_path(&volume.host_dir) {
        return RecipeError::with_token(
            RecipeErrorKind::InvalidVolume,
            directive.line,
            &volume.host_dir,
        )
        .with_arg_index(1);
    }
    if !volume.container_dir.starts_with('/')
        || !path::is_valid_directory_path(&volume.container_dir)
    {
        if volume.container_dir.is_empty() {
            return RecipeError::with_token(
                RecipeErrorKind::InvalidVolume,
                directive.line,
                directive.name,
            );
        }
        return RecipeError::with_token(
            RecipeErrorKind::InvalidVolume,
            directive.line,
            &volume.container_dir,
        )
        .with_arg_index(2);
    }
    RecipeError::with_token(
        RecipeErrorKind::InvalidVolume,
        directive.line,
        &volume.options,
    )
    .with_arg_index(3)
}

/// Sets `flag` in `seen` for `directive` or returns [`RecipeErrorKind::DuplicateDirective`].
fn mark_seen(seen: &mut u8, flag: u8, directive: &PslDirective<'_>) -> Result<(), RecipeError> {
    if *seen & flag != 0 {
        return Err(RecipeError::with_token(
            RecipeErrorKind::DuplicateDirective,
            directive.line,
            directive.name,
        ));
    }
    *seen |= flag;
    Ok(())
}

/// Extracts the sole argument of a directive or returns [`RecipeErrorKind::InvalidArgumentCount`].
fn parse_single_argument<'a>(directive: &'a PslDirective<'a>) -> Result<&'a str, RecipeError> {
    let [argument] = directive.arguments.as_slice() else {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            directive.line,
            directive.name,
        ));
    };
    Ok(argument.as_ref())
}

/// Parses a single-argument text directive, rejecting empty strings and control/BiDi characters.
fn parse_text_directive(
    directive: &PslDirective<'_>,
    invalid_kind: RecipeErrorKind,
) -> Result<String, RecipeError> {
    let value = parse_single_argument(directive)?;
    if !string::is_valid_text(value) {
        return Err(RecipeError::with_token(invalid_kind, directive.line, value));
    }
    Ok(value.to_owned())
}

/// Parses a single-argument token directive (`restart`, `user`).
fn parse_token_directive(
    directive: &PslDirective<'_>,
    invalid_kind: RecipeErrorKind,
) -> Result<String, RecipeError> {
    let value = parse_single_argument(directive)?;
    if !string::is_valid_token(value) {
        return Err(RecipeError::with_token(invalid_kind, directive.line, value));
    }
    Ok(value.to_owned())
}
