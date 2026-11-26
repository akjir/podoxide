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

//! Terminal-aware Braille spinner runner with bounded `stderr` capture.

use std::env;
use std::ffi::OsStr;
use std::io::{self, IsTerminal, Write};
use std::process::{Child, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

use crate::core::log;
use crate::error::PodError;
use crate::utilities::{format, string};
use super::command::{self, ExecOutcome, InterruptPolicy};

/// Maximum `stderr` bytes captured from a background spinner command (`16 KiB`).
const MAX_SPINNER_STDERR_BYTES: usize = 16 * 1024;
/// ANSI CSI sequence erasing from cursor to end of line on final TTY output.
const SPINNER_CLEAR_EOL: &str = "\x1b[K";
/// Braille spinner frames displayed while waiting for a background process on an interactive TTY.
const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Polling interval in milliseconds between TTY spinner frame updates.
const SPINNER_INTERVAL_MS: u64 = 80;
/// Maximum byte length of the sanitized spinner prefix so prefix plus summary fits within [`log::MAX_LOG_MESSAGE_BYTES`].
const SPINNER_MAX_PREFIX_BYTES: usize = log::MAX_LOG_MESSAGE_BYTES - SPINNER_MAX_SUMMARY_BYTES;
/// Maximum byte length of the sanitized spinner completion summary suffix.
const SPINNER_MAX_SUMMARY_BYTES: usize = 16;
/// VT100 DECRC sequence restoring the saved post-prefix cursor position on TTY frames.
const SPINNER_RESTORE_CURSOR: &str = "\x1b8";
/// VT100 DECSC sequence saving the post-prefix cursor position on TTY startup.
const SPINNER_SAVE_CURSOR: &str = "\x1b7";

/// Configuration controlling spinner simulation, terminal animation, prefixing, and polling cadence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SpinnerConfig<'a> {
    is_tty: bool,
    poll_interval: Duration,
    prefix: &'a str,
    simulate: bool,
}

