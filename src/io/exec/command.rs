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
use std::process::{Command, ExitStatus};

use crate::core::log;
use crate::error::{PodError, SystemError};
use crate::utilities::string::escape_shell;

const ESTIMATED_ARG_BYTES: usize = 12;

/// Execution options controlling simulation, interactive stdio inheritance, and output prefixing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecOptions<'a> {
    pub interactive: bool,
    pub prefix: &'a str,
    pub simulate: bool,
}

impl ExecOptions<'_> {
    /// Creates a new [`ExecOptions`] instance with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Executes `program` with `arguments` using `options`, or prints the command when `options.simulate` is `true`.
///
/// # Errors
///
/// Returns [`PodError`] if spawning the process fails or if the process exits with a non-zero status.
pub fn exec<S: AsRef<str>>(
    program: &str,
    arguments: &[S],
    options: &ExecOptions<'_>,
) -> Result<(), PodError> {
    exec_with(
        program,
        arguments,
        options,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    )
}

/// Executes `program` with `arguments` directly and returns captured `stdout` when exit status is 0.
#[must_use]
pub fn exec_capture(program: &str, arguments: &[&str]) -> Option<String> {
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
/// Returns [`PodError`] if spawning the process fails or if the process exits with a non-zero status.
pub fn exec_with<S, W, E>(
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
        return run_interactive(program, arguments);
    }
    run_captured(program, arguments, options.prefix, stdout)
}

/// Formats `program` and `arguments` into a single shell-escaped command string.
#[must_use]
pub fn format_command<S: AsRef<str>>(program: &str, arguments: &[S]) -> String {
    let escaped_program = escape_shell(program);
    let capacity = arguments
        .len()
        .saturating_mul(ESTIMATED_ARG_BYTES)
        .saturating_add(escaped_program.len())
        .min(log::MAX_LOG_MESSAGE_BYTES);
    let mut formatted = String::with_capacity(capacity);
    formatted.push_str(&escaped_program);
    for argument in arguments {
        formatted.push(' ');
        formatted.push_str(&escape_shell(argument.as_ref()));
    }
    formatted
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

/// Verifies that `status` succeeded or returns [`SystemError::CommandFailed`].
fn ensure_exit_success(status: ExitStatus) -> Result<(), PodError> {
    if status.success() {
        return Ok(());
    }
    Err(PodError::from(SystemError::CommandFailed(status.code())))
}

/// Executes `program` with `arguments`, writing captured output lines with `prefix` to `stdout`.
fn run_captured<S, W>(
    program: &str,
    arguments: &[S],
    prefix: &str,
    stdout: &mut W,
) -> Result<(), PodError>
where
    S: AsRef<str>,
    W: Write,
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
    ensure_exit_success(output.status)
}

/// Executes `program` with `arguments` interactively, inheriting standard input, output, and error streams.
fn run_interactive<S: AsRef<str>>(program: &str, arguments: &[S]) -> Result<(), PodError> {
    let status = build_command(program, arguments).status()?;
    ensure_exit_success(status)
}

#[cfg(test)]
mod tests {
    use crate::error::{PodError, SystemError};
    use super::{ExecOptions, exec, exec_capture, exec_with, format_command};

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
        let default_options = ExecOptions::new();
        assert!(!default_options.interactive);
        assert_eq!(default_options.prefix, "");
        assert!(!default_options.simulate);

        assert_eq!(
            format_command("podman", &["run", "--name", "app", "O'Hara", "Hancock 2"]),
            "podman run --name app 'O'\\''Hara' 'Hancock 2'"
        );

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let sim_options = ExecOptions {
            interactive: false,
            prefix: "Create container 'app': ",
            simulate: true,
        };
        exec_with(
            "podman",
            &["create", "--name", "app"],
            &sim_options,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        let sim_output = String::from_utf8(stdout).unwrap();
        assert_eq!(
            sim_output,
            "Create container 'app': \npodman create --name app\n"
        );
        assert!(stderr.is_empty());

        let mut no_prefix_stdout = Vec::new();
        let sim_no_prefix = ExecOptions {
            interactive: true,
            prefix: "",
            simulate: true,
        };
        exec_with(
            "podman",
            &["pull", "docker.io/alpine:latest"],
            &sim_no_prefix,
            &mut no_prefix_stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(no_prefix_stdout).unwrap(),
            "podman pull docker.io/alpine:latest\n"
        );

        let mut cap_stdout = Vec::new();
        let mut cap_stderr = Vec::new();
        let cap_options = ExecOptions {
            interactive: false,
            prefix: "Echo: ",
            simulate: false,
        };
        exec_with(
            "printf",
            &["line1\\nline2\\n"],
            &cap_options,
            &mut cap_stdout,
            &mut cap_stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(cap_stdout).unwrap(),
            "Echo: line1\nline2\n"
        );

        let mut stderr_capture_out = Vec::new();
        let stderr_capture_err = exec_with(
            "ls",
            &["-d", "/", "/nonexistent/podoxide_missing_path"],
            &ExecOptions {
                interactive: false,
                prefix: "List: ",
                simulate: false,
            },
            &mut stderr_capture_out,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(
            stderr_capture_err,
            PodError::System(SystemError::CommandFailed(Some(_)))
        ));
        let stderr_capture_text = String::from_utf8(stderr_capture_out).unwrap();
        assert!(stderr_capture_text.starts_with("List: /\n"));
        assert!(stderr_capture_text.lines().count() >= 2);

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

        exec(
            "true",
            &Vec::<String>::new(),
            &ExecOptions {
                interactive: true,
                prefix: "",
                simulate: false,
            },
        )
        .unwrap();

        let fail_err = exec_with(
            "false",
            &Vec::<String>::new(),
            &ExecOptions::new(),
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap_err();
        match fail_err {
            PodError::System(SystemError::CommandFailed(Some(code))) => assert_ne!(code, 0),
            other => panic!("expected CommandFailed, got {other:?}"),
        }

        let interactive_fail_err = exec_with(
            "false",
            &Vec::<String>::new(),
            &ExecOptions {
                interactive: true,
                prefix: "",
                simulate: false,
            },
            &mut Vec::new(),
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(matches!(
            interactive_fail_err,
            PodError::System(SystemError::CommandFailed(Some(_)))
        ));

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
