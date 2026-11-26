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

//! Unit tests for terminal spinner execution helpers in [`super`].

use std::io;
use std::process::Stdio;
use std::time::Duration;

use crate::core::log::{self, DebugGuard};
use crate::error::PodError;
use crate::io::exec::command::{self, ExecOutcome};
use super::{
    MAX_SPINNER_STDERR_BYTES, SPINNER_CLEAR_EOL, SPINNER_FRAMES, SPINNER_MAX_PREFIX_BYTES,
    SPINNER_MAX_SUMMARY_BYTES, SpinnerConfig, exec_spinner_configured, exec_spinner_with,
    poll_child_status, run_spinner_child, wait_child_status,
};

#[test]
fn spinner_drains_oversized_stderr_without_sigpipe_and_bounds_capture() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut observed_outcome = None;
    exec_spinner_with(
        "dd",
        &["if=/dev/zero", "of=/dev/stderr", "bs=1024", "count=128"],
        "  Pull 'big'... ",
        false,
        |outcome| {
            observed_outcome = Some(outcome);
            "Updated."
        },
        &mut stdout,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(
        observed_outcome,
        Some(ExecOutcome::Succeeded),
        "child writing >100 KiB to stderr must not be killed by SIGPIPE"
    );

    let mut child = command::build_command(
        "dd",
        &["if=/dev/zero", "of=/dev/stderr", "bs=1024", "count=128"],
    )
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .spawn()
    .unwrap();
    let (status, captured) = run_spinner_child(
        &mut child,
        "  Pull 'big'... ",
        false,
        Duration::from_millis(2),
        &mut Vec::new(),
    )
    .unwrap();
    assert!(status.success(), "child must exit 0 after draining stderr");
    assert_eq!(captured.len(), MAX_SPINNER_STDERR_BYTES);
}

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

    let mut wait_child = command::build_command("sleep", &["10"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let wait_error =
        wait_child_status(&mut wait_child, Err(io::Error::other("wait failure"))).unwrap_err();
    assert!(matches!(
        wait_error,
        PodError::Io(ref io_err) if io_err.kind() == io::ErrorKind::Other
    ));
    assert!(
        wait_child.try_wait().unwrap().is_some(),
        "wait failure must kill and reap child before thread::scope joins stderr reader"
    );
}
