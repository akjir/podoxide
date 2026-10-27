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

//! Container domain models (`container` blocks and `volume` mounts in `.pod` recipes) and Podman container lifecycle execution.

use std::borrow::Cow;
use std::io::{self, Write};
use std::path::Path;

use crate::core::log;
use crate::core::pod::Pod;
use crate::error::{PodError, RecipeError, RecipeErrorKind};
use crate::io::exec::podman::{self, CreateContainer};
use crate::utilities::collection::{
    MAX_COLLECTION_PREALLOC, bounded_capacity, is_valid_option_value,
};
use crate::utilities::path::{is_valid_directory, is_valid_directory_path};
use crate::utilities::string::{
    contains_control_or_bidi, contains_control_or_whitespace, is_valid_pod_or_container_name,
    is_valid_text, is_valid_token, normalize_name,
};

/// Container specification and runtime arguments from a `container` block in a `.pod` recipe.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Container {
    pub commands: Vec<String>,
    pub image: String,
    pub line: usize,
    pub name: String,
    pub options: Vec<String>,
    pub registry: String,
    pub restart: String,
    pub volumes: Vec<VolumeMount>,
}

impl Container {
    /// Creates a new [`Container`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves and normalizes the container name using `pod_id` and its 1-based `index`.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] when the configured container name is invalid.
    pub fn ensure_name(&mut self, pod_id: &str, index: usize) -> Result<(), RecipeError> {
        self.name = resolve_container_name_value(&self.name, pod_id, index, self.line)?;
        Ok(())
    }

    /// Validates that container fields (`name`, `image`, `registry`, `restart`, `options`, `commands`, and `volumes`) are well-formed.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] when `image` is missing or any container field is invalid.
    pub fn validate(&self) -> Result<(), RecipeError> {
        let label = container_label(&self.name);
        if !self.name.is_empty() && !is_valid_pod_or_container_name(&self.name) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidContainerName,
                self.line,
                &self.name,
            ));
        }
        validate_container_image(&self.image, self.line, label)?;
        validate_optional_token(&self.registry, RecipeErrorKind::InvalidRegistry, self.line)?;
        validate_optional_token(
            &self.restart,
            RecipeErrorKind::InvalidRestartPolicy,
            self.line,
        )?;
        validate_option_entries(&self.options, self.line)?;
        validate_text_entries(&self.commands, RecipeErrorKind::InvalidCommand, self.line)?;
        validate_volume_mounts(&self.volumes, self.line, label)
    }

    /// Validates that this container and its parent `pod` satisfy all preconditions for `podman create`.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] when the container name, pod ID, pod publish/option entries, pod registry, container fields, or relative volume base directory are invalid.
    pub fn validate_create(&self, pod: &Pod) -> Result<(), RecipeError> {
        validate_container_name(self)?;
        pod.validate_create()?;
        validate_optional_token(&pod.registry, RecipeErrorKind::InvalidRegistry, pod.line)?;
        self.validate()?;
        if has_relative_host_volume(&self.volumes) && !is_valid_directory(&pod.path) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidVolume,
                self.line,
                container_label(&self.name),
            ));
        }
        Ok(())
    }
}

/// Host-to-container volume mount specification from a `volume` directive inside a `container` block.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VolumeMount {
    pub container_dir: String,
    pub host_dir: String,
    pub options: String,
}

impl VolumeMount {
    /// Creates a new [`VolumeMount`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Creates `container` in Podman (`podman create`), creating `pod` first if it does not exist, without starting it.
///
/// # Errors
///
/// Returns [`PodError`] if validation, volume resolution, or Podman execution fails.
pub fn create(container: &Container, pod: &Pod, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    create_with(container, pod, simulate, &mut stdout, &mut stderr)
}

/// Creates `container` in Podman (`podman create`), creating `pod` first if it does not exist, and writes command output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if validation, volume resolution, stream writing, or Podman execution fails.
pub fn create_with<W: Write, E: Write>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    create_with_pod_check(container, pod, simulate, podman::pod_exists, stdout, stderr)
}

/// Stops and removes `container` in Podman (`stop` + `remove`).
///
/// # Errors
///
/// Returns [`PodError`] if stopping or removing the container fails.
pub fn down(container: &Container, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    down_with(container, simulate, &mut stdout, &mut stderr)
}

