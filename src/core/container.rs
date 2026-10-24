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

use std::io::{self, Write};
use std::path::Path;

use crate::core::log;
use crate::core::pod::Pod;
use crate::error::{PodError, RecipeError, RecipeErrorKind};
use crate::io::exec::command::{self, ExecOptions};
use crate::utilities::path::{build_full_path, is_valid_directory_path};
use crate::utilities::string::{
    contains_control_or_whitespace, is_bidi_control, is_valid_pod_or_container_name, normalize_name,
};

const BASE_CREATE_ARG_COUNT: usize = 7;

/// Container specification and runtime arguments from a `container` block in a `.pod` recipe.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Container {
    pub commands: Vec<String>,
    pub detach: bool,
    pub image: String,
    pub line: usize,
    pub name: String,
    pub options: Vec<String>,
    pub registry: String,
    pub restart: String,
    pub volumes: Vec<VolumeMount>,
}

impl Container {
    /// Creates a new [`Container`] with default settings (`detach` enabled).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the `podman create` argument vector for this container inside `pod`.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] when pod/container fields are invalid or a volume mount cannot be resolved.
    pub fn build_create_args(&self, pod: &Pod) -> Result<Vec<String>, RecipeError> {
        let label = container_label(&self.name);
        validate_container_name(self)?;
        if !is_valid_pod_or_container_name(&pod.id) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidPodId,
                pod.line,
                &pod.id,
            ));
        }
        validate_optional_token(&pod.registry, RecipeErrorKind::InvalidRegistry, pod.line)?;
        self.validate()?;
        let capacity = BASE_CREATE_ARG_COUNT
            .saturating_add(self.volumes.len().saturating_mul(2))
            .saturating_add(self.options.len().saturating_mul(2))
            .saturating_add(self.commands.len());
        let mut arguments = Vec::with_capacity(capacity);
        arguments.push(String::from("create"));
        arguments.push(String::from("--name"));
        arguments.push(self.name.clone());
        arguments.push(String::from("--pod"));
        arguments.push(pod.id.clone());
        if !self.restart.is_empty() {
            arguments.push(String::from("--restart"));
            arguments.push(self.restart.clone());
        }
        append_volume_arguments(&mut arguments, &self.volumes, &pod.path, self.line, label)?;
        append_option_tokens(&mut arguments, &self.options);
        arguments.push(resolve_image_reference(self, pod));
        arguments.extend(self.commands.iter().cloned());
        Ok(arguments)
    }

    /// Builds the `podman rm` argument vector for this container.
    #[must_use]
    pub fn build_remove_args(&self) -> Vec<String> {
        vec![String::from("rm"), self.name.clone()]
    }

    /// Builds the `podman start` argument vector for this container, passing `--attach` when `detach` is `false`.
    #[must_use]
    pub fn build_start_args(&self) -> Vec<String> {
        if self.detach {
            vec![String::from("start"), self.name.clone()]
        } else {
            vec![
                String::from("start"),
                String::from("--attach"),
                self.name.clone(),
            ]
        }
    }

    /// Builds the `podman stop` argument vector for this container.
    #[must_use]
    pub fn build_stop_args(&self) -> Vec<String> {
        vec![String::from("stop"), self.name.clone()]
    }

    /// Builds the `podman pull` argument vector for this container's image inside `pod`.
    #[must_use]
    pub fn build_update_args(&self, pod: &Pod) -> Vec<String> {
        vec![String::from("pull"), resolve_image_reference(self, pod)]
    }

    /// Creates this container in Podman (`podman create`) without starting it.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if volume resolution or Podman execution fails.
    pub fn create(&self, pod: &Pod, simulate: bool) -> Result<(), PodError> {
        create(self, pod, simulate)
    }

    /// Creates this container in Podman (`podman create`), writing command output to `stdout` and `stderr`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if volume resolution, stream writing, or Podman execution fails.
    pub fn create_with<W: Write, E: Write>(
        &self,
        pod: &Pod,
        simulate: bool,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<(), PodError> {
        create_with(self, pod, simulate, stdout, stderr)
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

    /// Removes this container in Podman (`podman rm`).
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if Podman execution fails.
    pub fn remove(&self, simulate: bool) -> Result<(), PodError> {
        remove(self, simulate)
    }

    /// Removes this container in Podman (`podman rm`), writing command output to `stdout` and `stderr`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if stream writing or Podman execution fails.
    pub fn remove_with<W: Write, E: Write>(
        &self,
        simulate: bool,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<(), PodError> {
        remove_with(self, simulate, stdout, stderr)
    }

    /// Starts this container in Podman (`podman start`).
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if Podman execution fails.
    pub fn start(&self, simulate: bool) -> Result<(), PodError> {
        start(self, simulate)
    }

    /// Starts this container in Podman (`podman start`), writing command output to `stdout` and `stderr`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if stream writing or Podman execution fails.
    pub fn start_with<W: Write, E: Write>(
        &self,
        simulate: bool,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<(), PodError> {
        start_with(self, simulate, stdout, stderr)
    }

    /// Stops this container in Podman (`podman stop`).
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if Podman execution fails.
    pub fn stop(&self, simulate: bool) -> Result<(), PodError> {
        stop(self, simulate)
    }

    /// Stops this container in Podman (`podman stop`), writing command output to `stdout` and `stderr`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if stream writing or Podman execution fails.
    pub fn stop_with<W: Write, E: Write>(
        &self,
        simulate: bool,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<(), PodError> {
        stop_with(self, simulate, stdout, stderr)
    }

    /// Pulls the latest image for this container in Podman (`podman pull`).
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if Podman execution fails.
    pub fn update(&self, pod: &Pod, simulate: bool) -> Result<(), PodError> {
        update(self, pod, simulate)
    }

    /// Pulls the latest image for this container in Podman (`podman pull`), writing output to `stdout` and `stderr`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if stream writing or Podman execution fails.
    pub fn update_with<W: Write, E: Write>(
        &self,
        pod: &Pod,
        simulate: bool,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<(), PodError> {
        update_with(self, pod, simulate, stdout, stderr)
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
        validate_text_entries(&self.options, RecipeErrorKind::InvalidOption, self.line)?;
        validate_text_entries(&self.commands, RecipeErrorKind::InvalidCommand, self.line)?;
        validate_volume_mounts(&self.volumes, self.line, label)
    }
}

impl Default for Container {
    fn default() -> Self {
        Self {
            commands: Vec::new(),
            detach: true,
            image: String::new(),
            line: 0,
            name: String::new(),
            options: Vec::new(),
            registry: String::new(),
            restart: String::new(),
            volumes: Vec::new(),
        }
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

/// Appends `option` directive tokens from `options` into `arguments`, splitting combined `--flag value` entries.
pub fn append_option_tokens(arguments: &mut Vec<String>, options: &[String]) {
    for option in options {
        append_single_option(arguments, option);
    }
}

/// Creates `container` in Podman (`podman create`) without starting it.
///
/// # Errors
///
/// Returns [`PodError`] if volume resolution or Podman execution fails.
pub fn create(container: &Container, pod: &Pod, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    create_with(container, pod, simulate, &mut stdout, &mut stderr)
}

/// Creates `container` in Podman (`podman create`), writing command output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if volume resolution, stream writing, or Podman execution fails.
pub fn create_with<W: Write, E: Write>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    let arguments = container.build_create_args(pod)?;
    let prefix = format!("Create container '{}': ", container.name);
    command::exec_with(
        "podman",
        &arguments,
        &ExecOptions {
            interactive: false,
            prefix: &prefix,
            simulate,
        },
        stdout,
        stderr,
    )
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
        String::new()
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
    if !volume.host_dir.is_empty() && !is_valid_directory_path(&volume.host_dir) {
        return false;
    }
    is_valid_volume_options(&volume.options)
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
    let arguments = container.build_remove_args();
    let prefix = format!("Remove container '{}': ", container.name);
    command::exec_with(
        "podman",
        &arguments,
        &ExecOptions {
            interactive: false,
            prefix: &prefix,
            simulate,
        },
        stdout,
        stderr,
    )
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
        return Ok(format!("{pod_id}-{index}"));
    }
    if raw_name
        .chars()
        .any(|character| character.is_control() || is_bidi_control(character))
    {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidContainerName,
            line,
            raw_name,
        ));
    }
    let normalized = normalize_name(raw_name);
    if let Some(suffix) = normalized.strip_prefix('*') {
        if !is_valid_pod_or_container_name(suffix) {
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
pub fn resolve_image_reference(container: &Container, pod: &Pod) -> String {
    let registry = if container.registry.is_empty() {
        pod.registry.as_str()
    } else {
        container.registry.as_str()
    };
    if registry.is_empty() {
        return container.image.clone();
    }
    format!("{registry}/{}", container.image)
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
    let arguments = container.build_start_args();
    let prefix = format!("Start container '{}': ", container.name);
    command::exec_with(
        "podman",
        &arguments,
        &ExecOptions {
            interactive: !container.detach,
            prefix: &prefix,
            simulate,
        },
        stdout,
        stderr,
    )
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
    let arguments = container.build_stop_args();
    let prefix = format!("Stop container '{}': ", container.name);
    command::exec_with(
        "podman",
        &arguments,
        &ExecOptions {
            interactive: false,
            prefix: &prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Pulls the latest image for `container` in Podman (`podman pull`).
///
/// # Errors
///
/// Returns [`PodError`] if Podman execution fails.
pub fn update(container: &Container, pod: &Pod, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    update_with(container, pod, simulate, &mut stdout, &mut stderr)
}

/// Pulls the latest image for `container` in Podman (`podman pull`), writing output to `stdout` and `stderr`.
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
    container.validate()?;
    validate_optional_token(&pod.registry, RecipeErrorKind::InvalidRegistry, pod.line)?;
    log::write_print(stdout, &format!("Update container '{}' ...", container.name));
    let arguments = container.build_update_args(pod);
    command::exec_with(
        "podman",
        &arguments,
        &ExecOptions {
            interactive: false,
            prefix: "",
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Appends a single `option` directive token (or whitespace-split `--flag value` pair) to `arguments`.
fn append_single_option(arguments: &mut Vec<String>, option: &str) {
    let trimmed = option.trim();
    if trimmed.starts_with('-') && trimmed.contains(char::is_whitespace) {
        arguments.extend(trimmed.split_whitespace().map(str::to_owned));
    } else {
        arguments.push(option.to_owned());
    }
}

/// Appends `--volume <mount>` pairs for `volumes` into `arguments`.
fn append_volume_arguments(
    arguments: &mut Vec<String>,
    volumes: &[VolumeMount],
    pod_path: &Path,
    line: usize,
    label: &str,
) -> Result<(), RecipeError> {
    for volume in volumes {
        let mount = format_volume_mount(volume, pod_path, line, label)?;
        arguments.push(String::from("--volume"));
        arguments.push(mount);
    }
    Ok(())
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

/// Evaluates whether `value` contains non-whitespace characters and is free of control and Unicode BiDi characters.
#[must_use]
fn is_valid_text_entry(value: &str) -> bool {
    !value.trim().is_empty()
        && !value
            .chars()
            .any(|character| character.is_control() || is_bidi_control(character))
}

/// Evaluates whether `value` is a non-empty single token without leading hyphens, whitespace, or control characters.
#[must_use]
fn is_valid_token(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-') && !contains_control_or_whitespace(value)
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

/// Resolves a non-empty `host_dir` against `pod_path` when `host_dir` is relative.
fn resolve_host_volume_path(
    host_dir: &str,
    pod_path: &Path,
    line: usize,
    label: &str,
) -> Result<String, RecipeError> {
    if host_dir.starts_with('/') {
        return Ok(host_dir.to_owned());
    }
    if !pod_path.to_str().is_some_and(is_valid_directory_path) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidVolume,
            line,
            label,
        ));
    }
    let mut rest = host_dir;
    while let Some(stripped) = rest.strip_prefix("./") {
        rest = stripped;
    }
    let clean_host = if rest == "." { "" } else { rest };
    let resolved = build_full_path(pod_path, clean_host, "");
    let Some(resolved_str) = resolved.to_str() else {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidVolume,
            line,
            label,
        ));
    };
    Ok(resolved_str.to_owned())
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

/// Validates that every entry in `entries` satisfies [`is_valid_text_entry`].
fn validate_text_entries(
    entries: &[String],
    kind: RecipeErrorKind,
    line: usize,
) -> Result<(), RecipeError> {
    for entry in entries {
        if !is_valid_text_entry(entry) {
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
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};

    use crate::core::log::MAX_LOG_MESSAGE_BYTES;
    use crate::core::pod::Pod;
    use crate::error::{PodError, RecipeErrorKind};
    use super::{
        Container, VolumeMount, append_option_tokens, container_label, format_volume_mount,
        is_valid_volume_mount, resolve_container_name_value, resolve_image_reference,
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
        assert!(container.detach);
        assert!(container.name.is_empty());
        assert_eq!(container_label(&container.name), "container");

        let unnamed_missing_image = container.validate().unwrap_err();
        assert_eq!(
            unnamed_missing_image.kind,
            RecipeErrorKind::MissingContainerImage
        );
        assert_eq!(unnamed_missing_image.token.as_deref(), Some("container"));

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

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        container
            .create_with(&pod, true, &mut stdout, &mut stderr)
            .unwrap();
        container
            .start_with(true, &mut stdout, &mut stderr)
            .unwrap();
        container.detach = false;
        container
            .start_with(true, &mut stdout, &mut stderr)
            .unwrap();
        container
            .stop_with(true, &mut stdout, &mut stderr)
            .unwrap();
        container
            .remove_with(true, &mut stdout, &mut stderr)
            .unwrap();
        container
            .update_with(&pod, true, &mut stdout, &mut stderr)
            .unwrap();

        let output = String::from_utf8(stdout).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(
            lines,
            vec![
                "Create container 'commandos-1': ",
                "podman create --name commandos-1 --pod commandos --restart on-failure:5 --volume /my_pod_path/relative/path:/container/dir/relative:z --unknown thing --another unknown bel.io/squad:1998 'O'\\''Hara' 'Hancock 2' Woolridge3 Brooklyn:4 Blackwood=5 Duchamp-6",
                "Start container 'commandos-1': ",
                "podman start commandos-1",
                "Start container 'commandos-1': ",
                "podman start --attach commandos-1",
                "Stop container 'commandos-1': ",
                "podman stop commandos-1",
                "Remove container 'commandos-1': ",
                "podman rm commandos-1",
                "Update container 'commandos-1' ...",
                "podman pull bel.io/squad:1998",
            ]
        );
        assert!(stderr.is_empty());

        // Exercise the standard stream wrappers in simulate mode.
        assert!(container.create(&pod, true).is_ok());
        assert!(container.start(true).is_ok());
        assert!(container.stop(true).is_ok());
        assert!(container.remove(true).is_ok());
        assert!(container.update(&pod, true).is_ok());
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
        assert!(
            container
                .update_with(&pod, true, &mut broken_stdout, &mut stderr)
                .is_ok()
        );

        let mut injected = Container::new();
        injected.name = format!(
            "bad\x1b[31m\n\u{202E}{}",
            "a".repeat(MAX_LOG_MESSAGE_BYTES)
        );
        injected.image = String::from("alpine:latest");

        let mut captured_stdout = Vec::new();
        let update_error = injected
            .update_with(&pod, true, &mut captured_stdout, &mut stderr)
            .unwrap_err();
        assert!(captured_stdout.is_empty());
        let formatted_error = update_error.to_string();
        assert!(!formatted_error.contains('\x1b'));
        assert!(!formatted_error.contains('\u{202E}'));
        assert_eq!(formatted_error.lines().count(), 1);
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

        let arguments_without_restart = base.build_create_args(&pod).unwrap();
        assert!(
            !arguments_without_restart
                .iter()
                .any(|argument| argument == "--restart")
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
                target.build_create_args(&pod).unwrap_err().kind,
                RecipeErrorKind::InvalidContainerName
            );
            assert!(matches!(
                target.create_with(&pod, true, &mut Vec::new(), &mut Vec::new()),
                Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidContainerName
            ));
            assert!(matches!(
                target.start_with(true, &mut Vec::new(), &mut Vec::new()),
                Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidContainerName
            ));
            assert!(matches!(
                target.stop_with(true, &mut Vec::new(), &mut Vec::new()),
                Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidContainerName
            ));
            assert!(matches!(
                target.remove_with(true, &mut Vec::new(), &mut Vec::new()),
                Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidContainerName
            ));
        }

        let mut bad_pod_id = pod.clone();
        bad_pod_id.id = String::from("--all");
        assert_eq!(
            base.build_create_args(&bad_pod_id).unwrap_err().kind,
            RecipeErrorKind::InvalidPodId
        );

        let mut bad_pod_registry = pod.clone();
        bad_pod_registry.registry = String::from("--privileged");
        assert_eq!(
            base.build_create_args(&bad_pod_registry).unwrap_err().kind,
            RecipeErrorKind::InvalidRegistry
        );
        assert!(matches!(
            base.update_with(&bad_pod_registry, true, &mut Vec::new(), &mut Vec::new()),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::InvalidRegistry
        ));
    }

    #[test]
    fn container_volume_formatting_and_options() {
        let pod_path = Path::new("/my_pod_path");
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
