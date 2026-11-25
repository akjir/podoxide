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

//! Direct operating system process execution helpers.

use std::borrow::Cow;
use std::io::{self, Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;

use crate::core::log;
use crate::error::PodError;
use crate::utilities::{format, path, string};

/// Initial allocation size for bounded process stream capture buffers.
const INITIAL_CAPTURE_CAPACITY: usize = 4096;
/// Maximum bytes captured from a single child process stream (`4 MiB`).
const MAX_CAPTURED_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
/// POSIX signal number for interactive terminal interrupt (`SIGINT`).
const SIGINT: i32 = 2;

/// Exit outcome of an external process execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecOutcome {
    Failed,
    Succeeded,
}

impl ExecOutcome {
    /// Converts a process [`ExitStatus`] into [`ExecOutcome::Succeeded`] or [`ExecOutcome::Failed`].
    #[inline]
    #[must_use]
    pub(super) fn from_status(status: ExitStatus) -> Self {
        if status.success() {
            Self::Succeeded
        } else {
            Self::Failed
        }
    }
}

/// Output and standard-stream handling mode for external process execution.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ExecMode {
    #[default]
    Captured,
    Interactive,
}

/// Execution options controlling output mode, interrupt warning policy, prefixing, and simulation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ExecOptions<'a> {
    pub(super) interrupt_policy: InterruptPolicy,
    pub(super) mode: ExecMode,
    pub(super) prefix: &'a str,
    pub(super) simulate: bool,
}

/// Controls whether a `SIGINT` signal termination emits a warning on `stderr`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum InterruptPolicy {
    QuietOnSigint,
    #[default]
    WarnOnInterrupt,
}

/// Bounded `stdout`, `stderr`, and exit status collected from a child process.
struct CapturedOutput {
    status: ExitStatus,
    stderr: Vec<u8>,
    stdout: Vec<u8>,
}

/// Executes `editor` with `file_path` interactively (or prints the command when `simulate` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if `editor` or `file_path` is invalid or if spawning the editor process fails.
pub fn exec_editor_with<W, E>(
    editor: &str,
    file_path: &Path,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    W: Write,
    E: Write,
{
    if !path::is_valid_editor_value(editor) {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid editor executable",
        )));
    }
    if !path::is_valid_target_path(file_path) {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid editor target path",
        )));
    }
    let Some(raw_path) = file_path.to_str() else {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid editor target path",
        )));
    };
    let safe_path = normalize_editor_target(raw_path);
    let _ = exec_with(
        editor,
        &[safe_path.as_ref()],
        &ExecOptions {
            interrupt_policy: InterruptPolicy::WarnOnInterrupt,
            mode: ExecMode::Interactive,
            prefix: "",
            simulate,
        },
        stdout,
        stderr,
    )?;
    Ok(())
}

/// Constructs a [`Command`] for `program` populated with `arguments`.
pub(super) fn build_command<S: AsRef<str>>(program: &str, arguments: &[S]) -> Command {
    let mut command = Command::new(program);
    for argument in arguments {
        command.arg(argument.as_ref());
    }
    command
}

/// Executes `program` with `arguments` directly and returns bounded UTF-8 `stdout` when exit status is `0`.
#[must_use]
pub(super) fn exec_capture(program: &str, arguments: &[&str]) -> Option<String> {
    exec_capture_bounded(program, arguments, MAX_CAPTURED_OUTPUT_BYTES)
}

/// Executes `program` with `arguments` using `options`, routing formatted output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if spawning or waiting on the process fails.
pub(super) fn exec_with<S, W, E>(
    program: &str,
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
    if options.simulate {
        let formatted = format::format_command(program, arguments);
        if !options.prefix.is_empty() {
            log::write_print(stdout, options.prefix);
        }
        log::write_print(stdout, &formatted);
        return Ok(ExecOutcome::Succeeded);
    }
    if log::is_debug_enabled() {
        let formatted = format::format_command(program, arguments);
        log::write_debug(stderr, &format!("Execute: {formatted}"));
    }
    match options.mode {
        ExecMode::Captured => run_captured(
            program,
            arguments,
            options.prefix,
            MAX_CAPTURED_OUTPUT_BYTES,
            stdout,
            stderr,
        ),
        ExecMode::Interactive => {
            run_interactive(program, arguments, options.interrupt_policy, stderr)
        }
    }
}

/// Reads at most `max_bytes` from `reader`, returning collected bytes and whether unread bytes remained.
pub(super) fn read_bounded_bytes<R: Read>(
    reader: R,
    max_bytes: usize,
) -> io::Result<(Vec<u8>, bool)> {
    let initial_capacity = max_bytes.min(INITIAL_CAPTURE_CAPACITY);
    let mut buffer = Vec::with_capacity(initial_capacity);
    let limit = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    reader.take(limit).read_to_end(&mut buffer)?;
    let truncated = buffer.len() > max_bytes;
    if truncated {
        buffer.truncate(max_bytes);
    }
    Ok((buffer, truncated))
}

/// Writes a warning to `stderr` when `status` exited with a non-zero code or was terminated by a signal,
/// suppressing `SIGINT` warnings when `interrupt_policy` is [`InterruptPolicy::QuietOnSigint`].
pub(super) fn warn_on_exit_failure<E: Write>(
    status: ExitStatus,
    interrupt_policy: InterruptPolicy,
    stderr: &mut E,
) {
    if status.success() {
        return;
    }
    if let Some(code) = status.code() {
        log::write_warning(stderr, &format!("Command exited with code '{code}'!"));
        return;
    }
    if interrupt_policy == InterruptPolicy::QuietOnSigint && status.signal() == Some(SIGINT) {
        return;
    }
    log::write_warning(stderr, "Command terminated by signal!");
}

