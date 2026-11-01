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

use std::io::{self, Write};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

use crate::core::log;
use crate::error::PodError;
use crate::utilities::{format, path, string};

/// Completion suffix printed when a spinner command exits with status `0`.
const SPINNER_DONE_SUMMARY: &str = "Done.";
/// Completion suffix printed when a spinner command exits with a non-zero status.
const SPINNER_FAILED_SUMMARY: &str = "Failed.";
/// Braille spinner frames displayed while waiting for a background process to complete.
const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// ANSI DECTCEM escape sequence that hides the terminal cursor while the spinner is active.
const SPINNER_HIDE_CURSOR: &str = "\x1b[?25l";
/// Polling interval in milliseconds between spinner frame updates.
const SPINNER_INTERVAL_MS: u64 = 80;
/// Maximum byte length of the sanitized spinner prefix so prefix plus summary fits within [`log::MAX_LOG_MESSAGE_BYTES`].
const SPINNER_MAX_PREFIX_BYTES: usize = log::MAX_LOG_MESSAGE_BYTES - SPINNER_FAILED_SUMMARY.len();
/// ANSI DECTCEM escape sequence that restores terminal cursor visibility when the spinner finishes.
const SPINNER_SHOW_CURSOR: &str = "\x1b[?25h";

/// Output and standard-stream handling mode for external process execution.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum ExecMode {
    #[default]
    Captured,
    Interactive,
    Spinner,
}

/// Execution options controlling output mode, output prefixing, and command simulation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ExecOptions<'a> {
    pub(super) mode: ExecMode,
    pub(super) prefix: &'a str,
    pub(super) simulate: bool,
}

/// RAII guard that hides the terminal cursor on creation and restores it on drop.
struct SpinnerCursor<'a, W: Write> {
    stdout: &'a mut W,
}

impl<'a, W: Write> SpinnerCursor<'a, W> {
    /// Writes the ANSI hide-cursor escape sequence to `stdout` and returns a guard that restores cursor visibility on drop.
    #[must_use]
    fn new(stdout: &'a mut W) -> Self {
        let _ = stdout.write_all(SPINNER_HIDE_CURSOR.as_bytes());
        let _ = stdout.flush();
        Self { stdout }
    }
}

impl<W: Write> Drop for SpinnerCursor<'_, W> {
    /// Writes the ANSI show-cursor escape sequence to `stdout` and flushes the stream.
    fn drop(&mut self) {
        let _ = self.stdout.write_all(SPINNER_SHOW_CURSOR.as_bytes());
        let _ = self.stdout.flush();
    }
}

