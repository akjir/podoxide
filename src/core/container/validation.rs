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

use std::net::Ipv6Addr;

use crate::core::pod::Pod;
use crate::core::recipe::validation as recipe_validation;
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

/// Evaluates whether `value` is a valid container image reference (`[host[:port]/]repo[:tag][@digest]`).
#[must_use]
pub(crate) fn is_valid_image_value(value: &str) -> bool {
    if !string::is_valid_token(value) || value.contains("://") {
        return false;
    }
    let without_digest = match value.split_once('@') {
        Some((name_part, digest)) => {
            if !is_valid_image_digest(digest) {
                return false;
            }
            name_part
        }
        None => value,
    };
    if without_digest.is_empty() {
        return false;
    }
    let (prefix_path, final_segment) = match without_digest.rsplit_once('/') {
        Some((prefix, last)) => (Some(prefix), last),
        None => (None, without_digest),
    };
    if let Some(prefix) = prefix_path
        && !is_valid_image_prefix(prefix)
    {
        return false;
    }
    is_valid_image_leaf(final_segment)
}

/// Evaluates whether `value` is a valid registry hostname (`host[:port][/namespace...]`) without schemes or traversal segments.
#[must_use]
pub(crate) fn is_valid_registry_value(value: &str) -> bool {
    if !string::is_valid_token(value) || value.contains("://") {
        return false;
    }
    let (host_segment, namespace) = match value.split_once('/') {
        Some((host, path_suffix)) => (host, Some(path_suffix)),
        None => (value, None),
    };
    if !is_valid_registry_host(host_segment) {
        return false;
    }
    match namespace {
        Some(path_suffix) => path_suffix.split('/').all(is_valid_repo_segment),
        None => true,
    }
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

/// Evaluates whether `algorithm` is a non-empty digest algorithm identifier (such as `sha256`).
#[must_use]
fn is_valid_digest_algorithm(algorithm: &str) -> bool {
    let Some((&first, rest)) = algorithm.as_bytes().split_first() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && rest
            .iter()
            .copied()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'.' | b'_' | b'-'))
}

/// Evaluates whether `encoded` is a non-empty digest payload without delimiter characters.
#[must_use]
fn is_valid_digest_encoded(encoded: &str) -> bool {
    !encoded.is_empty()
        && encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'=' | b'_' | b'-'))
}

/// Evaluates whether `digest` is a non-empty `<algorithm>:<encoded>` OCI image digest.
#[must_use]
fn is_valid_image_digest(digest: &str) -> bool {
    let Some((algorithm, encoded)) = digest.split_once(':') else {
        return false;
    };
    is_valid_digest_algorithm(algorithm) && is_valid_digest_encoded(encoded)
}

/// Evaluates whether the terminal `<name>[:<tag>]` image segment has a valid name and non-empty tag.
#[must_use]
fn is_valid_image_leaf(segment: &str) -> bool {
    match segment.split_once(':') {
        Some((name, tag)) => is_valid_repo_segment(name) && is_valid_image_tag(tag),
        None => is_valid_repo_segment(segment),
    }
}

/// Evaluates whether the leading `/` path segments before an image leaf are non-empty and safe.
#[must_use]
fn is_valid_image_prefix(prefix: &str) -> bool {
    let mut segments = prefix.split('/');
    let Some(first) = segments.next() else {
        return false;
    };
    let first_valid = if first.contains([':', '[', ']']) {
        is_valid_registry_host(first)
    } else {
        is_valid_repo_segment(first)
    };
    first_valid && segments.all(is_valid_repo_segment)
}

/// Evaluates whether `tag` is a non-empty container image tag without separator characters.
#[must_use]
fn is_valid_image_tag(tag: &str) -> bool {
    !tag.is_empty() && tag != "." && tag != ".." && !tag.contains([':', '/', '@'])
}

/// Evaluates whether `after_open_bracket` forms `<ipv6>][:<port>]` with a valid IPv6 literal and optional `1..=65535` port.
#[must_use]
fn is_valid_ipv6_registry_host(after_open_bracket: &str) -> bool {
    let Some((address, remainder)) = after_open_bracket.split_once(']') else {
        return false;
    };
    if address.parse::<Ipv6Addr>().is_err() {
        return false;
    }
    if remainder.is_empty() {
        return true;
    }
    remainder
        .strip_prefix(':')
        .and_then(recipe_validation::parse_valid_port)
        .is_some()
}

/// Evaluates whether `host_segment` is a non-empty registry host or bracketed IPv6 literal with an optional valid `:port` (`1..=65535`).
#[must_use]
fn is_valid_registry_host(host_segment: &str) -> bool {
    if let Some(rest) = host_segment.strip_prefix('[') {
        return is_valid_ipv6_registry_host(rest);
    }
    if host_segment.contains(['[', ']']) {
        return false;
    }
    match host_segment.split_once(':') {
        Some((host, port)) => {
            is_valid_repo_segment(host)
                && !host.starts_with('-')
                && recipe_validation::parse_valid_port(port).is_some()
        }
        None => is_valid_repo_segment(host_segment) && !host_segment.starts_with('-'),
    }
}

/// Evaluates whether `segment` is a non-empty image repository or registry namespace path segment without `.`, `..`, `:`, `@`, or brackets.
#[must_use]
fn is_valid_repo_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.contains([':', '@', '[', ']'])
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
