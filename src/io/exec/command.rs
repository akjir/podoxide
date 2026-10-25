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

use std::io::Write;
use std::process::{Command, ExitStatus};

use crate::core::log;
use crate::error::PodError;
use crate::utilities::format::format_command;

/// Execution options controlling simulation, interactive stdio inheritance, and output prefixing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct ExecOptions<'a> {
    pub(super) interactive: bool,
    pub(super) prefix: &'a str,
    pub(super) simulate: bool,
}

/// Executes `editor` with `file_path` interactively (or prints the command when `simulate` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if spawning the editor process fails.
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
    exec_with(
        editor,
        &[file_path],
        &ExecOptions {
            interactive: true,
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
    let output = Command::new(program).args(arguments).output().ok()?;
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
        let formatted = format_command(program, arguments);
        if !options.prefix.is_empty() {
            log::write_print(stdout, options.prefix);
        }
        log::write_print(stdout, &formatted);
        return Ok(());
    }
    if log::is_debug_enabled() {
        let formatted = format_command(program, arguments);
        log::write_debug(stderr, &format!("Execute: {formatted}"));
    }
    if options.interactive {
        return run_interactive(program, arguments, stderr);
    }
    run_captured(program, arguments, options.prefix, stdout, stderr)
}

/// Constructs a [`Command`] for `program` populated with `arguments`.
fn build_command<S: AsRef<str>>(program: &str, arguments: &[S]) -> Command {
    let mut command = Command::new(program);
    for argument in arguments {
        command.arg(argument.as_ref());
    }
    command
}

/// Writes each line of `text` to `stdout`, prepending `prefix` to the first emitted line.
fn emit_captured_lines<W: Write>(
    text: &str,
    prefix: &str,
    stdout: &mut W,
    output_captured: &mut bool,
) {
    for line in text.lines() {
        if !*output_captured {
            log::write_print(stdout, &format!("{prefix}{line}"));
            *output_captured = true;
        } else {
            log::write_print(stdout, line);
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
    let mut output_captured = false;
    emit_captured_lines(&stdout_text, prefix, stdout, &mut output_captured);
    emit_captured_lines(&stderr_text, prefix, stdout, &mut output_captured);
    if !output_captured {
        log::write_print(stdout, &format!("{prefix}..."));
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

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    use crate::core::log;
    use crate::error::PodError;
    use crate::utilities::format::format_command;
    use super::{ExecOptions, exec_capture, exec_with, warn_on_exit_failure};

    #[test]
    fn exec_capture_handles_success_failure_and_missing_binary() {
        let output = exec_capture("echo", &["podoxide-test"]).unwrap();
        assert_eq!(output.trim(), "podoxide-test");

        assert!(exec_capture("false", &[]).is_none());
        assert!(exec_capture("/nonexistent/podoxide_missing_bin", &[]).is_none());
        assert!(exec_capture("printf", &["\\377\\376"]).is_none());
    }

    #[test]
    fn exec_simulate_captured_interactive_and_error_handling() {
        let default_options = ExecOptions::default();
        assert!(!default_options.interactive);
        assert_eq!(default_options.prefix, "");
        assert!(!default_options.simulate);

        assert_eq!(
            format_command("podman", &["run", "--name", "app", "O'Hara", "Hancock 2"]),
            "podman run --name app 'O'\\''Hara' 'Hancock 2'"
        );

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let simulate_options = ExecOptions {
            interactive: false,
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
            interactive: true,
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
            interactive: false,
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
                interactive: false,
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
                interactive: false,
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
                interactive: false,
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
                interactive: false,
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
                interactive: true,
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

        let mut signal_stderr = Vec::new();
        warn_on_exit_failure(ExitStatus::from_raw(9), &mut signal_stderr);
        assert_eq!(
            String::from_utf8(signal_stderr).unwrap(),
            "WARNING: Command terminated by signal!\n"
        );

        for interactive in [false, true] {
            let io_error = exec_with(
                "/nonexistent/podoxide_missing_bin",
                &Vec::<String>::new(),
                &ExecOptions {
                    interactive,
                    prefix: "",
                    simulate: false,
                },
                &mut Vec::new(),
                &mut Vec::new(),
            )
            .unwrap_err();
            assert!(matches!(io_error, PodError::Io(_)));
        }
    }
}