/// Stops and removes `container` in Podman (`stop` + `remove`), writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if container validation, stream writing, or Podman execution fails.
pub fn down_with<W: Write, E: Write>(
    container: &Container,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_container_name(container)?;
    stop_with(container, simulate, stdout, stderr)?;
    remove_with(container, simulate, stdout, stderr)
}

/// Formats `volume` against `pod_path` into a Podman `--volume` specification string.
///
/// # Errors
///
/// Returns [`RecipeError`] with [`RecipeErrorKind::InvalidVolume`] when `volume` is invalid.
pub fn format_volume_mount(
    volume: &VolumeMount,
    pod_path: &Path,
    line: usize,
    label: &str,
) -> Result<String, RecipeError> {
    if !is_valid_volume_mount(volume) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidVolume,
            line,
            label,
        ));
    }
    let host_path = if volume.host_dir.is_empty() {
        Cow::Borrowed("")
    } else {
        resolve_host_volume_path(&volume.host_dir, pod_path, line, label)?
    };
    let capacity = host_path
        .len()
        .saturating_add(volume.container_dir.len())
        .saturating_add(volume.options.len())
        .saturating_add(2);
    let mut formatted = String::with_capacity(capacity);
    if !host_path.is_empty() {
        formatted.push_str(&host_path);
        formatted.push(':');
    }
    formatted.push_str(&volume.container_dir);
    if !volume.options.is_empty() {
        formatted.push(':');
        formatted.push_str(&volume.options);
    }
    Ok(formatted)
}

/// Evaluates whether `volume` has a valid absolute `container_dir` and safe `host_dir` and `options`.
#[must_use]
pub fn is_valid_volume_mount(volume: &VolumeMount) -> bool {
    if !volume.container_dir.starts_with('/') || !is_valid_directory_path(&volume.container_dir) {
        return false;
    }
    if volume.host_dir.is_empty() {
        return volume.options.is_empty();
    }
    is_valid_directory_path(&volume.host_dir) && is_valid_volume_options(&volume.options)
}

/// Pulls the latest image for `container` in Podman (`podman pull`).
///
/// # Errors
///
/// Returns [`PodError`] if Podman execution fails.
pub fn pull(container: &Container, pod: &Pod, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    pull_with(container, pod, simulate, &mut stdout, &mut stderr)
}

/// Pulls the latest image for `container` in Podman (`podman pull`), writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if container/pod validation or Podman execution fails.
pub fn pull_with<W: Write, E: Write>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_container_name(container)?;
    container.validate()?;
    validate_optional_token(&pod.registry, RecipeErrorKind::InvalidRegistry, pod.line)?;
    log::write_print(stdout, &format!("Update container '{}' ...", container.name));
    let image_reference = resolve_image_reference(container, pod);
    podman::pull_image(&image_reference, "", simulate, stdout, stderr)
}

/// Stops, removes, recreates, and starts `container` inside `pod` (`down` + `up`).
///
/// # Errors
///
/// Returns [`PodError`] if tearing down or bringing up the container fails.
pub fn reload(container: &Container, pod: &Pod, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    reload_with(container, pod, simulate, &mut stdout, &mut stderr)
}

/// Stops, removes, recreates, and starts `container` inside `pod` (`down` + `up`), writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if validation, stream writing, or Podman execution fails.
pub fn reload_with<W: Write, E: Write>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    container.validate_create(pod)?;
    down_with(container, simulate, stdout, stderr)?;
    up_with(container, pod, simulate, stdout, stderr)
}

/// Removes `container` in Podman (`podman rm`).
///
/// # Errors
///
/// Returns [`PodError`] if Podman execution fails.
pub fn remove(container: &Container, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    remove_with(container, simulate, &mut stdout, &mut stderr)
}

