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
            child.wait()?
        };
        let captured_stderr = stderr_handle.join().unwrap_or_default();
        Ok((status, captured_stderr))
    })
}

/// Polls `child` until completion while rendering Braille spinner frames after `sanitized_prefix` on `stdout`.
fn wait_with_spinner<W: Write>(
    child: &mut Child,
    sanitized_prefix: &str,
    poll_interval: Duration,
    stdout: &mut W,
) -> Result<ExitStatus, PodError> {
    let mut frames = SPINNER_FRAMES.iter().copied().cycle();
    loop {
        let frame = frames.next().unwrap_or("⠋");
        write_spinner_frame(stdout, sanitized_prefix, frame);
        let poll_result = child.try_wait();
        if let Some(status) = poll_child_status(child, poll_result, poll_interval)? {
            return Ok(status);
        }
    }
}

/// Writes a single carriage-returned spinner frame (`\r<prefix><frame>`) to `stdout` and flushes the stream.
fn write_spinner_frame<W: Write>(stdout: &mut W, sanitized_prefix: &str, frame: &str) {
    let _ = write!(stdout, "\r{sanitized_prefix}{frame}");
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
            "\r{sanitized_prefix}{sanitized_summary}{SPINNER_CLEAR_EOL}"
        );
    } else {
        let _ = writeln!(stdout, "{sanitized_prefix}{sanitized_summary}");
    }
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::process::Stdio;
    use std::time::Duration;

    use crate::core::log::{self, DebugGuard};
    use crate::error::PodError;
    use crate::io::exec::command::{self, ExecOutcome};
    use super::{
        SPINNER_CLEAR_EOL, SPINNER_FRAMES, SPINNER_MAX_PREFIX_BYTES, SPINNER_MAX_SUMMARY_BYTES,
        SpinnerConfig, exec_spinner_configured, exec_spinner_with, poll_child_status,
    };

    #[test]
    fn spinner_non_tty_emits_clean_line_without_escapes_and_handles_stderr_policies() {
        let mut sim_stdout = Vec::new();
        let mut sim_stderr = Vec::new();
        exec_spinner_with(
            "podman",
            &["pull", "docker.io/library/nginx:stable-alpine"],
            "  Pull 'proxy-nginx' (docker.io/library/nginx:stable-alpine): ",
            true,
            |_| panic!("resolve_summary must not run in simulate mode"),
            &mut sim_stdout,
            &mut sim_stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(sim_stdout).unwrap(),
            "  Pull 'proxy-nginx' (docker.io/library/nginx:stable-alpine): \n  podman pull docker.io/library/nginx:stable-alpine\n"
        );
        assert!(sim_stderr.is_empty());

        let non_tty_config = SpinnerConfig {
            is_tty: false,
            poll_interval: Duration::from_millis(2),
            prefix: "  Pull 'app'... ",
            simulate: false,
        };
        let mut ok_stdout = Vec::new();
        let mut ok_stderr = Vec::new();
        exec_spinner_configured(
            "dd",
            &["if=/dev/null", "of=/dev/null"],
            &non_tty_config,
            |outcome| {
                assert_eq!(outcome, ExecOutcome::Succeeded);
                "Updated."
            },
            &mut ok_stdout,
            &mut ok_stderr,
        )
        .unwrap();
        let ok_text = String::from_utf8(ok_stdout).unwrap();
        assert_eq!(ok_text, "  Pull 'app'... Updated.\n");
        assert!(!ok_text.contains('\r'));
        assert!(!ok_text.contains("\x1b[?25l"));
        assert!(!ok_text.contains("\x1b[?25h"));
        assert!(!ok_text.contains(SPINNER_CLEAR_EOL));
        assert!(
            ok_stderr.is_empty(),
            "stderr must stay silent on success without --debug"
        );

        let mut debug_stdout = Vec::new();
        let mut debug_stderr = Vec::new();
        {
            let _guard = DebugGuard::new(true);
            exec_spinner_configured(
                "dd",
                &["if=/dev/null", "of=/dev/null"],
                &non_tty_config,
                |_| "Up to date.",
                &mut debug_stdout,
                &mut debug_stderr,
            )
            .unwrap();
        }
        let debug_err_text = String::from_utf8(debug_stderr).unwrap();
        assert!(debug_err_text.contains("DEBUG: Execute: "));
        assert!(debug_err_text.contains("0+0 records in\n"));

        let mut fail_stdout = Vec::new();
        let mut fail_stderr = Vec::new();
        exec_spinner_configured(
            "ls",
            &["-d", "/nonexistent/podoxide_spinner_fail_target"],
            &SpinnerConfig {
                prefix: "Trying to pull\nbad\u{202E}... ",
                ..non_tty_config
            },
            |outcome| {
                assert_eq!(outcome, ExecOutcome::Failed);
                "Failed."
            },
            &mut fail_stdout,
            &mut fail_stderr,
        )
        .unwrap();
        let fail_out_text = String::from_utf8(fail_stdout).unwrap();
        assert_eq!(fail_out_text, "Trying to pull bad ... Failed.\n");
        assert!(!fail_out_text.contains('\u{202E}'));
        let fail_err_text = String::from_utf8(fail_stderr).unwrap();
        assert!(fail_err_text.contains("podoxide_spinner_fail_target"));
        assert!(fail_err_text.contains("WARNING: Command exited with code '"));
    }

    #[test]
    fn spinner_tty_animates_clears_eol_omits_cursor_escapes_and_enforces_bounds() {
        let mut tty_stdout = Vec::new();
        let mut tty_stderr = Vec::new();
        exec_spinner_configured(
            "sleep",
            &["0.02"],
            &SpinnerConfig {
                is_tty: true,
                poll_interval: Duration::from_millis(2),
                prefix: "  Pull 'proxy-nginx'... ",
                simulate: false,
            },
            |outcome| {
                assert_eq!(outcome, ExecOutcome::Succeeded);
                "Updated."
            },
            &mut tty_stdout,
            &mut tty_stderr,
        )
        .unwrap();
        let tty_text = String::from_utf8(tty_stdout).unwrap();
        assert!(tty_text.starts_with(&format!("\r  Pull 'proxy-nginx'... {}", SPINNER_FRAMES[0])));
        assert!(tty_text.contains(&format!("\r  Pull 'proxy-nginx'... {}", SPINNER_FRAMES[1])));
        assert!(tty_text.ends_with(&format!(
            "\r  Pull 'proxy-nginx'... Updated.{SPINNER_CLEAR_EOL}\n"
        )));
        assert!(!tty_text.contains("\x1b[?25l"));
        assert!(!tty_text.contains("\x1b[?25h"));
        assert!(tty_stderr.is_empty());

        let mut oversized_stdout = Vec::new();
        let mut oversized_stderr = Vec::new();
        let oversized_prefix = "p".repeat(log::MAX_LOG_MESSAGE_BYTES + 1);
        exec_spinner_configured(
            "false",
            &Vec::<String>::new(),
            &SpinnerConfig {
                is_tty: true,
                poll_interval: Duration::from_millis(2),
                prefix: &oversized_prefix,
                simulate: false,
            },
            |_| "0123456789abcdef_overflow",
            &mut oversized_stdout,
            &mut oversized_stderr,
        )
        .unwrap();
        let oversized_text = String::from_utf8(oversized_stdout).unwrap();
        let final_line = oversized_text
            .trim_end_matches('\n')
            .rsplit('\r')
            .next()
            .unwrap()
            .strip_suffix(SPINNER_CLEAR_EOL)
            .unwrap();
        assert_eq!(
            final_line.len(),
            SPINNER_MAX_PREFIX_BYTES + SPINNER_MAX_SUMMARY_BYTES
        );
        assert_eq!(final_line.len(), log::MAX_LOG_MESSAGE_BYTES);

        let missing_err = exec_spinner_with(
            "/nonexistent/podoxide_missing_bin",
            &Vec::<String>::new(),
            "",
            false,
            |_| "Failed.",
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(missing_err, PodError::Io(_)));

        let mut child = command::build_command("sleep", &["10"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let poll_error = poll_child_status(
            &mut child,
            Err(io::Error::other("poll failure")),
            Duration::from_millis(1),
        )
        .unwrap_err();
        assert!(matches!(
            poll_error,
            PodError::Io(ref io_err) if io_err.kind() == io::ErrorKind::Other
        ));
        assert!(child.try_wait().unwrap().is_some());
    }
}
