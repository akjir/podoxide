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

//! Semantic validation for parsed `.pod` recipes, port publish specifications, pod options, and commands.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{Ipv4Addr, Ipv6Addr};

use crate::core::config::Config;
use crate::core::container::Container;
use crate::core::container::validation as container_validation;
use crate::core::log;
use crate::core::pod::{Pod, PodCommand};
use crate::error::{RecipeError, RecipeErrorKind};
use crate::utilities::{collection, path, string};
use super::Recipe;

/// Parses `value` as a decimal TCP/UDP port number in `1..=65535`.
#[must_use]
pub(crate) fn parse_valid_port(value: &str) -> Option<u16> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse::<u16>().ok().filter(|&port| port > 0)
}

/// Validates that every `pod.options` entry is a safe option argument.
pub(crate) fn validate_pod_options(options: &[String], line: usize) -> Result<(), RecipeError> {
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

/// Validates that every `pod.publish` entry is a well-formed Podman `--publish` specification.
pub(crate) fn validate_pod_publish(publish: &[String], line: usize) -> Result<(), RecipeError> {
    for entry in publish {
        if !is_valid_publish_spec(entry) {
            return Err(RecipeError::at_line_or_none(
                RecipeErrorKind::InvalidPublish,
                line,
                entry,
            ));
        }
    }
    Ok(())
}

/// Evaluates whether `value` is a valid host bind specification (`<port[-port]>` or `<ip>:[<port[-port]>]`).
#[must_use]
pub(super) fn is_valid_host_bind_spec(value: &str) -> bool {
    if !string::is_valid_token(value) {
        return false;
    }
    if let Some((ip_part, port_part)) = value.rsplit_once(':') {
        return is_valid_host_ip(ip_part)
            && (port_part.is_empty() || is_valid_port_or_range(port_part));
    }
    is_valid_port_or_range(value)
}

/// Evaluates whether `value` is a numeric port (`1..=65535`) or ascending port range (`<start>-<end>` with `start <= end`).
#[must_use]
pub(super) fn is_valid_port_or_range(value: &str) -> bool {
    if let Some((start, end)) = value.split_once('-') {
        return matches!(
            (parse_valid_port(start), parse_valid_port(end)),
            (Some(start_port), Some(end_port)) if start_port <= end_port
        );
    }
    parse_valid_port(value).is_some()
}

/// Evaluates whether `endpoint` is a valid publish endpoint without a `/<proto>` suffix.
#[must_use]
pub(super) fn is_valid_publish_endpoint(endpoint: &str) -> bool {
    if !string::is_valid_token(endpoint) || endpoint.contains('/') {
        return false;
    }
    if let Some((host_part, container_port)) = endpoint.rsplit_once(':') {
        return is_valid_host_bind_spec(host_part) && is_valid_port_or_range(container_port);
    }
    is_valid_port_or_range(endpoint)
}

/// Normalizes a case-insensitive publish protocol (`TCP` or `UDP`) to uppercase.
#[must_use]
pub(super) fn normalize_protocol(value: &str) -> Option<&'static str> {
    if value.eq_ignore_ascii_case("TCP") {
        return Some("TCP");
    }
    if value.eq_ignore_ascii_case("UDP") {
        return Some("UDP");
    }
    None
}

/// Validates a full `--publish` specification and normalizes any `/<proto>` suffix to uppercase.
#[must_use]
pub(super) fn normalize_publish_spec(spec: &str) -> Option<String> {
    if !string::is_valid_token(spec) {
        return None;
    }
    if let Some((endpoint, raw_protocol)) = spec.split_once('/') {
        let protocol = normalize_protocol(raw_protocol)?;
        if !is_valid_publish_endpoint(endpoint) {
            return None;
        }
        return Some(format!("{endpoint}/{protocol}"));
    }
    is_valid_publish_endpoint(spec).then(|| spec.to_owned())
}

/// Validates required pod and container fields, applies default pod paths, and resolves container names.
pub(super) fn validate_recipe_inner(
    recipe: &mut Recipe,
    config: &Config,
    file_name: &str,
) -> Result<(), RecipeError> {
    if !recipe.has_pod || recipe.pod.id.is_empty() {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::MissingPodBlock,
            recipe.pod.line,
            file_name,
        ));
    }
    validate_pod_fields(&mut recipe.pod, config, file_name)?;
    if recipe.containers.is_empty() {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::MissingContainersBlock,
            recipe.pod.line,
            file_name,
        ));
    }
    validate_and_resolve_containers(&mut recipe.containers, &recipe.pod.id)
}

/// Evaluates whether `value` is a valid IPv4 literal or bracketed IPv6 literal (`[addr]`).
#[must_use]
fn is_valid_host_ip(value: &str) -> bool {
    if let Some(ipv6) = value
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    {
        return ipv6.parse::<Ipv6Addr>().is_ok();
    }
    value.parse::<Ipv4Addr>().is_ok()
}

/// Evaluates whether `spec` is a complete Podman `--publish` specification string.
#[must_use]
fn is_valid_publish_spec(spec: &str) -> bool {
    if !string::is_valid_token(spec) {
        return false;
    }
    if let Some((endpoint, protocol)) = spec.split_once('/') {
        return normalize_protocol(protocol).is_some() && is_valid_publish_endpoint(endpoint);
    }
    is_valid_publish_endpoint(spec)
}