/// Removes `container` in Podman (`podman rm`), writing command output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn remove_with<W: Write, E: Write>(
    container: &Container,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_container_name(container)?;
    let prefix = format!("Remove container '{}': ", container.name);
    podman::remove_container(&container.name, &prefix, simulate, stdout, stderr)
}

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
        if index == 0 || !is_valid_pod_or_container_name(pod_id) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidContainerName,
                line,
                raw_name,
            ));
        }
        return Ok(format!("{pod_id}-{index}"));
    }
    if contains_control_or_bidi(raw_name) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidContainerName,
            line,
            raw_name,
        ));
    }
    let normalized = normalize_name(raw_name);
    if let Some(suffix) = normalized.strip_prefix('*') {
        if !is_valid_pod_or_container_name(pod_id) || !is_valid_pod_or_container_name(suffix) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidContainerName,
                line,
                raw_name,
            ));
        }
        return Ok(format!("{pod_id}-{suffix}"));
    }
    if !is_valid_pod_or_container_name(&normalized) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidContainerName,
            line,
            raw_name,
        ));
    }
    Ok(normalized)
}

/// Resolves the fully qualified `<registry>/<image>` reference for `container` inside `pod`.
#[must_use]
pub fn resolve_image_reference<'a>(container: &'a Container, pod: &'a Pod) -> Cow<'a, str> {
    let registry = if container.registry.is_empty() {
        pod.registry.as_str()
    } else {
        container.registry.as_str()
    };
    if registry.is_empty() {
        return Cow::Borrowed(container.image.as_str());
    }
    Cow::Owned(format!("{registry}/{}", container.image))
}

/// Stops and starts `container` in Podman (`stop` + `start`).
///
/// # Errors
///
/// Returns [`PodError`] if stopping or starting the container fails.
pub fn restart(container: &Container, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    restart_with(container, simulate, &mut stdout, &mut stderr)
}

/// Stops and starts `container` in Podman (`stop` + `start`), writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if container validation, stream writing, or Podman execution fails.
pub fn restart_with<W: Write, E: Write>(
    container: &Container,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_container_name(container)?;
    stop_with(container, simulate, stdout, stderr)?;
    start_with(container, simulate, stdout, stderr)
}

/// Starts `container` in Podman (`podman start`).
///
/// # Errors
///
/// Returns [`PodError`] if Podman execution fails.
pub fn start(container: &Container, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    start_with(container, simulate, &mut stdout, &mut stderr)
}

/// Starts `container` in Podman (`podman start`), writing command output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn start_with<W: Write, E: Write>(
    container: &Container,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_container_name(container)?;
    let prefix = format!("Start container '{}': ", container.name);
    podman::start_container(&container.name, &prefix, simulate, stdout, stderr)
}

/// Stops `container` in Podman (`podman stop`).
///
/// # Errors
///
/// Returns [`PodError`] if Podman execution fails.
pub fn stop(container: &Container, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    stop_with(container, simulate, &mut stdout, &mut stderr)
}

/// Stops `container` in Podman (`podman stop`), writing command output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn stop_with<W: Write, E: Write>(
    container: &Container,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_container_name(container)?;
    let prefix = format!("Stop container '{}': ", container.name);
    podman::stop_container(&container.name, &prefix, simulate, stdout, stderr)
}

/// Creates and starts `container` inside `pod` (`create` + `start`).
///
/// # Errors
///
/// Returns [`PodError`] if creating or starting the container fails.
pub fn up(container: &Container, pod: &Pod, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    up_with(container, pod, simulate, &mut stdout, &mut stderr)
}

/// Creates and starts `container` inside `pod` (`create` + `start`), writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if validation, stream writing, or Podman execution fails.
pub fn up_with<W: Write, E: Write>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    create_with(container, pod, simulate, stdout, stderr)?;
    start_with(container, simulate, stdout, stderr)
}

/// Pulls the latest image for `container` and reloads the container inside `pod` when the image was updated (`pull` + conditional `reload`).
///
/// # Errors
///
/// Returns [`PodError`] if pulling the image or reloading the container fails.
pub fn update(container: &Container, pod: &Pod, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    update_with(container, pod, simulate, &mut stdout, &mut stderr)
}

/// Pulls the latest image for `container` and reloads the container inside `pod` when the image was updated (or when `simulate` is `true`), writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if container/pod validation or Podman execution fails.
pub fn update_with<W: Write, E: Write>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    if simulate {
        container.validate_create(pod)?;
        pull_with(container, pod, true, stdout, stderr)?;
        return reload_with(container, pod, true, stdout, stderr);
    }
    let mut query_id = podman::query_image_id;
    update_with_query(container, pod, false, &mut query_id, stdout, stderr)
}

