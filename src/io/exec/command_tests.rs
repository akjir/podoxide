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

//! Unit tests for process execution helpers in [`super`].

use std::ffi::OsString;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use crate::core::log::DebugGuard;
use crate::error::PodError;
use crate::utilities::format;
use super::{
    ExecMode, ExecOptions, ExecOutcome, InterruptPolicy, build_command, exec_capture,
    exec_capture_bounded, exec_editor_with, exec_with, read_bounded_bytes, run_captured,
    warn_on_exit_failure,
};

#[test]
fn exec_capture_handles_success_failure_bounds_and_missing_binary() {
    let output = exec_capture("echo", &["podoxide-test"]).unwrap();
    assert_eq!(output.trim(), "podoxide-test");

    assert!(exec_capture("false", &[]).is_none());
    assert!(exec_capture("/nonexistent/podoxide_missing_bin", &[]).is_none());
    assert!(exec_capture("printf", &["\\377\\376"]).is_none());

    assert_eq!(
        exec_capture_bounded("printf", &["123456"], 6).as_deref(),
        Some("123456")
    );
    assert!(exec_capture_bounded("printf", &["1234567"], 6).is_none());

    assert_eq!(
        ExecOutcome::Succeeded.combine(ExecOutcome::Succeeded),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        ExecOutcome::Succeeded.combine(ExecOutcome::Failed),
        ExecOutcome::Failed
    );
    assert_eq!(
        ExecOutcome::Failed.combine(ExecOutcome::Succeeded),
        ExecOutcome::Failed
    );
    assert_eq!(
        ExecOutcome::Failed.combine(ExecOutcome::Failed),
        ExecOutcome::Failed
    );
}