/// Executes `program` with `arguments` while rendering a Braille spinner on TTY `stdout` (or a clean single line when redirected), resolving the final completion label via `resolve_summary`.
///
/// # Errors
///
/// Returns [`PodError`] if spawning or waiting on the child process fails.
pub(super) fn exec_spinner_with<S, W, E, F>(
    program: &str,
    arguments: &[S],
    prefix: &str,
    simulate: bool,
    resolve_summary: F,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    S: AsRef<str>,
    W: Write,
    E: Write,
    F: FnOnce(ExecOutcome) -> &'static str,
{
    let config = SpinnerConfig {
        is_tty: is_spinner_tty(),
        poll_interval: Duration::from_millis(SPINNER_INTERVAL_MS),
        prefix,
        simulate,
    };
    exec_spinner_configured(program, arguments, &config, resolve_summary, stdout, stderr)
}

/// Emits bounded captured child `stderr` lines when the process failed or debug logging is enabled.
fn emit_spinner_stderr<E: Write>(captured_stderr: &[u8], status: ExitStatus, stderr: &mut E) {
    if (status.success() && !log::is_debug_enabled()) || captured_stderr.is_empty() {
        return;
    }
    let stderr_text = String::from_utf8_lossy(captured_stderr);
    for line in stderr_text.lines() {
        log::write_print(stderr, line);
    }
}

/// Executes `program` with explicit `config` controls for deterministic testing.
fn exec_spinner_configured<S, W, E, F>(
    program: &str,
    arguments: &[S],
    config: &SpinnerConfig<'_>,
    resolve_summary: F,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    S: AsRef<str>,
    W: Write,
    E: Write,
    F: FnOnce(ExecOutcome) -> &'static str,
{
    if config.simulate {
        command::write_simulated_command(program, arguments, config.prefix, stdout);
        return Ok(());
    }
    if log::is_debug_enabled() {
        let formatted = format::format_command(program, arguments);
        log::write_debug(stderr, &format!("Execute: {formatted}"));
    }
    let mut child = command::build_command(program, arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    let sanitized_prefix = string::sanitize_text(config.prefix, SPINNER_MAX_PREFIX_BYTES);
    let (exit_status, captured_stderr) = run_spinner_child(
        &mut child,
        &sanitized_prefix,
        config.is_tty,
        config.poll_interval,
        stdout,
    )?;
    let outcome = ExecOutcome::from_status(exit_status);
    let summary = resolve_summary(outcome);
    write_spinner_result(stdout, &sanitized_prefix, summary, config.is_tty);
    emit_spinner_stderr(&captured_stderr, exit_status, stderr);
    command::warn_on_exit_failure(exit_status, InterruptPolicy::WarnOnInterrupt, stderr);
    Ok(())
}

/// Evaluates whether standard output is an interactive terminal capable of spinner animation.
#[must_use]
fn is_spinner_tty() -> bool {
    io::stdout().is_terminal() && env::var_os("TERM").as_deref() != Some(OsStr::new("dumb"))
}

/// Evaluates a `try_wait` `poll_result` for `child`, sleeping for `interval` when still running or killing and reaping `child` on error.
fn poll_child_status(
    child: &mut Child,
    poll_result: io::Result<Option<ExitStatus>>,
    interval: Duration,
) -> Result<Option<ExitStatus>, PodError> {
    match poll_result {
        Ok(Some(status)) => Ok(Some(status)),
        Ok(None) => {
            thread::sleep(interval);
            Ok(None)
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(PodError::from(error))
        }
    }
}

/// Drains bounded `stderr` in a scoped thread while waiting on `child` (animating spinner frames only when `is_tty` is `true`).
fn run_spinner_child<W: Write>(
    child: &mut Child,
    sanitized_prefix: &str,
    is_tty: bool,
    poll_interval: Duration,
    stdout: &mut W,
) -> Result<(ExitStatus, Vec<u8>), PodError> {
    let stderr_pipe = child.stderr.take();
    thread::scope(|scope| {
        let stderr_handle = scope.spawn(move || {
            let Some(pipe) = stderr_pipe else {
                return Vec::new();
            };
            command::read_bounded_bytes(pipe, MAX_SPINNER_STDERR_BYTES)
                .map_or_else(|_| Vec::new(), |(bytes, _)| bytes)
        });
        let status = if is_tty {
            wait_with_spinner(child, sanitized_prefix, poll_interval, stdout)?
        } else {
            let wait_result = child.wait();
            wait_child_status(child, wait_result)?
        };
        let captured_stderr = stderr_handle.join().unwrap_or_default();
        Ok((status, captured_stderr))
    })
}

/// Evaluates a blocking `wait` `wait_result` for `child`, killing and reaping `child` on error so scoped pipe readers reach EOF.
fn wait_child_status(
    child: &mut Child,
    wait_result: io::Result<ExitStatus>,
) -> Result<ExitStatus, PodError> {
    match wait_result {
        Ok(status) => Ok(status),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(PodError::from(error))
        }
    }
}

/// Polls `child` until completion while rendering Braille spinner frames after `sanitized_prefix` on `stdout`.
fn wait_with_spinner<W: Write>(
    child: &mut Child,
    sanitized_prefix: &str,
    poll_interval: Duration,
    stdout: &mut W,
) -> Result<ExitStatus, PodError> {
    let _ = write!(stdout, "{sanitized_prefix}{SPINNER_SAVE_CURSOR}");
    let _ = stdout.flush();
    let mut frames = SPINNER_FRAMES.iter().copied().cycle();
    loop {
        let frame = frames.next().unwrap_or("⠋");
        write_spinner_frame(stdout, frame);
        let poll_result = child.try_wait();
        match poll_child_status(child, poll_result, poll_interval) {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                let _ = writeln!(stdout);
                let _ = stdout.flush();
                return Err(error);
            }
        }
    }
}

/// Writes a single restored-cursor spinner frame (`\x1b8<frame>`) to `stdout` and flushes the stream.
fn write_spinner_frame<W: Write>(stdout: &mut W, frame: &str) {
    let _ = write!(stdout, "{SPINNER_RESTORE_CURSOR}{frame}");
    let _ = stdout.flush();
}

/// Writes the final sanitized spinner completion line to `stdout` and flushes the stream.
fn write_spinner_result<W: Write>(
    stdout: &mut W,
    sanitized_prefix: &str,
    summary: &str,
    is_tty: bool,
) {
    let sanitized_summary = string::sanitize_text(summary, SPINNER_MAX_SUMMARY_BYTES);
    if is_tty {
        let _ = writeln!(
            stdout,
            "{SPINNER_RESTORE_CURSOR}{sanitized_summary}{SPINNER_CLEAR_EOL}"
        );
    } else {
        let _ = writeln!(stdout, "{sanitized_prefix}{sanitized_summary}");
    }
    let _ = stdout.flush();
}

#[cfg(test)]
#[path = "spinner_tests.rs"]
mod tests;