/// Pulls the latest image for `container` and returns `true` when the local image ID changed or was newly downloaded.
///
/// # Errors
///
/// Returns [`PodError`] if container/pod validation or Podman execution fails.
pub(crate) fn pull_and_check_with<W, E, Q>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    query_id: &mut Q,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<bool, PodError>
where
    W: Write,
    E: Write,
    Q: FnMut(&str) -> Option<String>,
{
    validate_container_name(container)?;
    container.validate()?;
    validate_optional_token(&pod.registry, RecipeErrorKind::InvalidRegistry, pod.line)?;
    let image_reference = resolve_image_reference(container, pod);
    let before_id = query_id(&image_reference);
    log::write_print(stdout, &format!("Update container '{}' ...", container.name));
    podman::pull_image(&image_reference, "", simulate, stdout, stderr)?;
    let after_id = query_id(&image_reference);
    Ok(after_id.is_some() && before_id != after_id)
}

/// Returns `name` when non-empty, or `"container"` as a fallback error token.
#[must_use]
const fn container_label(name: &str) -> &str {
    if name.is_empty() {
        "container"
    } else {
        name
    }
}

/// Validates `container` and `pod`, ensures `pod` exists via `pod_exists` (creating it when missing), and executes `podman create`.
fn create_with_pod_check<W, E, F>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    pod_exists: F,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    W: Write,
    E: Write,
    F: FnOnce(&str) -> bool,
{
    container.validate_create(pod)?;
    let label = container_label(&container.name);
    let volumes = format_volume_mounts(&container.volumes, &pod.path, container.line, label)?;
    let image = resolve_image_reference(container, pod);
    pod.ensure_created_with(simulate, pod_exists, stdout, stderr)?;
    let prefix = format!("Create container '{}': ", container.name);
    podman::create_container(
        &CreateContainer {
            commands: &container.commands,
            container_name: &container.name,
            image: &image,
            options: &container.options,
            pod_name: &pod.id,
            restart: &container.restart,
            volumes: &volumes,
        },
        &prefix,
        simulate,
        stdout,
        stderr,
    )
}

/// Formats all `volumes` against `pod_path` into Podman `--volume` specification strings.
fn format_volume_mounts(
    volumes: &[VolumeMount],
    pod_path: &Path,
    line: usize,
    label: &str,
) -> Result<Vec<String>, RecipeError> {
    let capacity = bounded_capacity(volumes.len(), MAX_COLLECTION_PREALLOC);
    let mut mounts = Vec::with_capacity(capacity);
    for volume in volumes {
        mounts.push(format_volume_mount(volume, pod_path, line, label)?);
    }
    Ok(mounts)
}

/// Evaluates whether `volumes` contains any mount with a relative host directory or volume name.
#[must_use]
fn has_relative_host_volume(volumes: &[VolumeMount]) -> bool {
    volumes
        .iter()
        .any(|volume| !volume.host_dir.is_empty() && !volume.host_dir.starts_with('/'))
}

/// Evaluates whether volume mount `options` (when non-empty) are free of colons, slashes, `..`, leading hyphens, and whitespace.
#[must_use]
fn is_valid_volume_options(options: &str) -> bool {
    options.is_empty()
        || (!options.starts_with('-')
            && !options.contains(':')
            && !options.contains('/')
            && !options.contains("..")
            && !contains_control_or_whitespace(options))
}

/// Joins a validated UTF-8 `pod_dir` and normalized relative `clean_host` into a volume host path.
#[must_use]
fn join_pod_volume_path(pod_dir: &str, clean_host: &str) -> String {
    let trimmed_pod = pod_dir.trim_end_matches('/');
    let clean_pod = if trimmed_pod.is_empty() {
        pod_dir
    } else {
        trimmed_pod
    };
    let needs_dot_prefix =
        !clean_pod.starts_with('/') && clean_pod != "." && !clean_pod.starts_with("./");
    let capacity = clean_pod
        .len()
        .saturating_add(clean_host.len())
        .saturating_add(3);
    let mut resolved = String::with_capacity(capacity);
    if needs_dot_prefix {
        resolved.push_str("./");
    }
    resolved.push_str(clean_pod);
    if !clean_host.is_empty() {
        resolved.push('/');
        resolved.push_str(clean_host);
    }
    resolved
}

