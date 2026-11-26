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

//! Podman CLI runtime operations for container execution, pod logs, and image pulling.

use std::borrow::Cow;
use std::env;
use std::ffi::OsStr;
use std::io::{self, IsTerminal, Write};

use crate::error::PodError;
use crate::io::exec::command::{ExecMode, ExecOptions, ExecOutcome, InterruptPolicy};
use crate::io::exec::spinner;
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::string;
use super::query;

/// Base argument count (`exec`, `-it`, `-u`, `<user>`, `<container>`) for `podman exec`.
const BASE_CONTAINER_EXEC_ARG_COUNT: usize = 5;
/// Upper bound on the number of arguments passed to `podman pod logs`.
const MAX_POD_LOGS_ARGS: usize = 16;
/// Command-line arguments executed inside a container to launch an interactive `bash` or `sh` shell.
pub const SHELL_FALLBACK_COMMAND: [&str; 3] = [
    "sh",
    "-c",
    "if command -v bash >/dev/null 2>&1; then exec bash; fi; exec sh",
];

/// Parameters for executing a command inside a running container via [`exec_container`] (`podman exec`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecContainer<'a, S: AsRef<str>> {
    pub command: &'a [S],
    pub container_name: &'a str,
    pub interactive: bool,
    pub user: Option<&'a str>,
}

/// Parameters for fetching or streaming pod logs via [`logs_pod`] (`podman pod logs`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PodLogs<'a> {
    pub container_name: Option<&'a str>,
    pub follow: bool,
    pub pod_name: &'a str,
    pub since: Option<&'a str>,
    pub tail: Option<&'a str>,
    pub timestamps: bool,
    pub until: Option<&'a str>,
}

/// Outcome of executing `podman pull` for a container image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PullOutcome {
    Failed,
    Simulated,
    UpToDate,
    Updated,
}

impl PullOutcome {
    /// Returns the terminal spinner completion suffix for live `podman pull` output.
    #[must_use]
    const fn spinner_suffix(self) -> &'static str {
        match self {
            Self::Failed => "Failed.",
            Self::Simulated => "Simulated.",
            Self::UpToDate => "Up to date.",
            Self::Updated => "Updated.",
        }
    }
}

