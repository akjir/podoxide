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

//! Shared RAII workspace and deterministic mock `podman` harness for CLI integration tests.

use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const MOCK_PODMAN_SCRIPT: &str = r#"#!/bin/sh
set -eu

if [ -n "${MOCK_PODMAN_LOG:-}" ]; then
    printf '%s\n' "$*" >> "$MOCK_PODMAN_LOG"
fi

cmd="${1:-}"
sub="${2:-}"

if [ "$cmd" = "--version" ]; then
    if [ -n "${MOCK_PODMAN_VERSION_FAIL:-}" ]; then
        exit 1
    fi
    printf 'podman version %s\n' "${MOCK_PODMAN_VERSION:-5.8.0}"
    exit 0
fi

if [ -n "${MOCK_PODMAN_FAIL_CMD:-}" ]; then
    if [ "$MOCK_PODMAN_FAIL_CMD" = "$cmd" ] || [ "$MOCK_PODMAN_FAIL_CMD" = "$cmd $sub" ]; then
        if [ -n "${MOCK_PODMAN_FAIL_MSG:-}" ]; then
            printf '%s\n' "$MOCK_PODMAN_FAIL_MSG" >&2
        fi
        exit "${MOCK_PODMAN_FAIL_CODE:-125}"
    fi
fi

case "$cmd" in
    ps)
        for arg in "$@"; do
            if [ "$arg" = "--no-trunc" ]; then
                if [ -n "${MOCK_PODMAN_IN_USE_IMAGES:-}" ]; then
                    printf '%s' "$MOCK_PODMAN_IN_USE_IMAGES"
                fi
                exit 0
            fi
        done
        if [ -n "${MOCK_PODMAN_PS_FILE:-}" ] && [ -f "$MOCK_PODMAN_PS_FILE" ]; then
            cat "$MOCK_PODMAN_PS_FILE"
        elif [ -n "${MOCK_PODMAN_PS:-}" ]; then
            printf '%s' "$MOCK_PODMAN_PS"
        fi
        exit 0
        ;;
    images)
        if [ -n "${MOCK_PODMAN_IMAGES_FILE:-}" ] && [ -f "$MOCK_PODMAN_IMAGES_FILE" ]; then
            cat "$MOCK_PODMAN_IMAGES_FILE"
        elif [ -n "${MOCK_PODMAN_IMAGES:-}" ]; then
            printf '%s' "$MOCK_PODMAN_IMAGES"
        fi
        exit 0
        ;;
    image)
        if [ "$sub" = "inspect" ]; then
            target=""
            for arg in "$@"; do
                target="$arg"
            done
            if [ -n "${MOCK_PODMAN_STATE_DIR:-}" ] \
                && [ -f "$MOCK_PODMAN_STATE_DIR/pulled" ] \
                && grep -Fxq -- "$target" "$MOCK_PODMAN_STATE_DIR/pulled"; then
                printf '%s\n' "${MOCK_PODMAN_IMAGE_ID_AFTER:-0000000000000000000000000000000000000000000000000000000000000002}"
            else
                printf '%s\n' "${MOCK_PODMAN_IMAGE_ID_BEFORE:-0000000000000000000000000000000000000000000000000000000000000001}"
            fi
            exit 0
        fi
        if [ "$sub" = "prune" ]; then
            if [ -n "${MOCK_PODMAN_PRUNE_STDOUT:-}" ]; then
                printf '%s\n' "$MOCK_PODMAN_PRUNE_STDOUT"
            fi
            exit 0
        fi
        exit 0
        ;;
    container)
        if [ "$sub" = "exists" ]; then
            exit "${MOCK_PODMAN_CONTAINER_EXISTS_EXIT:-0}"
        fi
        if [ "$sub" = "inspect" ]; then
            for arg in "$@"; do
                if [ "$arg" = "{{.State.Status}}" ]; then
                    printf '%s\n' "${MOCK_PODMAN_CONTAINER_STATUS:-running}"
                    exit 0
                fi
            done
            printf '%s\n' "${MOCK_PODMAN_CONTAINER_IMAGE_ID:-0000000000000000000000000000000000000000000000000000000000000001}"
            exit 0
        fi
        exit 0
        ;;
    pod)
        if [ "$sub" = "exists" ]; then
            exit "${MOCK_PODMAN_POD_EXISTS_EXIT:-1}"
        fi
        exit 0
        ;;
    pull)
        if [ -n "${MOCK_PODMAN_STATE_DIR:-}" ]; then
            mkdir -p "$MOCK_PODMAN_STATE_DIR"
            if [ -z "${MOCK_PODMAN_UPDATE_IMAGE:-}" ] || [ "${MOCK_PODMAN_UPDATE_IMAGE:-}" = "$sub" ]; then
                printf '%s\n' "$sub" >> "$MOCK_PODMAN_STATE_DIR/pulled"
            fi
        fi
        exit 0
        ;;
    *)
        exit 0
        ;;