/// Resolves a non-empty `host_dir` against `pod_path` when `host_dir` is relative.
fn resolve_host_volume_path<'a>(
    host_dir: &'a str,
    pod_path: &Path,
    line: usize,
    label: &str,
) -> Result<Cow<'a, str>, RecipeError> {
    if host_dir.starts_with('/') {
        return Ok(Cow::Borrowed(host_dir));
    }
    let Some(pod_dir) = pod_path.to_str().filter(|dir| is_valid_directory_path(dir)) else {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidVolume,
            line,
            label,
        ));
    };
    let mut rest = host_dir.trim_end_matches('/');
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped.trim_start_matches('/');
    }
    let clean_host = if rest == "." { "" } else { rest };
    Ok(Cow::Owned(join_pod_volume_path(pod_dir, clean_host)))
}

/// Pulls the latest image for `container` using `query_id` to detect image updates and reloads the container only when updated.
fn update_with_query<W, E, Q>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    query_id: &mut Q,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    W: Write,
    E: Write,
    Q: FnMut(&str) -> Option<String>,
{
    container.validate_create(pod)?;
    if !pull_and_check_with(container, pod, simulate, query_id, stdout, stderr)? {
        log::write_print(
            stdout,
            &format!("Container '{}' is up to date.", container.name),
        );
        return Ok(());
    }
    reload_with(container, pod, simulate, stdout, stderr)
}

/// Validates that `image` is non-empty and a valid single token without leading hyphens.
fn validate_container_image(image: &str, line: usize, label: &str) -> Result<(), RecipeError> {
    if image.is_empty() {
        return Err(RecipeError::with_token(
            RecipeErrorKind::MissingContainerImage,
            line,
            label,
        ));
    }
    if !is_valid_token(image) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidImage,
            line,
            image,
        ));
    }
    Ok(())
}

/// Validates that `container.name` is a valid non-empty Podman container name.
fn validate_container_name(container: &Container) -> Result<(), RecipeError> {
    if !is_valid_pod_or_container_name(&container.name) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidContainerName,
            container.line,
            container_label(&container.name),
        ));
    }
    Ok(())
}

