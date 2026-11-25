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

//! Podman CLI lifecycle operations for creating, starting, stopping, removing, and pruning resources.

use std::io::Write;

use crate::error::PodError;
use crate::io::exec::command::{ExecMode, ExecOptions, ExecOutcome, InterruptPolicy};
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::{path, string};

/// Base argument count (`create`, `--name`, `<name>`, `--pod`, `<pod>`, `--restart`, `<policy>`) for `podman create`.
const BASE_CONTAINER_CREATE_ARG_COUNT: usize = 7;
/// Base argument count (`pod`, `create`, `--name`, `<pod>`) for `podman pod create`.
const BASE_POD_CREATE_ARG_COUNT: usize = 4;
/// Arguments for pruning all unused images (`podman image prune -f -a`).
const PRUNE_ALL_ARGS: [&str; 4] = ["image", "prune", "-f", "-a"];
/// Arguments for pruning dangling images (`podman image prune -f`).
const PRUNE_DANGLING_ARGS: [&str; 3] = ["image", "prune", "-f"];

/// Parameters for creating a container via [`create_container`] (`podman create`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreateContainer<'a> {
    pub commands: &'a [String],
    pub container_name: &'a str,
    pub image: &'a str,
    pub options: &'a [String],
    pub pod_name: &'a str,
    pub restart: &'a str,
    pub volumes: &'a [String],
}

/// Parameters for creating a pod via [`create_pod`] (`podman pod create`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CreatePod<'a> {
    pub options: &'a [String],
    pub pod_name: &'a str,
    pub publish: &'a [String],
}

