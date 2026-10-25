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

//! Central Podman CLI API for pod, container, and image execution and inspection.

use std::collections::BTreeSet;
use std::io::{self, Write};

use crate::error::PodError;
use crate::io::exec::command::{self, ExecOptions};
use crate::utilities::collection::{
    MAX_COLLECTION_PREALLOC, append_option_tokens, bounded_capacity, is_valid_option_value,
};
use crate::utilities::string::{
    contains_control_or_bidi, is_valid_pod_or_container_name, is_valid_text, is_valid_token,
    parse_ascii_u32,
};

/// Command-line arguments executed inside a container to launch an interactive `bash` or `sh` shell.
pub const SHELL_FALLBACK_COMMAND: [&str; 3] = [
    "sh",
    "-c",
    "command -v bash >/dev/null 2>&1 && exec bash || exec sh",
];

/// Base argument count (`create`, `--name`, `<name>`, `--pod`, `<pod>`, `--restart`, `<policy>`) for `podman create`.
const BASE_CONTAINER_CREATE_ARG_COUNT: usize = 7;
/// Base argument count (`exec`, `-it`, `-u`, `<user>`, `<container>`) for `podman exec -it`.
const BASE_CONTAINER_EXEC_ARG_COUNT: usize = 5;
/// Base argument count (`pod`, `create`, `--name`, `<pod>`) for `podman pod create`.
const BASE_POD_CREATE_ARG_COUNT: usize = 4;
/// Format template passed to `podman images --format`.
const IMAGES_FORMAT: &str =
    "{{.ID}};;;{{.Id}};;;{{.Repository}};;;{{.Tag}};;;{{.Size}};;;{{.Created}}";
/// Maximum number of container or image records inspected during image pruning queries.
const MAX_INSPECT_RECORDS: usize = 1024;
/// Upper bound on the number of arguments passed to `podman pod logs`.
const MAX_POD_LOGS_ARGS: usize = 16;
/// Maximum number of lines scanned in `podman --version` output.
const MAX_VERSION_LINES: usize = 32;
/// Maximum byte length accepted for `podman --version` output.
const MAX_VERSION_OUTPUT_BYTES: usize = 4096;
/// Minimum supported Podman major version.
const MIN_PODMAN_MAJOR: u32 = 5;
/// Minimum supported Podman minor version when major equals [`MIN_PODMAN_MAJOR`].
const MIN_PODMAN_MINOR: u32 = 8;
/// Podman CLI binary name.
const PODMAN_BIN: &str = "podman";
/// Arguments for pruning all unused images (`podman image prune -f -a`).
const PRUNE_ALL_ARGS: [&str; 4] = ["image", "prune", "-f", "-a"];
/// Arguments for pruning dangling images (`podman image prune -f`).
const PRUNE_DANGLING_ARGS: [&str; 3] = ["image", "prune", "-f"];
/// Format template passed to `podman ps -a --format`.
const PS_FORMAT: &str = "{{.ID}};;;{{.Image}};;;{{.Command}};;;{{.CreatedAt}};;;{{.Status}};;;{{.Ports}};;;{{.Names}};;;{{.PodName}};;;{{.Restarts}}";

/// Parameters for creating a container via [`create_container`] (`podman create`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CreateContainer<'a> {
    pub commands: &'a [String],
    pub container_name: &'a str,
    pub image: &'a str,
    pub options: &'a [String],
    pub pod_name: &'a str,
    pub restart: &'a str,
    pub volumes: &'a [String],
}

impl CreateContainer<'_> {
    /// Creates a new [`CreateContainer`] instance with default empty fields.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Parameters for creating a pod via [`create_pod`] (`podman pod create`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CreatePod<'a> {
    pub options: &'a [String],
    pub pod_name: &'a str,
    pub publish: &'a [String],
}

impl CreatePod<'_> {
    /// Creates a new [`CreatePod`] instance with default empty fields.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Parameters for executing a command inside a running container via [`exec_container`] (`podman exec -it`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecContainer<'a, S: AsRef<str> = String> {
    pub command: &'a [S],
    pub container_name: &'a str,
    pub interactive: bool,
    pub user: Option<&'a str>,
}

impl<S: AsRef<str>> ExecContainer<'_, S> {
    /// Creates a new [`ExecContainer`] instance with default empty fields.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl<S: AsRef<str>> Default for ExecContainer<'_, S> {
    fn default() -> Self {
        Self {
            command: &[],
            container_name: "",
            interactive: false,
            user: None,
        }
    }
}

/// Parameters for fetching or streaming pod logs via [`logs_pod`] (`podman pod logs`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PodLogs<'a> {
    pub container_name: Option<&'a str>,
    pub follow: bool,
    pub pod_name: &'a str,
    pub since: Option<&'a str>,
    pub tail: Option<&'a str>,
    pub timestamps: bool,
    pub until: Option<&'a str>,
}