/// Executes `editor` with `file_path` interactively (or prints the command when `simulate` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if `editor` or `file_path` is invalid or if spawning the editor process fails.
pub fn exec_editor_with<W, E>(
    editor: &str,
    file_path: &str,
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
    if !path::is_valid_target_path(Path::new(file_path)) {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid editor target path",
        )));
    }
    exec_with(
        editor,
        &[file_path],
        &ExecOptions {
            mode: ExecMode::Interactive,
            prefix: "",
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Executes `program` with `arguments` directly and returns captured `stdout` when exit status is 0.
#[must_use]
pub(in crate::io) fn exec_capture(program: &str, arguments: &[&str]) -> Option<String> {
    let output = build_command(program, arguments).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

/// Executes `program` with `arguments` using `options`, routing formatted output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if spawning the process fails.
pub(super) fn exec_with<S, W, E>(
    program: &str,
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
    if options.simulate {
        let formatted = format::format_command(program, arguments);
        if !options.prefix.is_empty() {
            log::write_print(stdout, options.prefix);
        }
        log::write_print(stdout, &formatted);
        return Ok(());
    }
    if log::is_debug_enabled() {
        let formatted = format::format_command(program, arguments);
        log::write_debug(stderr, &format!("Execute: {formatted}"));
    }
    match options.mode {
        ExecMode::Captured => run_captured(program, arguments, options.prefix, stdout, stderr),
        ExecMode::Interactive => run_interactive(program, arguments, stderr),
        ExecMode::Spinner => run_spinner(program, arguments, options.prefix, stdout, stderr),
    }
}

/// Constructs a [`Command`] for `program` populated with `arguments`.
fn build_command<S: AsRef<str>>(program: &str, arguments: &[S]) -> Command {
    let mut command = Command::new(program);
    for argument in arguments {
        command.arg(argument.as_ref());
    }
    command
}

/// Writes each line of `text` to `stdout`, prepending `clamped_prefix` to the first emitted line.
fn emit_captured_lines<W: Write>(
    text: &str,
    clamped_prefix: &str,
    stdout: &mut W,
    output_captured: &mut bool,
) {
    for line in text.lines() {
        if !*output_captured {
            let clamped_line = string::clamp_utf8_prefix(line, log::MAX_LOG_MESSAGE_BYTES);
            log::write_print(stdout, &format!("{clamped_prefix}{clamped_line}"));
            *output_captured = true;
        } else {
            log::write_print(stdout, line);
        }
    }
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

/// Executes `program` with `arguments`, writing captured output lines with `prefix` to `stdout` and logging exit warnings to `stderr`.
fn run_captured<S, W, E>(
    program: &str,
    arguments: &[S],
    prefix: &str,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    S: AsRef<str>,
    W: Write,
    E: Write,
{
    let output = build_command(program, arguments).output()?;
    let stdout_text = String::from_utf8_lossy(&output.stdout);
    let stderr_text = String::from_utf8_lossy(&output.stderr);
    let clamped_prefix = string::clamp_utf8_prefix(prefix, log::MAX_LOG_MESSAGE_BYTES);
    let mut output_captured = false;
    emit_captured_lines(&stdout_text, clamped_prefix, stdout, &mut output_captured);
    emit_captured_lines(&stderr_text, clamped_prefix, stdout, &mut output_captured);
    if !output_captured {
        log::write_print(stdout, &format!("{clamped_prefix}..."));
    }
    warn_on_exit_failure(output.status, stderr);
    Ok(())
}

/// Executes `program` with `arguments` interactively, inheriting standard streams and logging exit warnings to `stderr`.
fn run_interactive<S, E>(program: &str, arguments: &[S], stderr: &mut E) -> Result<(), PodError>
where
    S: AsRef<str>,
    E: Write,
{
    let status = build_command(program, arguments).status()?;
    warn_on_exit_failure(status, stderr);
    Ok(())
}

/// Executes `program` with `arguments` while animating a Braille spinner prefixed by `prefix` on `stdout`.
fn run_spinner<S, W, E>(
    program: &str,
    arguments: &[S],
    prefix: &str,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    S: AsRef<str>,
    W: Write,
    E: Write,
{
    let mut child = build_command(program, arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let sanitized_prefix = string::sanitize_text(prefix, SPINNER_MAX_PREFIX_BYTES);
    let status = wait_with_spinner(&mut child, &sanitized_prefix, stdout)?;
    write_spinner_result(stdout, &sanitized_prefix, status);
    warn_on_exit_failure(status, stderr);
    Ok(())
}

/// Polls `child` until completion while hiding the terminal cursor and rendering spinner frames after `sanitized_prefix` on `stdout`.
fn wait_with_spinner<W: Write>(
    child: &mut Child,
    sanitized_prefix: &str,
    stdout: &mut W,
) -> Result<ExitStatus, PodError> {
    let cursor = SpinnerCursor::new(stdout);
    let interval = Duration::from_millis(SPINNER_INTERVAL_MS);
    let mut frames = SPINNER_FRAMES.iter().copied().cycle();
    loop {
        let frame = frames.next().unwrap_or("⠋");
        write_spinner_frame(cursor.stdout, sanitized_prefix, frame);
        let poll_result = child.try_wait();
        if let Some(status) = poll_child_status(child, poll_result, interval)? {
            return Ok(status);
        }
    }
}

/// Writes a warning to `stderr` when `status` exited with a non-zero code or was terminated by a signal.
fn warn_on_exit_failure<E: Write>(status: ExitStatus, stderr: &mut E) {
    if status.success() {
        return;
    }
    if let Some(code) = status.code() {
        log::write_warning(stderr, &format!("Command exited with code '{code}'!"));
    } else {
        log::write_warning(stderr, "Command terminated by signal!");
    }
}

/// Writes a single carriage-returned spinner frame (`\r<prefix><frame>`) to `stdout` and flushes the stream.
fn write_spinner_frame<W: Write>(stdout: &mut W, sanitized_prefix: &str, frame: &str) {
    let _ = write!(stdout, "\r{sanitized_prefix}{frame}");
    let _ = stdout.flush();
}

/// Writes the final spinner completion line (`Done.` or `Failed.`) to `stdout` and flushes the stream.
fn write_spinner_result<W: Write>(stdout: &mut W, sanitized_prefix: &str, status: ExitStatus) {
    let summary = if status.success() {
        SPINNER_DONE_SUMMARY
    } else {
        SPINNER_FAILED_SUMMARY
    };
    let _ = writeln!(stdout, "\r{sanitized_prefix}{summary}");
    let _ = stdout.flush();
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Stdio};
    use std::time::Duration;

    use crate::core::log;
    use crate::error::PodError;
    use crate::utilities::format;
    use super::{
        ExecMode, ExecOptions, SPINNER_FAILED_SUMMARY, SPINNER_FRAMES, SPINNER_HIDE_CURSOR,
        SPINNER_MAX_PREFIX_BYTES, SPINNER_SHOW_CURSOR, build_command, exec_capture,
        exec_editor_with, exec_with, poll_child_status, warn_on_exit_failure,
    };

    #[test]
    fn exec_capture_handles_success_failure_and_missing_binary() {
        let output = exec_capture("echo", &["podoxide-test"]).unwrap();
        assert_eq!(output.trim(), "podoxide-test");

        assert!(exec_capture("false", &[]).is_none());
        assert!(exec_capture("/nonexistent/podoxide_missing_bin", &[]).is_none());
        assert!(exec_capture("printf", &["\\377\\376"]).is_none());
    }

    #[test]
    fn exec_editor_with_validates_editor_and_target_path() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        exec_editor_with("vim", "./pods.conf", true, &mut stdout, &mut stderr).unwrap();
        assert_eq!(String::from_utf8(stdout).unwrap(), "vim ./pods.conf\n");
        assert!(stderr.is_empty());

        let mut live_stdout = Vec::new();
        let mut live_stderr = Vec::new();
        exec_editor_with(
            "true",
            "./recipe.pod",
            false,
            &mut live_stdout,
            &mut live_stderr,
        )
        .unwrap();
        assert!(live_stdout.is_empty());
        assert!(live_stderr.is_empty());

        for invalid_editor in [
            "",
            "-vim",
            "/usr/bin/-vim",
            "../vim",
            "vim\n",
            "vim -u NONE",
            "podman",
            "/usr/bin/podman",
        ] {
            let error = exec_editor_with(
                invalid_editor,
                "./pods.conf",
                true,
                &mut Vec::new(),
                &mut Vec::new(),
            )
            .unwrap_err();
            assert!(
                matches!(error, PodError::Io(ref io_err) if io_err.kind() == io::ErrorKind::InvalidInput),
                "expected InvalidInput for editor {invalid_editor:?}"
            );
        }

        for invalid_path in [
            "",
            "-c",
            "--cmd",
            "../etc/passwd",
            ".",
            "..",
            "/",
            ".pod",
            "./.conf",
            "pods\n.conf",
        ] {
            let error = exec_editor_with(
                "vim",
                invalid_path,
                true,
                &mut Vec::new(),
                &mut Vec::new(),
            )
            .unwrap_err();
            assert!(
                matches!(error, PodError::Io(ref io_err) if io_err.kind() == io::ErrorKind::InvalidInput),
                "expected InvalidInput for file_path {invalid_path:?}"
            );
        }
    }

    #[test]
    fn exec_simulate_captured_interactive_spinner_and_error_handling() {
        let default_options = ExecOptions::default();
        assert_eq!(default_options.mode, ExecMode::Captured);
        assert_eq!(default_options.prefix, "");
        assert!(!default_options.simulate);

        assert_eq!(
            format::format_command("podman", &["run", "--name", "app", "O'Hara", "Hancock 2"]),
            "podman run --name app 'O'\\''Hara' 'Hancock 2'"
        );

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let simulate_options = ExecOptions {
            mode: ExecMode::Captured,
            prefix: "Create container 'app': ",
            simulate: true,
        };
        exec_with(
            "podman",
            &["create", "--name", "app"],
            &simulate_options,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        let simulate_output = String::from_utf8(stdout).unwrap();
        assert_eq!(
            simulate_output,
            "Create container 'app': \npodman create --name app\n"
        );
        assert!(stderr.is_empty());

        let mut no_prefix_stdout = Vec::new();
        let simulate_without_prefix = ExecOptions {
            mode: ExecMode::Interactive,
            prefix: "",
            simulate: true,
        };
        exec_with(
            "podman",
            &["pull", "docker.io/alpine:latest"],
            &simulate_without_prefix,
            &mut no_prefix_stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(no_prefix_stdout).unwrap(),
            "podman pull docker.io/alpine:latest\n"
        );

        let mut captured_stdout = Vec::new();
        let mut captured_stderr = Vec::new();
        let capture_options = ExecOptions {
            mode: ExecMode::Captured,
            prefix: "Echo: ",
            simulate: false,
        };
        exec_with(
            "printf",
            &["line1\\nline2\\n"],
            &capture_options,
            &mut captured_stdout,
            &mut captured_stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(captured_stdout).unwrap(),
            "Echo: line1\nline2\n"
        );
        assert!(captured_stderr.is_empty());

        let mut stderr_capture_stdout = Vec::new();
        let mut stderr_capture_stderr = Vec::new();
        exec_with(
            "ls",
            &["-d", "/", "/nonexistent/podoxide_missing_path"],
            &ExecOptions {
                mode: ExecMode::Captured,
                prefix: "List: ",
                simulate: false,
            },
            &mut stderr_capture_stdout,
            &mut stderr_capture_stderr,
        )
        .unwrap();
        let stderr_capture_text = String::from_utf8(stderr_capture_stdout).unwrap();
        assert!(stderr_capture_text.starts_with("List: /\n"));
        assert!(stderr_capture_text.lines().count() >= 2);
        let stderr_warning_text = String::from_utf8(stderr_capture_stderr).unwrap();
        assert!(stderr_warning_text.starts_with("WARNING: Command exited with code '"));

        let mut stderr_only_stdout = Vec::new();
        let mut stderr_only_stderr = Vec::new();
        exec_with(
            "ls",
            &["-d", "/nonexistent/podoxide_exec_missing_path"],
            &ExecOptions {
                mode: ExecMode::Captured,
                prefix: "Missing: ",
                simulate: false,
            },
            &mut stderr_only_stdout,
            &mut stderr_only_stderr,
        )
        .unwrap();
        let stderr_only_text = String::from_utf8(stderr_only_stdout).unwrap();
        assert!(stderr_only_text.starts_with("Missing: "));
        assert!(!stderr_only_text.starts_with("Missing: ..."));
        let stderr_only_warning = String::from_utf8(stderr_only_stderr).unwrap();
        assert!(stderr_only_warning.starts_with("WARNING: Command exited with code '"));

        let mut empty_stdout = Vec::new();
        let mut empty_stderr = Vec::new();
        exec_with(
            "true",
            &Vec::<String>::new(),
            &ExecOptions {
                mode: ExecMode::Captured,
                prefix: "Quiet: ",
                simulate: false,
            },
            &mut empty_stdout,
            &mut empty_stderr,
        )
        .unwrap();
        assert_eq!(String::from_utf8(empty_stdout).unwrap(), "Quiet: ...\n");
        assert!(empty_stderr.is_empty());

        let mut debug_stdout = Vec::new();
        let mut debug_stderr = Vec::new();
        log::set_debug_enabled(true);
        let debug_result = exec_with(
            "true",
            &["--version"],
            &ExecOptions {
                mode: ExecMode::Captured,
                prefix: "Debug: ",
                simulate: false,
            },
            &mut debug_stdout,
            &mut debug_stderr,
        );
        log::set_debug_enabled(false);
        debug_result.unwrap();
        assert_eq!(
            String::from_utf8(debug_stderr).unwrap(),
            "DEBUG: Execute: true --version\n"
        );

        let mut fail_stderr = Vec::new();
        exec_with(
            "false",
            &Vec::<String>::new(),
            &ExecOptions::default(),
            &mut Vec::new(),
            &mut fail_stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(fail_stderr).unwrap(),
            "WARNING: Command exited with code '1'!\n"
        );

        let mut interactive_fail_stderr = Vec::new();
        exec_with(
            "false",
            &Vec::<String>::new(),
            &ExecOptions {
                mode: ExecMode::Interactive,
                prefix: "",
                simulate: false,
            },
            &mut Vec::new(),
            &mut interactive_fail_stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(interactive_fail_stderr).unwrap(),
            "WARNING: Command exited with code '1'!\n"
        );

        let mut spinner_stdout = Vec::new();
        let mut spinner_stderr = Vec::new();
        exec_with(
            "sleep",
            &["0.1"],
            &ExecOptions {
                mode: ExecMode::Spinner,
                prefix: "Trying to pull docker.io/library/nginx:stable-alpine... ",
                simulate: false,
            },
            &mut spinner_stdout,
            &mut spinner_stderr,
        )
        .unwrap();
        let spinner_text = String::from_utf8(spinner_stdout).unwrap();
        assert!(spinner_text.starts_with(
            &format!(
                "{SPINNER_HIDE_CURSOR}\rTrying to pull docker.io/library/nginx:stable-alpine... {}",
                SPINNER_FRAMES[0]
            )
        ));
        assert!(spinner_text.contains(
            &format!("\rTrying to pull docker.io/library/nginx:stable-alpine... {}", SPINNER_FRAMES[1])
        ));
        assert!(spinner_text.ends_with(
            &format!(
                "{SPINNER_SHOW_CURSOR}\rTrying to pull docker.io/library/nginx:stable-alpine... Done.\n"
            )
        ));
        assert!(spinner_stderr.is_empty());

        let mut spinner_fail_stdout = Vec::new();
        let mut spinner_fail_stderr = Vec::new();
        exec_with(
            "false",
            &Vec::<String>::new(),
            &ExecOptions {
                mode: ExecMode::Spinner,
                prefix: "Trying to pull\nbad\u{202E}... ",
                simulate: false,
            },
            &mut spinner_fail_stdout,
            &mut spinner_fail_stderr,
        )
        .unwrap();
        let spinner_fail_text = String::from_utf8(spinner_fail_stdout).unwrap();
        assert!(spinner_fail_text.starts_with(
            &format!("{SPINNER_HIDE_CURSOR}\rTrying to pull bad ... {}", SPINNER_FRAMES[0])
        ));
        assert!(
            spinner_fail_text
                .ends_with(&format!("{SPINNER_SHOW_CURSOR}\rTrying to pull bad ... Failed.\n"))
        );
        assert!(!spinner_fail_text.contains('\u{202E}'));
        assert_eq!(
            String::from_utf8(spinner_fail_stderr).unwrap(),
            "WARNING: Command exited with code '1'!\n"
        );

        let mut oversized_spinner_stdout = Vec::new();
        let mut oversized_spinner_stderr = Vec::new();
        let oversized_prefix = "p".repeat(log::MAX_LOG_MESSAGE_BYTES + 1);
        exec_with(
            "false",
            &Vec::<String>::new(),
            &ExecOptions {
                mode: ExecMode::Spinner,
                prefix: &oversized_prefix,
                simulate: false,
            },
            &mut oversized_spinner_stdout,
            &mut oversized_spinner_stderr,
        )
        .unwrap();
        let oversized_spinner_text = String::from_utf8(oversized_spinner_stdout).unwrap();
        let final_spinner_line = oversized_spinner_text
            .trim_end_matches('\n')
            .rsplit('\r')
            .next()
            .unwrap();
        assert_eq!(
            final_spinner_line.len(),
            SPINNER_MAX_PREFIX_BYTES + SPINNER_FAILED_SUMMARY.len()
        );
        assert_eq!(final_spinner_line.len(), log::MAX_LOG_MESSAGE_BYTES);

        let mut signal_stderr = Vec::new();
        warn_on_exit_failure(ExitStatus::from_raw(9), &mut signal_stderr);
        assert_eq!(
            String::from_utf8(signal_stderr).unwrap(),
            "WARNING: Command terminated by signal!\n"
        );

        for mode in [ExecMode::Captured, ExecMode::Interactive, ExecMode::Spinner] {
            let io_error = exec_with(
                "/nonexistent/podoxide_missing_bin",
                &Vec::<String>::new(),
                &ExecOptions {
                    mode,
                    prefix: "",
                    simulate: false,
                },
                &mut Vec::new(),
                &mut Vec::new(),
            )
            .unwrap_err();
            assert!(
                matches!(io_error, PodError::Io(_)),
                "expected PodError::Io for mode {mode:?}"
            );
        }

        let mut child = build_command("sleep", &["10"])
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
        assert!(
            matches!(poll_error, PodError::Io(ref io_err) if io_err.kind() == io::ErrorKind::Other)
        );
        assert!(child.try_wait().unwrap().is_some());
    }
}
