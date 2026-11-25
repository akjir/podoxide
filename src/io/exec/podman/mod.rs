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

//! Central Podman CLI API for pod, container, log, and image execution and inspection.

mod lifecycle;
mod query;
mod runtime;

use std::io::{self, Write};

use crate::error::PodError;
use crate::io::exec::command::{self, ExecOptions, ExecOutcome};
pub use self::lifecycle::{
    CreateContainer, CreatePod, create_container, create_pod, prune_images, remove_container,
    remove_pod, start_container, stop_container, stop_pod,
};
pub use self::query::{
    FIELD_DELIMITER, RECORD_DELIMITER, check_version, container_running, pod_exists,
    query_image_id, query_images, query_in_use_images, query_ps_status,
};
pub use self::runtime::{
    ExecContainer, PodLogs, PullOutcome, SHELL_FALLBACK_COMMAND, exec_container, logs_pod,
    pull_image,
};

/// Podman CLI binary name.
const PODMAN_BIN: &str = "podman";

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

/// Executes `podman` with `arguments` and returns bounded captured standard output when exit status is `0`.
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
) -> Result<ExecOutcome, PodError>
where
    S: AsRef<str>,
    W: Write,
    E: Write,
{
    command::exec_with(PODMAN_BIN, arguments, options, stdout, stderr)
}