/// Validates `pod.path` or populates it from `<config.pods.path>/<pod.id>` when omitted.
fn resolve_pod_path(pod: &mut Pod, config: &Config, file_name: &str) -> Result<(), RecipeError> {
    let used_fallback = pod.path.as_os_str().is_empty();
    if used_fallback {
        if config.pods.path.as_os_str().is_empty() {
            return Err(RecipeError::at_line_or_none(
                RecipeErrorKind::MissingPodPath,
                pod.line,
                file_name,
            ));
        }
        if !path::is_valid_directory(&config.pods.path) {
            return Err(RecipeError::at_line_or_none(
                RecipeErrorKind::InvalidPath,
                pod.line,
                &path::sanitize_path_token(&config.pods.path),
            ));
        }
        pod.path = path::build_full_path(&config.pods.path, &pod.id, "");
    }
    if !path::is_valid_directory(&pod.path) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidPath,
            pod.line,
            &path::sanitize_path_token(&pod.path),
        ));
    }
    if used_fallback && log::is_debug_enabled() {
        log::debug(&format!(
            "No pod path in recipe '{file_name}' set. Path '{}' used.",
            pod.path.display()
        ));
    }
    Ok(())
}

/// Resolves container names, validates each container, and rejects duplicate container names.
fn validate_and_resolve_containers(
    containers: &mut [Container],
    pod_id: &str,
) -> Result<(), RecipeError> {
    let mut seen_names = BTreeSet::new();
    for (index, container) in containers.iter_mut().enumerate() {
        container.ensure_name(pod_id, index + 1)?;
        container.validate()?;
        if !seen_names.insert(container.name.as_str()) {
            return Err(RecipeError::at_line_or_none(
                RecipeErrorKind::InvalidContainerName,
                container.line,
                &container.name,
            ));
        }
    }
    Ok(())
}

/// Validates a single [`PodCommand`] entry inside `pod.commands`, requiring non-empty `container` and `execute`.
fn validate_pod_command(
    key: &str,
    command: &PodCommand,
    pod_line: usize,
) -> Result<(), RecipeError> {
    let line = if command.line == 0 {
        pod_line
    } else {
        command.line
    };
    if !string::is_valid_identifier(key) || key != command.name {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            key,
        ));
    }
    if command.container.is_empty() {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            key,
        ));
    }
    if !string::is_valid_text(&command.container) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            &command.container,
        ));
    }
    if command.execute.is_empty() {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            key,
        ));
    }
    if !string::is_valid_text(&command.execute) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            &command.execute,
        ));
    }
    if !command.description.is_empty() && !string::is_valid_text(&command.description) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            &command.description,
        ));
    }
    if let Some(user) = &command.user
        && !string::is_valid_token(user)
    {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            user,
        ));
    }
    Ok(())
}

/// Validates all [`PodCommand`] entries in `pod.commands`.
fn validate_pod_commands(
    commands: &BTreeMap<String, PodCommand>,
    pod_line: usize,
) -> Result<(), RecipeError> {
    for (key, command) in commands {
        validate_pod_command(key, command, pod_line)?;
    }
    Ok(())
}

/// Validates pod fields (`id`, `name`, `description`, `registry`, `commands`, `publish`, `options`, and `path`).
fn validate_pod_fields(pod: &mut Pod, config: &Config, file_name: &str) -> Result<(), RecipeError> {
    if string::contains_control_or_bidi(&pod.id) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidPodId,
            pod.line,
            &pod.id,
        ));
    }
    let normalized_id = string::normalize_name(&pod.id);
    if !string::is_valid_pod_or_container_name(&normalized_id) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidPodId,
            pod.line,
            &pod.id,
        ));
    }
    pod.id = normalized_id;

    validate_pod_name(pod, file_name)?;

    if !pod.description.is_empty() && !string::is_valid_text(&pod.description) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidDescription,
            pod.line,
            &pod.description,
        ));
    }
    if pod.registry.is_empty() {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::MissingPodRegistry,
            pod.line,
            file_name,
        ));
    }
    if !container_validation::is_valid_registry_value(&pod.registry) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidRegistry,
            pod.line,
            &pod.registry,
        ));
    }

    validate_pod_commands(&pod.commands, pod.line)?;
    validate_pod_publish(&pod.publish, pod.line)?;
    validate_pod_options(&pod.options, pod.line)?;
    resolve_pod_path(pod, config, file_name)
}

/// Validates and trims `pod.name`, distinguishing missing names from whitespace/control-character names.
fn validate_pod_name(pod: &mut Pod, file_name: &str) -> Result<(), RecipeError> {
    if pod.name.is_empty() {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::MissingPodName,
            pod.line,
            file_name,
        ));
    }
    if !string::is_valid_text(&pod.name) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidPodName,
            pod.line,
            &pod.name,
        ));
    }
    let trimmed_name = pod.name.trim();
    if pod.name.len() != trimmed_name.len() {
        pod.name = trimmed_name.to_owned();
    }
    Ok(())
}