#[test]
fn exec_editor_with_validates_normalizes_and_runs() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    exec_editor_with(
        "vim",
        Path::new("./pods.conf"),
        true,
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(String::from_utf8(stdout).unwrap(), "vim ./pods.conf\n");
    assert!(stderr.is_empty());

    let mut bare_stdout = Vec::new();
    exec_editor_with(
        "vim",
        Path::new("pods.conf"),
        true,
        &mut bare_stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(String::from_utf8(bare_stdout).unwrap(), "vim ./pods.conf\n");

    let mut abs_stdout = Vec::new();
    exec_editor_with(
        "vim",
        Path::new("/etc/pods/pods.conf"),
        true,
        &mut abs_stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(
        String::from_utf8(abs_stdout).unwrap(),
        "vim /etc/pods/pods.conf\n"
    );

    let mut live_stdout = Vec::new();
    let mut live_stderr = Vec::new();
    assert_eq!(
        exec_editor_with(
            "true",
            Path::new("./recipe.pod"),
            false,
            &mut live_stdout,
            &mut live_stderr,
        )
        .unwrap(),
        ExecOutcome::Succeeded
    );
    assert!(live_stdout.is_empty());
    assert!(live_stderr.is_empty());

    let mut fail_stdout = Vec::new();
    let mut fail_stderr = Vec::new();
    assert_eq!(
        exec_editor_with(
            "false",
            Path::new("./recipe.pod"),
            false,
            &mut fail_stdout,
            &mut fail_stderr,
        )
        .unwrap(),
        ExecOutcome::Failed
    );
    assert!(fail_stdout.is_empty());
    assert!(
        String::from_utf8(fail_stderr)
            .unwrap()
            .contains("WARNING: Command exited with code '1'!")
    );

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
            Path::new("./pods.conf"),
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
        "+!id.conf",
        "./+!id.conf",
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
            Path::new(invalid_path),
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

    let non_utf8 = PathBuf::from(OsString::from_vec(vec![b'.', b'/', 0xFF, b'.', b'c']));
    assert!(matches!(
        exec_editor_with("vim", &non_utf8, true, &mut Vec::new(), &mut Vec::new()),
        Err(PodError::Io(ref io_err)) if io_err.kind() == io::ErrorKind::InvalidInput
    ));
}

#[test]
fn exec_with_routes_streams_bounds_capture_and_handles_sigint_policy() {
    let default_options = ExecOptions::default();
    assert_eq!(default_options.mode, ExecMode::Captured);
    assert_eq!(
        default_options.interrupt_policy,
        InterruptPolicy::WarnOnInterrupt
    );
    assert_eq!(default_options.prefix, "");
    assert!(!default_options.simulate);

    assert_eq!(
        format::format_command("podman", &["run", "--name", "app", "O'Hara", "Hancock 2"]),
        "podman run --name app 'O'\\''Hara' 'Hancock 2'"
    );

    let mut sim_stdout = Vec::new();
    let mut sim_stderr = Vec::new();
    let sim_outcome = exec_with(
        "podman",
        &["create", "--name", "app"],
        &ExecOptions {
            interrupt_policy: InterruptPolicy::WarnOnInterrupt,
            mode: ExecMode::Captured,
            prefix: "Create container 'app': ",
            simulate: true,
        },
        &mut sim_stdout,
        &mut sim_stderr,
    )
    .unwrap();
    assert_eq!(sim_outcome, ExecOutcome::Succeeded);
    assert_eq!(
        String::from_utf8(sim_stdout).unwrap(),
        "Create container 'app': \npodman create --name app\n"
    );
    assert!(sim_stderr.is_empty());

    let mut cap_stdout = Vec::new();
    let mut cap_stderr = Vec::new();
    let cap_outcome = exec_with(
        "printf",
        &["line1\\nline2\\n"],
        &ExecOptions {
            interrupt_policy: InterruptPolicy::WarnOnInterrupt,
            mode: ExecMode::Captured,
            prefix: "Echo: ",
            simulate: false,
        },
        &mut cap_stdout,
        &mut cap_stderr,
    )
    .unwrap();
    assert_eq!(cap_outcome, ExecOutcome::Succeeded);
    assert_eq!(
        String::from_utf8(cap_stdout).unwrap(),
        "Echo: line1\nline2\n"
    );
    assert!(cap_stderr.is_empty());

    let mut mixed_stdout = Vec::new();
    let mut mixed_stderr = Vec::new();
    let mixed_outcome = exec_with(
        "ls",
        &["-d", "/", "/nonexistent/podoxide_missing_path"],
        &ExecOptions {
            interrupt_policy: InterruptPolicy::WarnOnInterrupt,
            mode: ExecMode::Captured,
            prefix: "List: ",
            simulate: false,
        },
        &mut mixed_stdout,
        &mut mixed_stderr,
    )
    .unwrap();
    assert_eq!(mixed_outcome, ExecOutcome::Failed);
    assert_eq!(String::from_utf8(mixed_stdout).unwrap(), "List: /\n");
    let mixed_err_text = String::from_utf8(mixed_stderr).unwrap();
    assert!(mixed_err_text.contains("podoxide_missing_path"));
    assert!(mixed_err_text.contains("WARNING: Command exited with code '"));

    let mut err_only_stdout = Vec::new();
    let mut err_only_stderr = Vec::new();
    let err_only_outcome = exec_with(
        "ls",
        &["-d", "/nonexistent/podoxide_exec_missing_path"],
        &ExecOptions {
            interrupt_policy: InterruptPolicy::WarnOnInterrupt,
            mode: ExecMode::Captured,
            prefix: "Missing: ",
            simulate: false,
        },
        &mut err_only_stdout,
        &mut err_only_stderr,
    )
    .unwrap();
    assert_eq!(err_only_outcome, ExecOutcome::Failed);
    assert!(err_only_stdout.is_empty());
    let err_only_text = String::from_utf8(err_only_stderr).unwrap();
    assert!(err_only_text.starts_with("Missing: "));
    assert!(err_only_text.contains("WARNING: Command exited with code '"));

    let mut bounded_stdout = Vec::new();
    let mut bounded_stderr = Vec::new();
    let bounded_outcome = run_captured(
        "printf",
        &["0123456789abcdef_overflow"],
        "",
        16,
        &mut bounded_stdout,
        &mut bounded_stderr,
    )
    .unwrap();
    assert_eq!(bounded_outcome, ExecOutcome::Succeeded);
    assert_eq!(
        String::from_utf8(bounded_stdout).unwrap(),
        "0123456789abcdef\n"
    );
    assert!(bounded_stderr.is_empty());

    let mut empty_stdout = Vec::new();
    let mut empty_stderr = Vec::new();
    assert_eq!(
        exec_with(
            "true",
            &Vec::<String>::new(),
            &ExecOptions {
                interrupt_policy: InterruptPolicy::WarnOnInterrupt,
                mode: ExecMode::Captured,
                prefix: "Quiet: ",
                simulate: false,
            },
            &mut empty_stdout,
            &mut empty_stderr,
        )
        .unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(String::from_utf8(empty_stdout).unwrap(), "Quiet: ...\n");
    assert!(empty_stderr.is_empty());

    let mut debug_stderr = Vec::new();
    {
        let _guard = DebugGuard::new(true);
        exec_with(
            "true",
            &["--version"],
            &ExecOptions::default(),
            &mut Vec::new(),
            &mut debug_stderr,
        )
        .unwrap();
    }
    assert_eq!(
        String::from_utf8(debug_stderr).unwrap(),
        "DEBUG: Execute: true --version\n"
    );

    let mut interactive_fail_stderr = Vec::new();
    assert_eq!(
        exec_with(
            "false",
            &Vec::<String>::new(),
            &ExecOptions {
                interrupt_policy: InterruptPolicy::WarnOnInterrupt,
                mode: ExecMode::Interactive,
                prefix: "",
                simulate: false,
            },
            &mut Vec::new(),
            &mut interactive_fail_stderr,
        )
        .unwrap(),
        ExecOutcome::Failed
    );
    assert_eq!(
        String::from_utf8(interactive_fail_stderr).unwrap(),
        "WARNING: Command exited with code '1'!\n"
    );

    let mut sigint_quiet_stderr = Vec::new();
    warn_on_exit_failure(
        ExitStatus::from_raw(2),
        InterruptPolicy::QuietOnSigint,
        &mut sigint_quiet_stderr,
    );
    assert!(sigint_quiet_stderr.is_empty());

    let mut sigint_warn_stderr = Vec::new();
    warn_on_exit_failure(
        ExitStatus::from_raw(2),
        InterruptPolicy::WarnOnInterrupt,
        &mut sigint_warn_stderr,
    );
    assert_eq!(
        String::from_utf8(sigint_warn_stderr).unwrap(),
        "WARNING: Command terminated by signal!\n"
    );

    let mut sigkill_quiet_stderr = Vec::new();
    warn_on_exit_failure(
        ExitStatus::from_raw(9),
        InterruptPolicy::QuietOnSigint,
        &mut sigkill_quiet_stderr,
    );
    assert_eq!(
        String::from_utf8(sigkill_quiet_stderr).unwrap(),
        "WARNING: Command terminated by signal!\n"
    );

    for mode in [ExecMode::Captured, ExecMode::Interactive] {
        let io_error = exec_with(
            "/nonexistent/podoxide_missing_bin",
            &Vec::<String>::new(),
            &ExecOptions {
                interrupt_policy: InterruptPolicy::WarnOnInterrupt,
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
}

#[test]
fn read_bounded_bytes_drains_large_pipe_without_sigpipe_while_capping_buffer() {
    let mut child = build_command("dd", &["if=/dev/zero", "bs=1024", "count=128"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout_pipe = child.stdout.take().unwrap();
    let (bytes, truncated) = read_bounded_bytes(stdout_pipe, 64).unwrap();
    let status = child.wait().unwrap();
    assert!(
        status.success(),
        "child writing >80 KiB to bounded pipe must not receive SIGPIPE, status={status:?}"
    );
    assert!(truncated, "expected truncation flag when output > 64 bytes");
    assert_eq!(bytes.len(), 64);

    assert!(
        exec_capture_bounded("dd", &["if=/dev/zero", "bs=1024", "count=128"], 64).is_none(),
        "exec_capture_bounded must still return None on overflow"
    );
}
