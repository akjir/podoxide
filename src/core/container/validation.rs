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

//! Container field validation, image/registry token validation, and container name resolution.

use crate::core::pod::Pod;
use crate::error::{RecipeError, RecipeErrorKind};
use crate::utilities::{collection, path, string};
use super::{Container, VolumeMount, volume};

/// Resolves a raw container name (`""`, `"*suffix"`, or `"name"`) against `pod_id` and 1-based `index`.
///
/// # Errors
///
/// Returns [`RecipeError`] with [`RecipeErrorKind::InvalidContainerName`] when `raw_name` is malformed.
pub fn resolve_container_name_value(
    raw_name: &str,
    pod_id: &str,
    index: usize,
    line: usize,
) -> Result<String, RecipeError> {
    if raw_name.is_empty() {
        if index == 0 || !string::is_valid_pod_or_container_name(pod_id) {
            return Err(RecipeError::at_line_or_none(
                RecipeErrorKind::InvalidContainerName,
                line,
                raw_name,
            ));
        }
        return Ok(format!("{pod_id}-{index}"));
    }
    if string::contains_control_or_bidi(raw_name) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidContainerName,
            line,
            raw_name,
        ));
    }
    let normalized = string::normalize_name(raw_name);
    if let Some(suffix) = normalized.strip_prefix('*') {
        if !string::is_valid_pod_or_container_name(pod_id)
            || !string::is_valid_container_suffix(suffix)
        {
            return Err(RecipeError::at_line_or_none(
                RecipeErrorKind::InvalidContainerName,
                line,
                raw_name,
            ));
        }
        return Ok(format!("{pod_id}-{suffix}"));
    }
    if !string::is_valid_pod_or_container_name(&normalized) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidContainerName,
            line,
            raw_name,
        ));
    }
    Ok(normalized)
}

/// Evaluates whether `value` is a valid container image reference without leading/trailing `/` or URL schemes.
#[must_use]
pub(crate) fn is_valid_image_value(value: &str) -> bool {
    string::is_valid_token(value)
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("//")
}

/// Evaluates whether `value` is a valid registry hostname or prefix without schemes (`://`) or leading/trailing `/`.
#[must_use]
pub(crate) fn is_valid_registry_value(value: &str) -> bool {
    string::is_valid_token(value)
        && !value.starts_with('/')
        && !value.ends_with('/')
        && !value.contains("://")
        && !value.contains("//")
}

/// Validates that all fields of `container` are well-formed.
pub(crate) fn validate_container(container: &Container) -> Result<(), RecipeError> {
    let label = container_label(&container.name);
    if !container.name.is_empty() && !string::is_valid_pod_or_container_name(&container.name) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidContainerName,
            container.line,
            &container.name,
        ));
    }
    validate_container_image(&container.image, container.line, label)?;
    validate_optional_registry(&container.registry, container.line)?;
    validate_optional_token(
        &container.restart,
        RecipeErrorKind::InvalidRestartPolicy,
        container.line,
    )?;
    validate_option_entries(&container.options, container.line)?;
    validate_text_entries(
        &container.commands,
        RecipeErrorKind::InvalidCommand,
        container.line,
    )?;
    validate_volume_mounts(&container.volumes, container.line, label)
}

/// Validates that `container` and `pod` satisfy all preconditions for `podman create`.
pub(crate) fn validate_container_create(
    container: &Container,
    pod: &Pod,
) -> Result<(), RecipeError> {
    validate_container_name(container)?;
    pod.validate_create()?;
    validate_container_create_for_validated_pod(container, pod)
}

/// Validates `container`, `pod.registry`, and relative volume base paths assuming `pod.validate_create()` already succeeded.
pub(crate) fn validate_container_create_for_validated_pod(
    container: &Container,
    pod: &Pod,
) -> Result<(), RecipeError> {
    validate_container_name(container)?;
    validate_optional_registry(&pod.registry, pod.line)?;
    validate_container(container)?;
    if volume::has_relative_host_volume(&container.volumes) && !path::is_valid_directory(&pod.path)
    {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidVolume,
            container.line,
            container_label(&container.name),
        ));
    }
    Ok(())
}

/// Validates that `container.name` is a valid non-empty Podman container name.
pub(crate) fn validate_container_name(container: &Container) -> Result<(), RecipeError> {
    if !string::is_valid_pod_or_container_name(&container.name) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidContainerName,
            container.line,
            container_label(&container.name),
        ));
    }
    Ok(())
}

/// Validates that `value` (when non-empty) is a valid registry hostname without schemes or trailing `/`.
pub(crate) fn validate_optional_registry(value: &str, line: usize) -> Result<(), RecipeError> {
    if !value.is_empty() && !is_valid_registry_value(value) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidRegistry,
            line,
            value,
        ));
    }
    Ok(())
}

/// Returns `name` when non-empty, or `"container"` as a fallback error token.
#[inline]
#[must_use]
pub(super) const fn container_label(name: &str) -> &str {
    if name.is_empty() { "container" } else { name }
}

/// Validates that `image` is non-empty and satisfies [`is_valid_image_value`].
fn validate_container_image(image: &str, line: usize, label: &str) -> Result<(), RecipeError> {
    if image.is_empty() {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::MissingContainerImage,
            line,
            label,
        ));
    }
    if !is_valid_image_value(image) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidImage,
            line,
            image,
        ));
    }
    Ok(())
}

/// Validates that every entry in `options` satisfies [`collection::is_valid_option_value`].
fn validate_option_entries(options: &[String], line: usize) -> Result<(), RecipeError> {
    for option in options {
        if !collection::is_valid_option_value(option) {
            return Err(RecipeError::at_line_or_none(
                RecipeErrorKind::InvalidOption,
                line,
                option,
            ));
        }
    }
    Ok(())
}

/// Validates that `value` (when non-empty) is a single token without leading hyphens, whitespace, or control characters.
fn validate_optional_token(
    value: &str,
    kind: RecipeErrorKind,
    line: usize,
) -> Result<(), RecipeError> {
    if !value.is_empty() && !string::is_valid_token(value) {
        return Err(RecipeError::at_line_or_none(kind, line, value));
    }
    Ok(())
}

/// Validates that every entry in `entries` satisfies [`string::is_valid_text`].
fn validate_text_entries(
    entries: &[String],
    kind: RecipeErrorKind,
    line: usize,
) -> Result<(), RecipeError> {
    for entry in entries {
        if !string::is_valid_text(entry) {
            return Err(RecipeError::at_line_or_none(kind, line, entry));
        }
    }
    Ok(())
}

/// Validates that every volume mount in `volumes` satisfies [`volume::is_valid_volume_mount`].
fn validate_volume_mounts(
    volumes: &[VolumeMount],
    line: usize,
    label: &str,
) -> Result<(), RecipeError> {
    for mount in volumes {
        if !volume::is_valid_volume_mount(mount) {
            return Err(RecipeError::at_line_or_none(
                RecipeErrorKind::InvalidVolume,
                line,
                label,
            ));
        }
    }
    Ok(())
}
