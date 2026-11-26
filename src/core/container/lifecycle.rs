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

//! Podman container lifecycle orchestration (`create`, `start`, `stop`, `remove`, `up`, `down`, `restart`, `reload`).

use std::io::{self, Write};

use crate::core::pod::Pod;
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::{self, CreateContainer};
use super::{Container, update, validation, volume};

/// Creates `container` in Podman (`podman create`), creating `pod` first if it does not exist, without starting it.
///
/// # Errors
///
/// Returns [`PodError`] if validation, volume resolution, or Podman execution fails.
pub fn create(container: &Container, pod: &Pod, simulate: bool) -> Result<ExecOutcome, PodError> {
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
) -> Result<ExecOutcome, PodError> {
    create_with_pod_check(container, pod, simulate, podman::pod_exists, stdout, stderr)
}

/// Stops and removes `container` in Podman (`stop` + `remove`).
///
/// # Errors
///
/// Returns [`PodError`] if stopping or removing the container fails.
pub fn down(container: &Container, simulate: bool) -> Result<ExecOutcome, PodError> {
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
) -> Result<ExecOutcome, PodError> {
    validation::validate_container_name(container)?;
    let stop_outcome = stop_with(container, simulate, stdout, stderr)?;
    let remove_outcome = remove_with(container, simulate, stdout, stderr)?;
    Ok(stop_outcome.combine(remove_outcome))
}

/// Stops, removes, recreates, and starts `container` inside `pod` (`down` + `up`).
///
/// # Errors
///
/// Returns [`PodError`] if tearing down or bringing up the container fails.
pub fn reload(container: &Container, pod: &Pod, simulate: bool) -> Result<ExecOutcome, PodError> {
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
) -> Result<ExecOutcome, PodError> {
    container.validate_create(pod)?;
    let down_outcome = down_with(container, simulate, stdout, stderr)?;
    let up_outcome = up_with(container, pod, simulate, stdout, stderr)?;
    Ok(down_outcome.combine(up_outcome))
}

/// Removes `container` in Podman (`podman rm`).
///
/// # Errors
///
/// Returns [`PodError`] if Podman execution fails.
pub fn remove(container: &Container, simulate: bool) -> Result<ExecOutcome, PodError> {
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
) -> Result<ExecOutcome, PodError> {
    validation::validate_container_name(container)?;
    let prefix = format!("Remove container '{}': ", container.name);
    podman::remove_container(&container.name, &prefix, simulate, stdout, stderr)
}

/// Stops and starts `container` in Podman (`stop` + `start`).
///
/// # Errors
///
/// Returns [`PodError`] if stopping or starting the container fails.
pub fn restart(container: &Container, simulate: bool) -> Result<ExecOutcome, PodError> {
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
) -> Result<ExecOutcome, PodError> {
    validation::validate_container_name(container)?;
    let stop_outcome = stop_with(container, simulate, stdout, stderr)?;
    let start_outcome = start_with(container, simulate, stdout, stderr)?;
    Ok(stop_outcome.combine(start_outcome))
}

/// Starts `container` in Podman (`podman start`).
///
/// # Errors
///
/// Returns [`PodError`] if Podman execution fails.
pub fn start(container: &Container, simulate: bool) -> Result<ExecOutcome, PodError> {
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
) -> Result<ExecOutcome, PodError> {
    validation::validate_container_name(container)?;
    let prefix = format!("Start container '{}': ", container.name);
    podman::start_container(&container.name, &prefix, simulate, stdout, stderr)
}

/// Stops `container` in Podman (`podman stop`).
///
/// # Errors
///
/// Returns [`PodError`] if Podman execution fails.
pub fn stop(container: &Container, simulate: bool) -> Result<ExecOutcome, PodError> {
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
) -> Result<ExecOutcome, PodError> {
    validation::validate_container_name(container)?;
    let prefix = format!("Stop container '{}': ", container.name);
    podman::stop_container(&container.name, &prefix, simulate, stdout, stderr)
}

/// Creates and starts `container` inside `pod` (`create` + `start`).
///
/// # Errors
///
/// Returns [`PodError`] if creating or starting the container fails.
pub fn up(container: &Container, pod: &Pod, simulate: bool) -> Result<ExecOutcome, PodError> {
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
) -> Result<ExecOutcome, PodError> {
    let create_outcome = create_with(container, pod, simulate, stdout, stderr)?;
    let start_outcome = start_with(container, simulate, stdout, stderr)?;
    Ok(create_outcome.combine(start_outcome))
}

/// Executes `podman create` for an already validated `container` and `pod` without creating `pod`.
pub(crate) fn create_validated<W: Write, E: Write>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    let label = validation::container_label(&container.name);
    let volumes =
        volume::format_volume_mounts(&container.volumes, &pod.path, container.line, label)?;
    let image = update::resolve_image_reference(container, pod);
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

/// Validates `container` and `pod`, ensures `pod` exists via `pod_exists`, and executes `podman create`.
pub(super) fn create_with_pod_check<W, E, F>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    pod_exists: F,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError>
where
    W: Write,
    E: Write,
    F: FnOnce(&str) -> bool,
{
    container.validate_create(pod)?;
    let pod_outcome = pod.ensure_created_with(simulate, pod_exists, stdout, stderr)?;
    let create_outcome = create_validated(container, pod, simulate, stdout, stderr)?;
    Ok(pod_outcome.combine(create_outcome))
}