esac
"#;

static WORKSPACE_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Captured process result returned by executing the `pods` CLI binary.
pub struct CliOutput {
    pub stderr: String,
    pub stdout: String,
    pub success: bool,
}

/// Isolated temporary workspace with an RAII cleanup guard and deterministic mock `podman` on `PATH`.
pub struct TestWorkspace {
    mock_bin_dir: PathBuf,
    root_dir: PathBuf,
    work_dir: PathBuf,
}

impl TestWorkspace {
    /// Creates an isolated temporary root containing a `workspace/` directory and a mock `podman` binary.
    #[must_use]
    pub fn new(label: &str) -> Self {
        let sequence = WORKSPACE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let root_dir =
            env::temp_dir().join(format!("podoxide_it_{label}_{}_{sequence}", process::id()));
        let _ = fs::remove_dir_all(&root_dir);
        let mock_bin_dir = root_dir.join("mock_bin");
        let work_dir = root_dir.join("workspace");
        fs::create_dir_all(&mock_bin_dir).expect("create mock bin directory");
        fs::create_dir_all(&work_dir).expect("create workspace directory");

        let mock_podman = mock_bin_dir.join("podman");
        fs::write(&mock_podman, MOCK_PODMAN_SCRIPT).expect("write mock podman script");
        fs::set_permissions(&mock_podman, fs::Permissions::from_mode(0o755))
            .expect("set executable bit on mock podman");

        Self {
            mock_bin_dir,
            root_dir,
            work_dir,
        }
    }

    /// Returns the working directory used as `current_dir` for CLI invocations.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.work_dir
    }

    /// Executes `binary_path` in [`Self::dir`] with the mock `podman` stub and additional environment overrides.
    #[must_use]
    pub fn run_binary_with_env(
        &self,
        binary_path: &Path,
        arguments: &[&str],
        stdin_bytes: &[u8],
        env_vars: &[(&str, &str)],
    ) -> CliOutput {
        let mut command = Command::new(binary_path);
        command
            .current_dir(&self.work_dir)
            .env("PATH", self.path_env())
            .env_remove("NO_COLOR")
            .args(arguments)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env_vars {
            command.env(key, value);
        }

        let mut child = command.spawn().expect("spawn pods CLI binary");
        if let Some(mut child_stdin) = child.stdin.take() {
            let _ = child_stdin.write_all(stdin_bytes);
        }
        let output = child.wait_with_output().expect("wait for pods CLI binary");
        CliOutput {
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            success: output.status.success(),
        }
    }

    /// Executes `pods` in [`Self::dir`] with the mock `podman` stub first on `PATH`.
    #[must_use]
    pub fn run_pods(&self, arguments: &[&str], stdin_bytes: &[u8]) -> CliOutput {
        self.run_pods_with_env(arguments, stdin_bytes, &[])
    }

    /// Executes `pods` in [`Self::dir`] with the mock `podman` stub and additional environment overrides.
    #[must_use]
    pub fn run_pods_with_env(
        &self,
        arguments: &[&str],
        stdin_bytes: &[u8],
        env_vars: &[(&str, &str)],
    ) -> CliOutput {
        self.run_binary_with_env(
            Path::new(env!("CARGO_BIN_EXE_pods")),
            arguments,
            stdin_bytes,
            env_vars,
        )
    }

    /// Builds a `PATH` value with this workspace's `mock_bin` prepended before the inherited `PATH`.
    #[must_use]
    fn path_env(&self) -> OsString {
        let mut combined = OsString::from(self.mock_bin_dir.as_os_str());
        if let Some(existing) = env::var_os("PATH")
            && !existing.is_empty()
        {
            combined.push(":");
            combined.push(existing);
        }
        combined
    }
}

impl Drop for TestWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root_dir);
    }
}