/// Executes `podman create --name <container_name> --pod <pod_name> [--restart <restart>] [--volume <mount>...] [options...] <image> [commands...]`.
///
/// # Errors
///
/// Returns [`PodError`] if `request` contains invalid fields or spawning `podman create` fails.
pub fn create_container<W: Write, E: Write>(
    request: &CreateContainer<'_>,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_create_container(request)?;
    let arguments = build_container_create_args(request);
    let _ = super::run_podman_with(
        &arguments,
        &ExecOptions {
            interrupt_policy: InterruptPolicy::WarnOnInterrupt,
            mode: ExecMode::Captured,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )?;
    Ok(())
}

/// Executes `podman pod create --name <pod_name> [--publish <entry>...] [options...]` and returns its [`ExecOutcome`].
///
/// # Errors
///
/// Returns [`PodError`] if `request` contains invalid fields or spawning `podman pod create` fails.
pub fn create_pod<W: Write, E: Write>(
    request: &CreatePod<'_>,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    validate_create_pod(request)?;
    let arguments = build_pod_create_args(request);
    super::run_podman_with(
        &arguments,
        &ExecOptions {
            interrupt_policy: InterruptPolicy::WarnOnInterrupt,
            mode: ExecMode::Captured,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Executes `podman image prune -f` (or `podman image prune -f -a` when `all` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if spawning `podman image prune` fails.
pub fn prune_images<W: Write, E: Write>(
    all: bool,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    let arguments: &[&str] = if all {
        &PRUNE_ALL_ARGS
    } else {
        &PRUNE_DANGLING_ARGS
    };
    let _ = super::run_podman_with(
        arguments,
        &ExecOptions {
            interrupt_policy: InterruptPolicy::WarnOnInterrupt,
            mode: ExecMode::Captured,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )?;
    Ok(())
}

/// Executes `podman rm <container_name>`.
///
/// # Errors
///
/// Returns [`PodError`] if `container_name` is invalid or spawning `podman rm` fails.
pub fn remove_container<W: Write, E: Write>(
    container_name: &str,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    run_named_podman_command(
        &["rm", container_name],
        container_name,
        "invalid container name",
        prefix,
        simulate,
        stdout,
        stderr,
    )
}

/// Executes `podman pod rm <pod_name>`.
///
/// # Errors
///
/// Returns [`PodError`] if `pod_name` is invalid or spawning `podman pod rm` fails.
pub fn remove_pod<W: Write, E: Write>(
    pod_name: &str,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    run_named_podman_command(
        &["pod", "rm", pod_name],
        pod_name,
        "invalid pod name",
        prefix,
        simulate,
        stdout,
        stderr,
    )
}

/// Executes `podman start <container_name>`.
///
/// # Errors
///
/// Returns [`PodError`] if `container_name` is invalid or spawning `podman start` fails.
pub fn start_container<W: Write, E: Write>(
    container_name: &str,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    run_named_podman_command(
        &["start", container_name],
        container_name,
        "invalid container name",
        prefix,
        simulate,
        stdout,
        stderr,
    )
}

/// Executes `podman stop <container_name>`.
///
/// # Errors
///
/// Returns [`PodError`] if `container_name` is invalid or spawning `podman stop` fails.
pub fn stop_container<W: Write, E: Write>(
    container_name: &str,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    run_named_podman_command(
        &["stop", container_name],
        container_name,
        "invalid container name",
        prefix,
        simulate,
        stdout,
        stderr,
    )
}

/// Executes `podman pod stop <pod_name>`.
///
/// # Errors
///
/// Returns [`PodError`] if `pod_name` is invalid or spawning `podman pod stop` fails.
pub fn stop_pod<W: Write, E: Write>(
    pod_name: &str,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    run_named_podman_command(
        &["pod", "stop", pod_name],
        pod_name,
        "invalid pod name",
        prefix,
        simulate,
        stdout,
        stderr,
    )
}

/// Appends `--publish <entry>` pairs from `publish_entries` into `arguments`.
fn append_publish_arguments(arguments: &mut Vec<String>, publish_entries: &[String]) {
    for entry in publish_entries {
        arguments.push(String::from("--publish"));
        arguments.push(entry.clone());
    }
}

/// Appends `--volume <mount>` pairs from `volumes` into `arguments`.
fn append_volume_arguments(arguments: &mut Vec<String>, volumes: &[String]) {
    for mount in volumes {
        arguments.push(String::from("--volume"));
        arguments.push(mount.clone());
    }
}

/// Builds the `podman create` argument vector from `request`.
#[must_use]
fn build_container_create_args(request: &CreateContainer<'_>) -> Vec<String> {
    let capacity = collection::bounded_capacity(
        BASE_CONTAINER_CREATE_ARG_COUNT
            .saturating_add(request.volumes.len().saturating_mul(2))
            .saturating_add(request.options.len().saturating_mul(2))
            .saturating_add(request.commands.len()),
        MAX_COLLECTION_PREALLOC,
    );
    let mut arguments = Vec::with_capacity(capacity);
    arguments.push(String::from("create"));
    arguments.push(String::from("--name"));
    arguments.push(String::from(request.container_name));
    arguments.push(String::from("--pod"));
    arguments.push(String::from(request.pod_name));
    if !request.restart.is_empty() {
        arguments.push(String::from("--restart"));
        arguments.push(String::from(request.restart));
    }
    append_volume_arguments(&mut arguments, request.volumes);
    collection::append_option_tokens(&mut arguments, request.options);
    arguments.push(String::from(request.image));
    arguments.extend(request.commands.iter().cloned());
    arguments
}

/// Builds the `podman pod create` argument vector from `request`.
#[must_use]
fn build_pod_create_args(request: &CreatePod<'_>) -> Vec<String> {
    let capacity = collection::bounded_capacity(
        BASE_POD_CREATE_ARG_COUNT
            .saturating_add(request.publish.len().saturating_mul(2))
            .saturating_add(request.options.len().saturating_mul(2)),
        MAX_COLLECTION_PREALLOC,
    );
    let mut arguments = Vec::with_capacity(capacity);
    arguments.push(String::from("pod"));
    arguments.push(String::from("create"));
    arguments.push(String::from("--name"));
    arguments.push(String::from(request.pod_name));
    append_publish_arguments(&mut arguments, request.publish);
    collection::append_option_tokens(&mut arguments, request.options);
    arguments
}

/// Evaluates whether `mount` is a valid `--volume` specification (`<container_dir>`, `<host_dir>:<container_dir>`, or `<host_dir>:<container_dir>:<options>`).
#[must_use]
fn is_valid_volume_spec(mount: &str) -> bool {
    let Some((host_dir, remainder)) = mount.split_once(':') else {
        return mount.starts_with('/') && path::is_valid_directory_path(mount);
    };
    if !path::is_valid_directory_path(host_dir) {
        return false;
    }
    let Some((container_dir, options)) = remainder.split_once(':') else {
        return remainder.starts_with('/') && path::is_valid_directory_path(remainder);
    };
    container_dir.starts_with('/')
        && path::is_valid_directory_path(container_dir)
        && path::is_valid_volume_options(options)
}

/// Validates `name` as a managed pod or container name and runs `arguments` via Podman in captured mode.
fn run_named_podman_command<W: Write, E: Write>(
    arguments: &[&str],
    name: &str,
    invalid_message: &'static str,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    super::ensure_valid(
        string::is_valid_pod_or_container_name(name),
        invalid_message,
    )?;
    let _ = super::run_podman_with(
        arguments,
        &ExecOptions {
            interrupt_policy: InterruptPolicy::WarnOnInterrupt,
            mode: ExecMode::Captured,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )?;
    Ok(())
}

/// Validates all fields of `request` before building `podman create` arguments.
fn validate_create_container(request: &CreateContainer<'_>) -> Result<(), PodError> {
    super::ensure_valid(
        string::is_valid_pod_or_container_name(request.container_name),
        "invalid container name",
    )?;
    super::ensure_valid(
        string::is_valid_pod_or_container_name(request.pod_name),
        "invalid pod name",
    )?;
    super::ensure_valid(
        string::is_valid_token(request.image),
        "invalid container image",
    )?;
    super::ensure_valid(
        request.restart.is_empty() || string::is_valid_token(request.restart),
        "invalid restart policy",
    )?;
    super::ensure_valid(
        request
            .volumes
            .iter()
            .all(|mount| is_valid_volume_spec(mount)),
        "invalid volume mount",
    )?;
    super::ensure_valid(
        request
            .options
            .iter()
            .all(|option| collection::is_valid_option_value(option)),
        "invalid container option",
    )?;
    super::ensure_valid(
        request
            .commands
            .iter()
            .all(|command| string::is_valid_text(command)),
        "invalid container command",
    )
}

/// Validates all fields of `request` before building `podman pod create` arguments.
fn validate_create_pod(request: &CreatePod<'_>) -> Result<(), PodError> {
    super::ensure_valid(
        string::is_valid_pod_or_container_name(request.pod_name),
        "invalid pod name",
    )?;
    super::ensure_valid(
        request
            .publish
            .iter()
            .all(|entry| string::is_valid_token(entry)),
        "invalid publish entry",
    )?;
    super::ensure_valid(
        request
            .options
            .iter()
            .all(|option| collection::is_valid_option_value(option)),
        "invalid pod option",
    )
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