/// Validates that every entry in `options` satisfies [`is_valid_option_value`].
fn validate_option_entries(options: &[String], line: usize) -> Result<(), RecipeError> {
    for option in options {
        if !is_valid_option_value(option) {
            return Err(RecipeError::with_token(
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
    if !value.is_empty() && !is_valid_token(value) {
        return Err(RecipeError::with_token(kind, line, value));
    }
    Ok(())
}

/// Validates that every entry in `entries` satisfies [`is_valid_text`].
fn validate_text_entries(
    entries: &[String],
    kind: RecipeErrorKind,
    line: usize,
) -> Result<(), RecipeError> {
    for entry in entries {
        if !is_valid_text(entry) {
            return Err(RecipeError::with_token(kind, line, entry));
        }
    }
    Ok(())
}

/// Validates that every volume mount in `volumes` satisfies [`is_valid_volume_mount`].
fn validate_volume_mounts(
    volumes: &[VolumeMount],
    line: usize,
    label: &str,
) -> Result<(), RecipeError> {
    for volume in volumes {
        if !is_valid_volume_mount(volume) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidVolume,
                line,
                label,
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};

    use crate::core::log::MAX_LOG_MESSAGE_BYTES;
    use crate::core::pod::Pod;
    use crate::error::{PodError, RecipeErrorKind};
    use crate::utilities::collection::append_option_tokens;
    use super::{
        Container, VolumeMount, container_label, create, create_with, create_with_pod_check, down,
        down_with, format_volume_mount, is_valid_volume_mount, pull, pull_and_check_with,
        pull_with, reload, reload_with, remove, remove_with, resolve_container_name_value,
        resolve_host_volume_path, resolve_image_reference, restart, restart_with, start,
        start_with, stop, stop_with, up, up_with, update, update_with, update_with_query,
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

        container.image = String::from("-invalid:latest");
        let invalid_image_error = container.validate().unwrap_err();
        assert_eq!(invalid_image_error.kind, RecipeErrorKind::InvalidImage);

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

        for invalid_pod_id in ["", "--bad", "-p", "bad\npod"] {
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
    fn container_lifecycle_commands_and_simulation() {
        let mut pod = Pod::new();
        pod.id = String::from("commandos");
        pod.name = String::from("Commandos");
        pod.path = PathBuf::from("/my_pod_path");
        pod.registry = String::from("bel.io");

        let mut container = Container::new();
        container.name = String::from("commandos-1");
        container.image = String::from("squad:1998");
        container.restart = String::from("on-failure:5");
        container.options = vec![
            String::from("--unknown thing"),
            String::from("--another"),
            String::from("unknown"),
        ];
        container.volumes.push(VolumeMount {
            container_dir: String::from("/container/dir/relative"),
            host_dir: String::from("./relative/path"),
            options: String::from("z"),
        });
        container.commands = vec![
            String::from("O'Hara"),
            String::from("Hancock 2"),
            String::from("Woolridge3"),
            String::from("Brooklyn:4"),
            String::from("Blackwood=5"),
            String::from("Duchamp-6"),
        ];

        assert_eq!(resolve_image_reference(&container, &pod), "bel.io/squad:1998");
        container.registry = String::from("bestRegEver.io");
        assert_eq!(
            resolve_image_reference(&container, &pod),
            "bestRegEver.io/squad:1998"
        );
        container.registry.clear();
        assert!(matches!(
            resolve_image_reference(&container, &Pod::new()),
            Cow::Borrowed("squad:1998")
        ));

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        create_with(&container, &pod, true, &mut stdout, &mut stderr).unwrap();
        start_with(&container, true, &mut stdout, &mut stderr).unwrap();
        stop_with(&container, true, &mut stdout, &mut stderr).unwrap();
        remove_with(&container, true, &mut stdout, &mut stderr).unwrap();
        pull_with(&container, &pod, true, &mut stdout, &mut stderr).unwrap();

        let output = String::from_utf8(stdout).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(
            lines,
            vec![
                "Create pod 'Commandos' ('commandos'): ",
                "podman pod create --name commandos",
                "Create container 'commandos-1': ",
                "podman create --name commandos-1 --pod commandos --restart on-failure:5 --volume /my_pod_path/relative/path:/container/dir/relative:z --unknown thing --another unknown bel.io/squad:1998 'O'\\''Hara' 'Hancock 2' Woolridge3 Brooklyn:4 Blackwood=5 Duchamp-6",
                "Start container 'commandos-1': ",
                "podman start commandos-1",
                "Stop container 'commandos-1': ",
                "podman stop commandos-1",
                "Remove container 'commandos-1': ",
                "podman rm commandos-1",
                "Update container 'commandos-1' ...",
                "podman pull bel.io/squad:1998",
            ]
        );
        assert!(stderr.is_empty());

        // When the pod ALREADY exists in Podman, create_with_pod_check skips pod creation.
        let mut existing_pod = Pod::new();
        existing_pod.id = String::from("existing_pod");
        existing_pod.name = String::from("Existing Pod");
        existing_pod.path = PathBuf::from("/my_pod_path");
        existing_pod.registry = String::from("bel.io");
        let mut existing_out = Vec::new();
        create_with_pod_check(
            &container,
            &existing_pod,
            true,
            |_| true,
            &mut existing_out,
            &mut stderr,
        )
        .unwrap();
        let existing_lines: Vec<&str> = std::str::from_utf8(&existing_out)
            .unwrap()
            .lines()
            .collect();
        assert_eq!(existing_lines.len(), 2);
        assert_eq!(existing_lines[0], "Create container 'commandos-1': ");

        let mut composite_out = Vec::new();
        up_with(&container, &pod, true, &mut composite_out, &mut stderr).unwrap();
        down_with(&container, true, &mut composite_out, &mut stderr).unwrap();
        restart_with(&container, true, &mut composite_out, &mut stderr).unwrap();
        reload_with(&container, &pod, true, &mut composite_out, &mut stderr).unwrap();
        update_with(&container, &pod, true, &mut composite_out, &mut stderr).unwrap();
        let composite_lines: Vec<&str> = std::str::from_utf8(&composite_out)
            .unwrap()
            .lines()
            .collect();
        assert_eq!(composite_lines.len(), 30);

        let mut spaced_pod = pod.clone();
        spaced_pod.path = PathBuf::from("/pods/my pod");
        let mut spaced_container = container.clone();
        spaced_container.volumes = vec![VolumeMount {
            container_dir: String::from("/container/my dir"),
            host_dir: String::from("./my host"),
            options: String::from("ro"),
        }];
        let mut spaced_out = Vec::new();
        reload_with(&spaced_container, &spaced_pod, true, &mut spaced_out, &mut stderr).unwrap();
        assert!(
            String::from_utf8(spaced_out)
                .unwrap()
                .contains("--volume '/pods/my pod/my host:/container/my dir:ro'")
        );

        // Exercise the standard stream wrappers in simulate mode.
        assert!(create(&container, &pod, true).is_ok());
        assert!(start(&container, true).is_ok());
        assert!(stop(&container, true).is_ok());
        assert!(remove(&container, true).is_ok());
        assert!(pull(&container, &pod, true).is_ok());
        assert!(up(&container, &pod, true).is_ok());
        assert!(down(&container, true).is_ok());
        assert!(restart(&container, true).is_ok());
        assert!(reload(&container, &pod, true).is_ok());
        assert!(update(&container, &pod, true).is_ok());
    }

    #[test]
    fn container_update_with_sanitizes_errors_and_tolerates_broken_pipe() {
        let mut pod = Pod::new();
        pod.id = String::from("app_pod");
        pod.path = PathBuf::from("/pods/app_pod");

        let mut container = Container::new();
        container.name = String::from("app_1");
        container.image = String::from("alpine:latest");

        let mut broken_stdout = BrokenPipeWriter;
        let mut stderr = Vec::new();
        assert!(pull_with(&container, &pod, true, &mut broken_stdout, &mut stderr).is_ok());

        let mut injected = Container::new();
        injected.name = format!(
            "bad\x1b[31m\n\u{202E}{}",
            "a".repeat(MAX_LOG_MESSAGE_BYTES)
        );
        injected.image = String::from("alpine:latest");

        let mut captured_stdout = Vec::new();
        let update_error =
            pull_with(&injected, &pod, true, &mut captured_stdout, &mut stderr).unwrap_err();
        assert!(captured_stdout.is_empty());
        let formatted_error = update_error.to_string();
        assert!(!formatted_error.contains('\x1b'));
        assert!(!formatted_error.contains('\u{202E}'));
        assert_eq!(formatted_error.lines().count(), 1);

        // Conditional reload on container update:
        // 1. Image ID unchanged (Some("id1") -> Some("id1")) skips reload and prints up-to-date message.
        let mut unchanged_out = Vec::new();
        let mut same_id = |_: &str| Some(String::from("sha256:id1"));
        update_with_query(
            &container,
            &pod,
            true,
            &mut same_id,
            &mut unchanged_out,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(unchanged_out).unwrap().lines().collect::<Vec<_>>(),
            vec![
                "Update container 'app_1' ...",
                "podman pull alpine:latest",
                "Container 'app_1' is up to date.",
            ]
        );

        // 2. Image missing before and after pull (None -> None) returns false and skips reload.
        let mut missing_out = Vec::new();
        let mut no_id = |_: &str| None;
        assert!(
            !pull_and_check_with(
                &container,
                &pod,
                true,
                &mut no_id,
                &mut missing_out,
                &mut stderr,
            )
            .unwrap()
        );

        // 3. Image ID changes (Some("id1") -> Some("id2")) triggers reload.
        let mut changed_out = Vec::new();
        let mut call_count = 0usize;
        let mut changing_id = |_: &str| {
            call_count += 1;
            if call_count == 1 {
                Some(String::from("sha256:id1"))
            } else {
                Some(String::from("sha256:id2"))
            }
        };
        update_with_query(
            &container,
            &pod,
            true,
            &mut changing_id,
            &mut changed_out,
            &mut stderr,
        )
        .unwrap();
        let changed_lines: Vec<&str> = std::str::from_utf8(&changed_out).unwrap().lines().collect();
        assert!(changed_lines.contains(&"Stop container 'app_1': "));
        assert!(changed_lines.contains(&"Create container 'app_1': "));
        assert!(changed_lines.contains(&"Start container 'app_1': "));
        assert!(!changed_lines.contains(&"Container 'app_1' is up to date."));
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

        let mut bad_registry = base.clone();
        bad_registry.registry = String::from("--privileged");
        assert_eq!(
            bad_registry.validate().unwrap_err().kind,
            RecipeErrorKind::InvalidRegistry
        );

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
                pull_with(&target, &pod, true, &mut Vec::new(), &mut Vec::new()),
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
            create_with(
                &base,
                &bad_pod_registry,
                true,
                &mut Vec::new(),
                &mut Vec::new()
            ),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidRegistry
        ));
        assert!(matches!(
            pull_with(&base, &bad_pod_registry, true, &mut Vec::new(), &mut Vec::new()),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidRegistry
        ));
        assert!(matches!(
            update_with(&base, &bad_pod_registry, true, &mut Vec::new(), &mut Vec::new()),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidRegistry
        ));

        let mut with_relative_vol = base.clone();
        with_relative_vol.volumes.push(VolumeMount {
            container_dir: String::from("/data"),
            host_dir: String::from("./data"),
            options: String::new(),
        });
        let mut bad_pod_path = pod.clone();
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
                &mut Vec::new()
            ),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidVolume
        ));
        assert!(matches!(
            update_with(
                &with_relative_vol,
                &bad_pod_path,
                true,
                &mut Vec::new(),
                &mut Vec::new()
            ),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidVolume
        ));
    }

    #[test]
    fn container_volume_formatting_and_options() {
        let pod_path = Path::new("/my_pod_path");
        assert!(matches!(
            resolve_host_volume_path("/absolute/path", pod_path, 1, "app").unwrap(),
            Cow::Borrowed("/absolute/path")
        ));
        assert_eq!(
            resolve_host_volume_path("data", Path::new("pods"), 1, "app").unwrap(),
            Cow::Owned::<str>(String::from("./pods/data"))
        );
        assert_eq!(
            resolve_host_volume_path(".", Path::new("."), 1, "app").unwrap(),
            Cow::Owned::<str>(String::from("."))
        );
        assert_eq!(
            resolve_host_volume_path("data", Path::new("/my_pod_path/"), 1, "app").unwrap(),
            Cow::Owned::<str>(String::from("/my_pod_path/data"))
        );
        assert_eq!(
            resolve_host_volume_path(".", Path::new("/my_pod_path/"), 1, "app").unwrap(),
            Cow::Owned::<str>(String::from("/my_pod_path"))
        );
        assert_eq!(
            resolve_host_volume_path(".//data/", Path::new("/my_pod_path"), 1, "app").unwrap(),
            Cow::Owned::<str>(String::from("/my_pod_path/data"))
        );
        assert_eq!(
            resolve_host_volume_path(".//./data", Path::new("/my_pod_path"), 1, "app").unwrap(),
            Cow::Owned::<str>(String::from("/my_pod_path/data"))
        );
        assert_eq!(
            resolve_host_volume_path(".//.", Path::new("/my_pod_path"), 1, "app").unwrap(),
            Cow::Owned::<str>(String::from("/my_pod_path"))
        );
        assert_eq!(
            resolve_host_volume_path(".//", Path::new("/my_pod_path"), 1, "app").unwrap(),
            Cow::Owned::<str>(String::from("/my_pod_path"))
        );

        let cases = [
            ("", "/container/dir/anonymous", "", "/container/dir/anonymous"),
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
        for invalid_pod_path in [
            Path::new(""),
            Path::new("/pods:ro"),
            Path::new("../escape"),
        ] {
            let error =
                format_volume_mount(&relative_volume, invalid_pod_path, 9, "app").unwrap_err();
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
                "unknown value"
            ]
        );
    }
}