impl PodLogs<'_> {
    /// Creates a new [`PodLogs`] instance with default empty fields.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Executes `podman --version` and evaluates whether the installed Podman CLI is version 5.8.0 or higher.
#[must_use]
pub fn check_version() -> bool {
    let Some(output) = run_podman_capture(&["--version"]) else {
        return false;
    };
    is_supported_version(&output)
}

/// Executes `podman container inspect -f '{{.State.Status}}' <container_name>` and returns `true` when the container is running.
#[must_use]
pub fn container_running(container_name: &str) -> bool {
    if !is_valid_pod_or_container_name(container_name) {
        return false;
    }
    let Some(output) = run_podman_capture(&[
        "container",
        "inspect",
        "-f",
        "{{.State.Status}}",
        container_name,
    ]) else {
        return false;
    };
    is_running_container_status(&output)
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
    run_podman_with(
        &arguments,
        &ExecOptions {
            interactive: false,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Executes `podman pod create --name <pod_name> [--publish <entry>...] [options...]`.
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
) -> Result<(), PodError> {
    validate_create_pod(request)?;
    let arguments = build_pod_create_args(request);
    run_podman_with(
        &arguments,
        &ExecOptions {
            interactive: false,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Executes `podman exec -it [-u <user>] <container_name> <command...>`.
///
/// # Errors
///
/// Returns [`PodError`] if `request` contains invalid fields or spawning `podman exec` fails.
pub fn exec_container<S: AsRef<str>, W: Write, E: Write>(
    request: &ExecContainer<'_, S>,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_exec_container(request)?;
    let arguments = build_container_exec_args(request);
    run_podman_with(
        &arguments,
        &ExecOptions {
            interactive: request.interactive,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Evaluates whether `podman container inspect -f '{{.State.Status}}'` output indicates a running container.
#[must_use]
pub fn is_running_container_status(output: &str) -> bool {
    let mut lines = output.lines();
    lines.next() == Some("running") && lines.next().is_none()
}

/// Evaluates whether a `podman --version` output string satisfies version 5.8.0 or higher.
#[must_use]
pub fn is_supported_version(output: &str) -> bool {
    let Some((major, minor)) = parse_podman_major_minor(output) else {
        return false;
    };
    major > MIN_PODMAN_MAJOR || (major == MIN_PODMAN_MAJOR && minor >= MIN_PODMAN_MINOR)
}

/// Executes `podman pod logs -n --color [-f] [--since <since>] [--until <until>] [--tail <tail>] [--timestamps] [-c <container_name>] <pod_name>`.
///
/// # Errors
///
/// Returns [`PodError`] if `request` contains invalid fields or spawning `podman pod logs` fails.
pub fn logs_pod<W: Write, E: Write>(
    request: &PodLogs<'_>,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_pod_logs(request)?;
    let arguments = build_pod_logs_args(request);
    run_podman_with(
        &arguments,
        &ExecOptions {
            interactive: true,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Executes `podman pod exists <pod_name>` and returns `true` when the pod exists in local storage.
#[must_use]
pub fn pod_exists(pod_name: &str) -> bool {
    if !is_valid_pod_or_container_name(pod_name) {
        return false;
    }
    run_podman_capture(&["pod", "exists", pod_name]).is_some()
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
    run_podman_with(
        arguments,
        &ExecOptions {
            interactive: false,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Executes `podman pull <image_reference>`.
///
/// # Errors
///
/// Returns [`PodError`] if `image_reference` is invalid or spawning `podman pull` fails.
pub fn pull_image<W: Write, E: Write>(
    image_reference: &str,
    prefix: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    ensure_valid(is_valid_token(image_reference), "invalid image reference")?;
    run_podman_with(
        &["pull", image_reference],
        &ExecOptions {
            interactive: false,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Executes `podman images [--filter dangling=true] --format ...` and returns raw `;;;`-delimited image lines.
#[must_use]
pub fn query_images(all: bool) -> Option<String> {
    query_images_with(all, &mut run_podman_capture)
}

/// Executes `podman ps -aq` and `podman inspect -f '{{.Image}}'` to collect image IDs attached to existing containers.
#[must_use]
pub fn query_in_use_images() -> BTreeSet<String> {
    collect_in_use_images_with(&mut run_podman_capture)
}

/// Executes `podman ps -a --format ...` and returns raw `;;;`-delimited container status records.
#[must_use]
pub fn query_ps_status() -> Option<String> {
    run_podman_capture(&["ps", "-a", "--format", PS_FORMAT])
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
    ensure_valid(
        is_valid_pod_or_container_name(container_name),
        "invalid container name",
    )?;
    run_podman_with(
        &["rm", container_name],
        &ExecOptions {
            interactive: false,
            prefix,
            simulate,
        },
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
    ensure_valid(is_valid_pod_or_container_name(pod_name), "invalid pod name")?;
    run_podman_with(
        &["pod", "rm", pod_name],
        &ExecOptions {
            interactive: false,
            prefix,
            simulate,
        },
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
    ensure_valid(
        is_valid_pod_or_container_name(container_name),
        "invalid container name",
    )?;
    run_podman_with(
        &["start", container_name],
        &ExecOptions {
            interactive: false,
            prefix,
            simulate,
        },
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
    ensure_valid(
        is_valid_pod_or_container_name(container_name),
        "invalid container name",
    )?;
    run_podman_with(
        &["stop", container_name],
        &ExecOptions {
            interactive: false,
            prefix,
            simulate,
        },
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
    ensure_valid(is_valid_pod_or_container_name(pod_name), "invalid pod name")?;
    run_podman_with(
        &["pod", "stop", pod_name],
        &ExecOptions {
            interactive: false,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Appends `(flag, value)` to `arguments` when `value` is `Some`.
fn append_flag_pair<'a>(arguments: &mut Vec<&'a str>, flag: &'a str, value: Option<&'a str>) {
    if let Some(raw_value) = value {
        arguments.push(flag);
        arguments.push(raw_value);
    }
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
    let capacity = bounded_capacity(
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
    append_option_tokens(&mut arguments, request.options);
    arguments.push(String::from(request.image));
    arguments.extend(request.commands.iter().cloned());
    arguments
}

/// Builds the `podman exec -it` argument vector from `request`.
#[must_use]
fn build_container_exec_args<'a, S: AsRef<str>>(
    request: &'a ExecContainer<'a, S>,
) -> Vec<&'a str> {
    let capacity = bounded_capacity(
        BASE_CONTAINER_EXEC_ARG_COUNT.saturating_add(request.command.len()),
        MAX_COLLECTION_PREALLOC,
    );
    let mut arguments = Vec::with_capacity(capacity);
    arguments.push("exec");
    arguments.push("-it");
    if let Some(user) = request.user {
        arguments.push("-u");
        arguments.push(user);
    }
    arguments.push(request.container_name);
    for token in request.command {
        arguments.push(token.as_ref());
    }
    arguments
}

/// Builds the `podman pod create` argument vector from `request`.
#[must_use]
fn build_pod_create_args(request: &CreatePod<'_>) -> Vec<String> {
    let capacity = bounded_capacity(
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
    append_option_tokens(&mut arguments, request.options);
    arguments
}

/// Builds the `podman pod logs` argument vector from `request`.
#[must_use]
fn build_pod_logs_args<'a>(request: &'a PodLogs<'a>) -> Vec<&'a str> {
    let mut arguments = Vec::with_capacity(MAX_POD_LOGS_ARGS);
    arguments.push("pod");
    arguments.push("logs");
    arguments.push("-n");
    arguments.push("--color");
    if request.follow {
        arguments.push("-f");
    }
    append_flag_pair(&mut arguments, "--since", request.since);
    append_flag_pair(&mut arguments, "--until", request.until);
    append_flag_pair(&mut arguments, "--tail", request.tail);
    if request.timestamps {
        arguments.push("--timestamps");
    }
    if let Some(container) = request.container_name {
        arguments.push("-c");
        arguments.push(container);
    }
    arguments.push(request.pod_name);
    arguments
}

/// Extracts valid container ID tokens from `podman ps -aq` output up to [`MAX_INSPECT_RECORDS`].
#[must_use]
fn collect_container_ids(ps_output: &str) -> Vec<&str> {
    let capacity = ps_output.lines().take(MAX_COLLECTION_PREALLOC).count();
    if capacity == 0 {
        return Vec::new();
    }
    let mut container_ids = Vec::with_capacity(capacity);
    for (index, line) in ps_output.lines().enumerate() {
        if index >= MAX_INSPECT_RECORDS || container_ids.len() >= MAX_INSPECT_RECORDS {
            break;
        }
        let trimmed = line.trim();
        if is_valid_pod_or_container_name(trimmed) {
            container_ids.push(trimmed);
        }
    }
    container_ids
}

/// Queries `podman ps -aq` and `podman inspect` via `capture` to collect image IDs attached to existing containers.
#[must_use]
fn collect_in_use_images_with<F>(capture: &mut F) -> BTreeSet<String>
where
    F: FnMut(&[&str]) -> Option<String>,
{
    let Some(ps_output) = capture(&["ps", "-aq"]) else {
        return BTreeSet::new();
    };
    let container_ids = collect_container_ids(&ps_output);
    if container_ids.is_empty() {
        return BTreeSet::new();
    }
    let mut inspect_args = Vec::with_capacity(container_ids.len().saturating_add(3));
    inspect_args.push("inspect");
    inspect_args.push("-f");
    inspect_args.push("{{.Image}}");
    inspect_args.extend(container_ids);
    let Some(inspect_output) = capture(&inspect_args) else {
        return BTreeSet::new();
    };
    parse_in_use_output(&inspect_output)
}

/// Returns `Ok(())` when `condition` holds, or an [`io::ErrorKind::InvalidInput`] [`PodError`] with `message`.
fn ensure_valid(condition: bool, message: &'static str) -> Result<(), PodError> {
    if condition {
        return Ok(());
    }
    Err(PodError::from(io::Error::new(
        io::ErrorKind::InvalidInput,
        message,
    )))
}

/// Extracts the raw semantic version token from a `"podman version <semver>"` output line.
fn extract_version_token(line: &str) -> Option<&str> {
    let mut tokens = line.split_whitespace();
    if tokens.next()? != "podman" || tokens.next()? != "version" {
        return None;
    }
    let version_token = tokens.next()?;
    if tokens.next().is_some() {
        return None;
    }
    Some(version_token)
}

/// Evaluates whether `patch_remainder` starts with valid patch digits followed by an optional SemVer suffix.
fn is_valid_patch_segment(patch_remainder: &str) -> bool {
    let Some((patch_digits, metadata)) = patch_remainder.split_once(['-', '+']) else {
        return parse_ascii_u32(patch_remainder).is_some();
    };
    parse_ascii_u32(patch_digits).is_some() && is_valid_version_metadata(metadata)
}

/// Evaluates whether `metadata` is a non-empty SemVer pre-release or build metadata string.
fn is_valid_version_metadata(metadata: &str) -> bool {
    !metadata.is_empty()
        && metadata.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-' || byte == b'+'
        })
}

/// Collects valid trimmed image IDs from `podman inspect -f '{{.Image}}'` output up to [`MAX_INSPECT_RECORDS`].
#[must_use]
fn parse_in_use_output(inspect_output: &str) -> BTreeSet<String> {
    let mut in_use = BTreeSet::new();
    for (index, line) in inspect_output.lines().enumerate() {
        if index >= MAX_INSPECT_RECORDS || in_use.len() >= MAX_INSPECT_RECORDS {
            break;
        }
        let trimmed = line.trim();
        if is_valid_token(trimmed) && !in_use.contains(trimmed) {
            in_use.insert(String::from(trimmed));
        }
    }
    in_use
}

/// Parses a single `"podman version <major>.<minor>.<patch>"` line into its `(major, minor)` version tuple.
fn parse_line_major_minor(line: &str) -> Option<(u32, u32)> {
    let version_token = extract_version_token(line)?;
    let (major_str, remainder) = version_token.split_once('.')?;
    let (minor_str, patch_remainder) = remainder.split_once('.')?;
    if !is_valid_patch_segment(patch_remainder) {
        return None;
    }
    let major = parse_ascii_u32(major_str)?;
    let minor = parse_ascii_u32(minor_str)?;
    Some((major, minor))
}

/// Scans bounded `podman --version` output lines and returns the first valid `(major, minor)` version tuple.
fn parse_podman_major_minor(output: &str) -> Option<(u32, u32)> {
    if output.len() > MAX_VERSION_OUTPUT_BYTES {
        return None;
    }
    for (index, line) in output.lines().enumerate() {
        if index >= MAX_VERSION_LINES {
            return None;
        }
        if let Some(version) = parse_line_major_minor(line) {
            return Some(version);
        }
    }
    None
}

/// Queries `podman images` (with `--filter dangling=true` when `all` is `false`) via `capture`.
#[must_use]
fn query_images_with<F>(all: bool, capture: &mut F) -> Option<String>
where
    F: FnMut(&[&str]) -> Option<String>,
{
    if all {
        capture(&["images", "--format", IMAGES_FORMAT])
    } else {
        capture(&[
            "images",
            "--filter",
            "dangling=true",
            "--format",
            IMAGES_FORMAT,
        ])
    }
}

/// Executes `podman` with `arguments` and returns captured standard output when exit status is `0`.
#[must_use]
fn run_podman_capture(arguments: &[&str]) -> Option<String> {
    command::exec_capture(PODMAN_BIN, arguments)
}

/// Executes `podman` with `arguments` using `options`, routing output to `stdout` and `stderr`.
fn run_podman_with<S, W, E>(
    arguments: &[S],
    options: &ExecOptions<'_>,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    S: AsRef<str>,
    W: Write,
    E: Write,
{
    command::exec_with(PODMAN_BIN, arguments, options, stdout, stderr)
}

/// Validates all fields of `request` before building `podman create` arguments.
fn validate_create_container(request: &CreateContainer<'_>) -> Result<(), PodError> {
    ensure_valid(
        is_valid_pod_or_container_name(request.container_name),
        "invalid container name",
    )?;
    ensure_valid(
        is_valid_pod_or_container_name(request.pod_name),
        "invalid pod name",
    )?;
    ensure_valid(is_valid_token(request.image), "invalid container image")?;
    ensure_valid(
        request.restart.is_empty() || is_valid_token(request.restart),
        "invalid restart policy",
    )?;
    ensure_valid(
        request.volumes.iter().all(|mount| is_valid_token(mount)),
        "invalid volume mount",
    )?;
    ensure_valid(
        request
            .options
            .iter()
            .all(|option| is_valid_option_value(option)),
        "invalid container option",
    )?;
    ensure_valid(
        request.commands.iter().all(|command| is_valid_text(command)),
        "invalid container command",
    )
}

/// Validates all fields of `request` before building `podman pod create` arguments.
fn validate_create_pod(request: &CreatePod<'_>) -> Result<(), PodError> {
    ensure_valid(
        is_valid_pod_or_container_name(request.pod_name),
        "invalid pod name",
    )?;
    ensure_valid(
        request.publish.iter().all(|entry| is_valid_token(entry)),
        "invalid publish entry",
    )?;
    ensure_valid(
        request
            .options
            .iter()
            .all(|option| is_valid_option_value(option)),
        "invalid pod option",
    )
}

/// Validates all fields of `request` before building `podman exec -it` arguments.
fn validate_exec_container<S: AsRef<str>>(
    request: &ExecContainer<'_, S>,
) -> Result<(), PodError> {
    ensure_valid(
        is_valid_pod_or_container_name(request.container_name),
        "invalid container name",
    )?;
    ensure_valid(
        request.user.is_none_or(is_valid_token),
        "invalid exec user",
    )?;
    let Some(program) = request.command.first().map(AsRef::as_ref) else {
        return ensure_valid(false, "missing exec command");
    };
    ensure_valid(
        is_valid_text(program) && !program.trim_start().starts_with('-'),
        "invalid exec program",
    )?;
    ensure_valid(
        request
            .command
            .iter()
            .all(|argument| !contains_control_or_bidi(argument.as_ref())),
        "invalid exec argument",
    )
}

/// Validates all fields of `request` before building `podman pod logs` arguments.
fn validate_pod_logs(request: &PodLogs<'_>) -> Result<(), PodError> {
    ensure_valid(
        is_valid_pod_or_container_name(request.pod_name),
        "invalid pod name",
    )?;
    ensure_valid(
        request
            .container_name
            .is_none_or(is_valid_pod_or_container_name),
        "invalid container name",
    )?;
    ensure_valid(
        request.since.is_none_or(is_valid_token),
        "invalid since flag",
    )?;
    ensure_valid(
        request.until.is_none_or(is_valid_token),
        "invalid until flag",
    )?;
    ensure_valid(request.tail.is_none_or(is_valid_token), "invalid tail flag")
}

#[cfg(test)]
mod tests {
    use std::io;

    use crate::error::PodError;
    use super::{
        CreateContainer, CreatePod, ExecContainer, IMAGES_FORMAT, MAX_INSPECT_RECORDS,
        MAX_VERSION_LINES, MAX_VERSION_OUTPUT_BYTES, PodLogs, SHELL_FALLBACK_COMMAND,
        build_container_create_args, build_container_exec_args, build_pod_create_args,
        build_pod_logs_args, check_version, collect_in_use_images_with, container_running,
        create_container, create_pod, exec_container, is_running_container_status,
        is_supported_version, logs_pod, parse_in_use_output, pod_exists, prune_images, pull_image,
        query_images, query_images_with, query_in_use_images, query_ps_status, remove_container,
        remove_pod, start_container, stop_container, stop_pod,
    };

    #[test]
    fn check_version_and_version_parser_edge_cases() {
        assert!(check_version());

        assert!(is_supported_version("podman version 5.8.0\n"));
        assert!(is_supported_version("podman version 5.8.1"));
        assert!(is_supported_version("podman version 5.9.0-rc1"));
        assert!(is_supported_version("podman version 6.0.0"));
        assert!(is_supported_version("podman version 6.1.4\n"));

        assert!(!is_supported_version("podman version 5.7.9"));
        assert!(!is_supported_version("podman version 4.9.3"));
        assert!(!is_supported_version("podman version 5.8"));
        assert!(!is_supported_version("podman version 5.8.x"));
        assert!(!is_supported_version("podman version x.8.0"));
        assert!(!is_supported_version("podman version 5.x.0"));
        assert!(!is_supported_version("podman version"));
        assert!(!is_supported_version("podman 5.8.0"));
        assert!(!is_supported_version(""));
    }

    #[test]
    fn container_running_and_pod_exists_checks() {
        assert!(is_running_container_status("running"));
        assert!(is_running_container_status("running\n"));
        assert!(is_running_container_status("running\r\n"));
        assert!(!is_running_container_status("exited\n"));
        assert!(!is_running_container_status("created\n"));
        assert!(!is_running_container_status("paused\n"));
        assert!(!is_running_container_status("running extra\n"));
        assert!(!is_running_container_status("running\nexited\n"));
        assert!(!is_running_container_status("running\n\n"));
        assert!(!is_running_container_status("\nrunning\n"));
        assert!(!is_running_container_status(""));

        assert!(!container_running(""));
        assert!(!container_running("--help"));
        assert!(!container_running("bad name"));
        assert!(!container_running("Upper_Case"));
        assert!(!container_running("bad..container"));
        assert!(!container_running("/"));
        assert!(!container_running(".."));
        assert!(!container_running("*"));
        assert!(!container_running(
            "podoxide_nonexistent_test_container_999999"
        ));

        assert!(!pod_exists(""));
        assert!(!pod_exists("--help"));
        assert!(!pod_exists("bad name"));
        assert!(!pod_exists("Upper_Case"));
        assert!(!pod_exists("bad..pod"));
        assert!(!pod_exists("podoxide_nonexistent_test_pod_999999"));
    }

    #[test]
    fn is_supported_version_rejects_spoofed_and_malformed_inputs() {
        assert!(is_supported_version(
            "WARN[0000] cgroup version 2 check\npodman version 5.8.0\n"
        ));
        assert!(is_supported_version("podman version 5.8.0+build.123"));

        assert!(!is_supported_version("podman version5.8.0"));
        assert!(!is_supported_version("docker version 24.0.0"));
        assert!(!is_supported_version("subversion 5.8.0"));
        assert!(!is_supported_version("inversion 6.0.0"));
        assert!(!is_supported_version("podman version 5.8.0 extra"));
        assert!(!is_supported_version("podman version +5.8.0"));
        assert!(!is_supported_version("podman version 5.+8.0"));
        assert!(!is_supported_version("podman version +5.+8.0"));
        assert!(!is_supported_version("podman version 5.8.+0"));
        assert!(!is_supported_version("podman version 5.8.0.1"));
        assert!(!is_supported_version("podman version 5.8.0..1"));
        assert!(!is_supported_version("podman version 5.8.0bad"));
        assert!(!is_supported_version("podman version 5.8.0ä"));
        assert!(!is_supported_version("podman version 5.8.0-"));
        assert!(!is_supported_version("podman version 5.8.0+"));
        assert!(!is_supported_version("podman version 5.8.0-rc1;id"));
        assert!(!is_supported_version(
            "podman version 5.8.99999999999999999999999999999"
        ));

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
        assert_eq!(exact_max_bytes.len(), MAX_VERSION_OUTPUT_BYTES);
        assert!(is_supported_version(&exact_max_bytes));

        let one_over_max_bytes = format!(
            "{}{version_line}",
            " ".repeat(MAX_VERSION_OUTPUT_BYTES - version_line.len() + 1)
        );
        assert_eq!(one_over_max_bytes.len(), MAX_VERSION_OUTPUT_BYTES + 1);
        assert!(!is_supported_version(&one_over_max_bytes));
    }

    #[test]
    fn podman_api_rejects_flag_injection_and_malformed_inputs() {
        let default_str_exec: ExecContainer<'_, &str> = ExecContainer::default();
        assert_eq!(default_str_exec, ExecContainer::new());

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        for bad_name in ["", "--all", "-fa", "..", "*", "Upper_Case", "a/b"] {
            for result in [
                remove_container(bad_name, "", true, &mut stdout, &mut stderr),
                remove_pod(bad_name, "", true, &mut stdout, &mut stderr),
                start_container(bad_name, "", true, &mut stdout, &mut stderr),
                stop_container(bad_name, "", true, &mut stdout, &mut stderr),
                stop_pod(bad_name, "", true, &mut stdout, &mut stderr),
            ] {
                assert!(matches!(
                    result,
                    Err(PodError::Io(ref error)) if error.kind() == io::ErrorKind::InvalidInput
                ));
            }
        }

        for bad_image in ["", "--all-tags", "-q", "alpine:latest --tls-verify=false", "alpine\n"] {
            let result = pull_image(bad_image, "", true, &mut stdout, &mut stderr);
            assert!(matches!(
                result,
                Err(PodError::Io(ref error)) if error.kind() == io::ErrorKind::InvalidInput
            ));
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

        for bad_exec in [
            ExecContainer {
                command: &["sh"],
                container_name: "--privileged",
                interactive: false,
                user: None,
            },
            ExecContainer {
                command: &["sh"],
                container_name: "si_po-1",
                interactive: false,
                user: Some("--privileged"),
            },
            ExecContainer {
                command: &[],
                container_name: "si_po-1",
                interactive: false,
                user: None,
            },
            ExecContainer {
                command: &["--privileged", "sh"],
                container_name: "si_po-1",
                interactive: false,
                user: None,
            },
            ExecContainer {
                command: &["sh", "-c", "echo\nhello"],
                container_name: "si_po-1",
                interactive: false,
                user: None,
            },
        ] {
            assert!(exec_container(&bad_exec, "", true, &mut stdout, &mut stderr).is_err());
        }

        let base_logs = PodLogs {
            container_name: None,
            follow: false,
            pod_name: "cmd_pod",
            since: None,
            tail: None,
            timestamps: false,
            until: None,
        };
        for bad_logs in [
            PodLogs {
                pod_name: "--latest",
                ..base_logs
            },
            PodLogs {
                container_name: Some("--latest"),
                ..base_logs
            },
            PodLogs {
                since: Some("-f"),
                ..base_logs
            },
            PodLogs {
                until: Some("-f"),
                ..base_logs
            },
            PodLogs {
                tail: Some("-f"),
                ..base_logs
            },
        ] {
            assert!(logs_pod(&bad_logs, "", true, &mut stdout, &mut stderr).is_err());
        }

        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
    }

    #[test]
    fn podman_api_requests_and_direct_execution_commands() {
        let default_pod = CreatePod::new();
        assert!(default_pod.pod_name.is_empty());
        assert!(default_pod.publish.is_empty());
        assert!(default_pod.options.is_empty());

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

        let default_container = CreateContainer::new();
        assert!(default_container.container_name.is_empty());
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

        let no_restart_request = CreateContainer {
            commands: &[],
            container_name: "si_po-1",
            image: "alpine:latest",
            options: &[],
            pod_name: "si_po",
            restart: "",
            volumes: &[],
        };
        assert!(
            !build_container_create_args(&no_restart_request)
                .iter()
                .any(|arg| arg == "--restart")
        );

        let default_exec: ExecContainer<'_, String> = ExecContainer::new();
        assert!(default_exec.container_name.is_empty());
        assert!(!default_exec.interactive);
        assert_eq!(default_exec.user, None);

        let exec_tokens = vec![
            String::from("sh"),
            String::from("-c"),
            String::from("echo hello world"),
        ];
        let exec_request = ExecContainer {
            command: &exec_tokens,
            container_name: "cmd_pod-db",
            interactive: false,
            user: Some("33"),
        };
        assert_eq!(
            build_container_exec_args(&exec_request),
            vec![
                "exec",
                "-it",
                "-u",
                "33",
                "cmd_pod-db",
                "sh",
                "-c",
                "echo hello world"
            ]
        );

        let default_logs = PodLogs::new();
        assert!(default_logs.pod_name.is_empty());
        assert!(!default_logs.follow);
        let logs_request = PodLogs {
            container_name: Some("cmd_pod-db"),
            follow: true,
            pod_name: "cmd_pod",
            since: Some("1h"),
            tail: Some("10"),
            timestamps: true,
            until: Some("2h"),
        };
        assert_eq!(
            build_pod_logs_args(&logs_request),
            vec![
                "pod",
                "logs",
                "-n",
                "--color",
                "-f",
                "--since",
                "1h",
                "--until",
                "2h",
                "--tail",
                "10",
                "--timestamps",
                "-c",
                "cmd_pod-db",
                "cmd_pod",
            ]
        );

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        create_pod(
            &create_pod_request,
            "Create pod: ",
            true,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        stop_pod("si_po", "Stop pod: ", true, &mut stdout, &mut stderr).unwrap();
        remove_pod("si_po", "Remove pod: ", true, &mut stdout, &mut stderr).unwrap();
        create_container(
            &no_restart_request,
            "Create container: ",
            true,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        start_container(
            "si_po-1",
            "Start container: ",
            true,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        stop_container(
            "si_po-1",
            "Stop container: ",
            true,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        remove_container(
            "si_po-1",
            "Remove container: ",
            true,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        pull_image("docker.io/alpine:latest", "", true, &mut stdout, &mut stderr).unwrap();
        prune_images(false, "Prune: ", true, &mut stdout, &mut stderr).unwrap();
        prune_images(true, "Prune all: ", true, &mut stdout, &mut stderr).unwrap();
        exec_container(
            &ExecContainer {
                command: &SHELL_FALLBACK_COMMAND,
                container_name: "si_po-1",
                interactive: true,
                user: None,
            },
            "Connect: ",
            true,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        logs_pod(&logs_request, "Logs: ", true, &mut stdout, &mut stderr).unwrap();

        let rendered = String::from_utf8(stdout).unwrap();
        assert!(rendered.contains("podman pod create --name si_po"));
        assert!(rendered.contains("podman pod stop si_po"));
        assert!(rendered.contains("podman pod rm si_po"));
        assert!(rendered.contains("podman create --name si_po-1 --pod si_po alpine:latest"));
        assert!(rendered.contains("podman start si_po-1"));
        assert!(rendered.contains("podman stop si_po-1"));
        assert!(rendered.contains("podman rm si_po-1"));
        assert!(rendered.contains("podman pull docker.io/alpine:latest"));
        assert!(rendered.contains("podman image prune -f\n"));
        assert!(rendered.contains("podman image prune -f -a\n"));
        assert!(rendered.contains("podman exec -it si_po-1 sh -c"));
        assert!(rendered.contains("podman pod logs -n --color -f --since 1h"));
        assert!(stderr.is_empty());
    }

    #[test]
    fn query_in_use_and_candidate_images_handle_filters_and_bounds() {
        let mut captured_calls: Vec<Vec<String>> = Vec::new();
        let mut capture = |arguments: &[&str]| -> Option<String> {
            captured_calls.push(arguments.iter().map(|item| (*item).to_owned()).collect());
            match arguments.first().copied() {
                Some("ps") => Some(String::from(
                    "c1\n--bad-flag\n..\n/\n*\nUpper\nbad..id\n\nc2\n",
                )),
                Some("inspect") => Some(String::from(
                    "  long2  \n  long2  \n--bad\ntwo words\n\n",
                )),
                Some("images") => Some(String::from("img_line\n")),
                _ => None,
            }
        };

        let in_use = collect_in_use_images_with(&mut capture);
        assert!(in_use.contains("long2"));
        assert_eq!(in_use.len(), 1);
        assert_eq!(query_images_with(false, &mut capture).unwrap(), "img_line\n");
        assert_eq!(query_images_with(true, &mut capture).unwrap(), "img_line\n");

        assert_eq!(captured_calls[0], vec!["ps", "-aq"]);
        assert_eq!(
            captured_calls[1],
            vec!["inspect", "-f", "{{.Image}}", "c1", "c2"]
        );
        assert_eq!(
            captured_calls[2],
            vec![
                "images",
                "--filter",
                "dangling=true",
                "--format",
                IMAGES_FORMAT,
            ]
        );
        assert_eq!(
            captured_calls[3],
            vec!["images", "--format", IMAGES_FORMAT]
        );

        assert!(collect_in_use_images_with(&mut |_: &[&str]| None).is_empty());
        assert!(
            collect_in_use_images_with(&mut |args: &[&str]| {
                if args.first() == Some(&"ps") {
                    Some(String::new())
                } else {
                    None
                }
            })
            .is_empty()
        );
        assert!(
            collect_in_use_images_with(&mut |args: &[&str]| {
                if args.first() == Some(&"ps") {
                    Some(String::from("c1\n"))
                } else {
                    None
                }
            })
            .is_empty()
        );

        let mut oversized_ps = String::new();
        let mut oversized_inspect = String::new();
        for index in 0..(MAX_INSPECT_RECORDS + 10) {
            oversized_ps.push_str(&format!("container_{index}\n"));
            oversized_inspect.push_str(&format!("used_{index}\n"));
        }
        assert_eq!(
            parse_in_use_output(&oversized_inspect).len(),
            MAX_INSPECT_RECORDS
        );

        let duplicate_flood = format!(
            "{}trailing_image\n",
            "same_image\n".repeat(MAX_INSPECT_RECORDS)
        );
        let duplicate_set = parse_in_use_output(&duplicate_flood);
        assert_eq!(duplicate_set.len(), 1);
        assert!(duplicate_set.contains("same_image"));
        assert!(!duplicate_set.contains("trailing_image"));

        let invalid_ps_flood = format!("{}valid_after_limit\n", "..\n".repeat(MAX_INSPECT_RECORDS));
        assert!(
            collect_in_use_images_with(&mut |args: &[&str]| {
                if args.first() == Some(&"ps") {
                    Some(invalid_ps_flood.clone())
                } else {
                    Some(String::from("should_not_run\n"))
                }
            })
            .is_empty()
        );

        let bounded_in_use = collect_in_use_images_with(&mut |args: &[&str]| {
            match args.first().copied() {
                Some("ps") => Some(oversized_ps.clone()),
                Some("inspect") => {
                    assert_eq!(args.len(), MAX_INSPECT_RECORDS + 3);
                    Some(oversized_inspect.clone())
                }
                _ => None,
            }
        });
        assert_eq!(bounded_in_use.len(), MAX_INSPECT_RECORDS);

        assert!(query_ps_status().is_some());
        assert!(query_images(false).is_some());
        let _ = query_in_use_images();
    }
}