/// Writes `prefix` (when non-empty) and the shell-escaped simulated command line, preserving leading space indentation from `prefix`.
pub(super) fn write_simulated_command<S: AsRef<str>, W: Write>(
    program: &str,
    arguments: &[S],
    prefix: &str,
    stdout: &mut W,
) {
    let formatted = format::format_command(program, arguments);
    if !prefix.is_empty() {
        log::write_print(stdout, prefix);
    }
    let indent_len = prefix
        .len()
        .saturating_sub(prefix.trim_start_matches(' ').len());
    let indent = &prefix[..indent_len];
    if indent.is_empty() {
        log::write_print(stdout, &formatted);
    } else {
        log::write_print(stdout, &format!("{indent}{formatted}"));
    }
}

/// Drains bounded `stdout` and `stderr` concurrently from `child` without pipe deadlocks and reaps `child`.
fn capture_child_streams(child: &mut Child, max_bytes: usize) -> io::Result<CapturedOutput> {
    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();
    let (stdout_result, stderr_result) = thread::scope(|scope| {
        let stderr_handle = scope.spawn(move || read_optional_pipe(stderr_pipe, max_bytes));
        let stdout_result = read_optional_pipe(stdout_pipe, max_bytes);
        let stderr_result = stderr_handle
            .join()
            .unwrap_or_else(|_| Err(io::Error::other("stderr reader thread panicked")));
        (stdout_result, stderr_result)
    });
    let stdout = match stdout_result {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let stderr = match stderr_result {
        Ok(bytes) => bytes,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    let status = child.wait()?;
    Ok(CapturedOutput {
        status,
        stderr,
        stdout,
    })
}

/// Writes each line of `text` to `writer`, prepending `clamped_prefix` to the first emitted line.
fn emit_captured_lines<W: Write>(
    text: &str,
    clamped_prefix: &str,
    writer: &mut W,
    output_captured: &mut bool,
) {
    for line in text.lines() {
        if !*output_captured {
            let clamped_line = string::clamp_utf8_prefix(line, log::MAX_LOG_MESSAGE_BYTES);
            log::write_print(writer, &format!("{clamped_prefix}{clamped_line}"));
            *output_captured = true;
        } else {
            log::write_print(writer, line);
        }
    }
}

/// Executes `program` with `arguments` and returns bounded UTF-8 `stdout` when exit status is `0` and output fits within `max_bytes`.
#[must_use]
fn exec_capture_bounded(program: &str, arguments: &[&str], max_bytes: usize) -> Option<String> {
    let mut child = build_command(program, arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let Some(stdout_pipe) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let Ok((stdout_bytes, truncated)) = read_bounded_bytes(stdout_pipe, max_bytes) else {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    };
    let status = child.wait().ok()?;
    if !status.success() || truncated {
        return None;
    }
    String::from_utf8(stdout_bytes).ok()
}

/// Prefixes bare relative editor file paths with `./` so editors never parse them as options.
#[must_use]
fn normalize_editor_target(raw_path: &str) -> Cow<'_, str> {
    if raw_path.starts_with('/') || raw_path.starts_with("./") {
        return Cow::Borrowed(raw_path);
    }
    Cow::Owned(format!("./{raw_path}"))
}

/// Reads up to `max_bytes` from an optional child stream pipe, returning an empty buffer when `None`.
fn read_optional_pipe<R: Read>(pipe: Option<R>, max_bytes: usize) -> io::Result<Vec<u8>> {
    let Some(reader) = pipe else {
        return Ok(Vec::new());
    };
    let (bytes, _) = read_bounded_bytes(reader, max_bytes)?;
    Ok(bytes)
}

/// Executes `program` with `arguments`, writing bounded `stdout` to `stdout`, bounded `stderr` to `stderr`, and logging exit warnings to `stderr`.
fn run_captured<S, W, E>(
    program: &str,
    arguments: &[S],
    prefix: &str,
    max_bytes: usize,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError>
where
    S: AsRef<str>,
    W: Write,
    E: Write,
{
    let mut child = build_command(program, arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let captured = capture_child_streams(&mut child, max_bytes)?;
    let stdout_text = String::from_utf8_lossy(&captured.stdout);
    let stderr_text = String::from_utf8_lossy(&captured.stderr);
    let clamped_prefix = string::clamp_utf8_prefix(prefix, log::MAX_LOG_MESSAGE_BYTES);
    let mut output_captured = false;
    emit_captured_lines(&stdout_text, clamped_prefix, stdout, &mut output_captured);
    emit_captured_lines(&stderr_text, clamped_prefix, stderr, &mut output_captured);
    if !output_captured {
        log::write_print(stdout, &format!("{clamped_prefix}..."));
    }
    warn_on_exit_failure(captured.status, InterruptPolicy::WarnOnInterrupt, stderr);
    Ok(ExecOutcome::from_status(captured.status))
}

/// Executes `program` with `arguments` interactively, inheriting standard streams and logging exit warnings to `stderr`.
fn run_interactive<S, E>(
    program: &str,
    arguments: &[S],
    interrupt_policy: InterruptPolicy,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError>
where
    S: AsRef<str>,
    E: Write,
{
    let status = build_command(program, arguments).status()?;
    warn_on_exit_failure(status, interrupt_policy, stderr);
    Ok(ExecOutcome::from_status(status))
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