/// Executes `podman exec [-it|-i] [-u <user>] <container_name> <command...>` and returns its [`ExecOutcome`].
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
) -> Result<ExecOutcome, PodError> {
    validate_exec_container(request)?;
    let is_tty = if simulate {
        request.interactive
    } else {
        io::stdin().is_terminal() && io::stdout().is_terminal()
    };
    let arguments = build_container_exec_args(request, is_tty);
    let mode = if request.interactive {
        ExecMode::Interactive
    } else {
        ExecMode::Captured
    };
    super::run_podman_with(
        &arguments,
        &ExecOptions {
            interrupt_policy: InterruptPolicy::WarnOnInterrupt,
            mode,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Executes `podman pod logs -n [--color] [-f] [--since <since>] [--until <until>] [--tail <tail>] [--timestamps] [-c <container_name>] <pod_name>` and returns its [`ExecOutcome`].
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
) -> Result<ExecOutcome, PodError> {
    validate_pod_logs(request)?;
    let no_color = env::var_os("NO_COLOR");
    let use_color = should_use_log_color(simulate, io::stdout().is_terminal(), no_color.as_deref());
    let arguments = build_pod_logs_args(request, use_color);
    let interrupt_policy = if request.follow {
        InterruptPolicy::QuietOnSigint
    } else {
        InterruptPolicy::WarnOnInterrupt
    };
    super::run_podman_with(
        &arguments,
        &ExecOptions {
            interrupt_policy,
            mode: ExecMode::Interactive,
            prefix,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Executes `podman pull <image_reference>` while displaying progress and inspecting the local image ID before and after to report [`PullOutcome`].
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
) -> Result<PullOutcome, PodError> {
    let mut query_id = query::query_image_id;
    let mut run_spinner = |arguments: &[&str],
                           line_prefix: &str,
                           is_simulated: bool,
                           resolve: &mut dyn FnMut(ExecOutcome) -> &'static str,
                           out: &mut W,
                           err: &mut E| {
        spinner::exec_spinner_with(
            super::PODMAN_BIN,
            arguments,
            line_prefix,
            is_simulated,
            resolve,
            out,
            err,
        )
    };
    pull_image_with(
        image_reference,
        prefix,
        simulate,
        &mut query_id,
        &mut run_spinner,
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

/// Builds the `podman exec` argument vector from `request` and terminal state `is_tty`.
#[must_use]
fn build_container_exec_args<'a, S: AsRef<str>>(
    request: &'a ExecContainer<'a, S>,
    is_tty: bool,
) -> Vec<&'a str> {
    let capacity = collection::bounded_capacity(
        BASE_CONTAINER_EXEC_ARG_COUNT.saturating_add(request.command.len()),
        MAX_COLLECTION_PREALLOC,
    );
    let mut arguments = Vec::with_capacity(capacity);
    arguments.push("exec");
    if request.interactive {
        if is_tty {
            arguments.push("-it");
        } else {
            arguments.push("-i");
        }
    }
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

/// Builds the `podman pod logs` argument vector from `request` and color setting `use_color`.
#[must_use]
fn build_pod_logs_args<'a>(request: &'a PodLogs<'a>, use_color: bool) -> Vec<&'a str> {
    let mut arguments = Vec::with_capacity(MAX_POD_LOGS_ARGS);
    arguments.push("pod");
    arguments.push("logs");
    arguments.push("-n");
    if use_color {
        arguments.push("--color");
    }
    if request.follow {
        arguments.push("-f");
    }
    append_flag_pair(&mut arguments, "--since", request.since);
    append_flag_pair(&mut arguments, "--until", request.until);
    let resolved_tail = request.tail.map(|value| {
        if value.eq_ignore_ascii_case("all") {
            "-1"
        } else {
            value
        }
    });
    append_flag_pair(&mut arguments, "--tail", resolved_tail);
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

/// Orchestrates `podman pull` with injectable image-ID lookup and spinner runner for deterministic testing.
fn pull_image_with<W, E, Q, R>(
    image_reference: &str,
    prefix: &str,
    simulate: bool,
    query_id: &mut Q,
    run_spinner: &mut R,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<PullOutcome, PodError>
where
    W: Write,
    E: Write,
    Q: FnMut(&str) -> Option<String>,
    R: FnMut(
        &[&str],
        &str,
        bool,
        &mut dyn FnMut(ExecOutcome) -> &'static str,
        &mut W,
        &mut E,
    ) -> Result<(), PodError>,
{
    super::ensure_valid(
        string::is_valid_token(image_reference),
        "invalid image reference",
    )?;
    if simulate {
        let mut simulated_summary = |_: ExecOutcome| PullOutcome::Simulated.spinner_suffix();
        run_spinner(
            &["pull", image_reference],
            prefix,
            true,
            &mut simulated_summary,
            stdout,
            stderr,
        )?;
        return Ok(PullOutcome::Simulated);
    }
    let pull_prefix = if prefix.is_empty() {
        Cow::Owned(format!("Trying to pull {image_reference}... "))
    } else {
        Cow::Borrowed(prefix)
    };
    let before_id = query_id(image_reference);
    let mut outcome = PullOutcome::Failed;
    let mut resolve = |status: ExecOutcome| {
        let after_id = if status == ExecOutcome::Succeeded {
            query_id(image_reference)
        } else {
            None
        };
        outcome = resolve_pull_outcome(status, before_id.as_deref(), after_id.as_deref());
        outcome.spinner_suffix()
    };
    run_spinner(
        &["pull", image_reference],
        &pull_prefix,
        false,
        &mut resolve,
        stdout,
        stderr,
    )?;
    Ok(outcome)
}

/// Evaluates [`PullOutcome`] from the process exit status and before/after local image IDs.
#[must_use]
fn resolve_pull_outcome(
    status: ExecOutcome,
    before_id: Option<&str>,
    after_id: Option<&str>,
) -> PullOutcome {
    if status == ExecOutcome::Failed {
        return PullOutcome::Failed;
    }
    match after_id {
        Some(after) if before_id != Some(after) => PullOutcome::Updated,
        _ => PullOutcome::UpToDate,
    }
}

/// Evaluates whether `podman pod logs` should pass `--color`, keeping `--simulate` previews deterministic and treating empty `NO_COLOR` as unset.
#[must_use]
fn should_use_log_color(simulate: bool, is_tty: bool, no_color: Option<&OsStr>) -> bool {
    if simulate {
        return true;
    }
    let color_disabled = no_color.is_some_and(|value| !value.is_empty());
    is_tty && !color_disabled
}

/// Validates all fields of `request` before building `podman exec` arguments.
fn validate_exec_container<S: AsRef<str>>(request: &ExecContainer<'_, S>) -> Result<(), PodError> {
    super::ensure_valid(
        string::is_valid_unmanaged_container_name(request.container_name),
        "invalid container name",
    )?;
    super::ensure_valid(
        request.user.is_none_or(string::is_valid_token),
        "invalid exec user",
    )?;
    let Some(program) = request.command.first().map(AsRef::as_ref) else {
        return super::ensure_valid(false, "missing exec command");
    };
    super::ensure_valid(string::is_valid_token(program), "invalid exec program")?;
    super::ensure_valid(
        request
            .command
            .iter()
            .all(|argument| !string::contains_control_or_bidi(argument.as_ref())),
        "invalid exec argument",
    )
}

/// Validates all fields of `request` before building `podman pod logs` arguments.
fn validate_pod_logs(request: &PodLogs<'_>) -> Result<(), PodError> {
    super::ensure_valid(
        string::is_valid_pod_or_container_name(request.pod_name),
        "invalid pod name",
    )?;
    super::ensure_valid(
        request
            .container_name
            .is_none_or(string::is_valid_pod_or_container_name),
        "invalid container name",
    )?;
    super::ensure_valid(
        request.since.is_none_or(string::is_valid_log_time),
        "invalid since flag",
    )?;
    super::ensure_valid(
        request.until.is_none_or(string::is_valid_log_time),
        "invalid until flag",
    )?;
    super::ensure_valid(
        request.tail.is_none_or(string::is_valid_log_tail),
        "invalid tail flag",
    )
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
