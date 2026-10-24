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

//! Entry point for the Podoxide CLI tool (`pods`).

#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use podoxide::core::cli;
use podoxide::core::config::DEFAULT_CONFIG_NAME;
use podoxide::core::context::Context;
use podoxide::core::log;
use podoxide::core::modes;
use podoxide::error::{PodError, SystemError};
use podoxide::io::exec::podman;
use podoxide::io::system::env as system_env;

/// Runs the CLI pipeline for `arguments`, logs any error to standard error, and returns the process exit code.
fn execute<I, T>(arguments: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    match run(arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            log::error(&error.to_string());
            ExitCode::FAILURE
        }
    }
}

/// Prints the application name and version string to standard output.
fn handle_version() -> Result<(), PodError> {
    log::print(concat!("Podoxide v", env!("CARGO_PKG_VERSION")));
    Ok(())
}

/// Entry point of the `pods` binary that forwards process command-line arguments to [`execute`].
fn main() -> ExitCode {
    execute(env::args_os().skip(1))
}

/// Verifies host environment prerequisites, parses CLI arguments, loads `pods.conf` when required, and dispatches the selected mode.
fn run<I, T>(arguments: I) -> Result<(), PodError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    verify_environment()?;

    let mut context = Context::new();
    cli::parse_args(&mut context, arguments)?;
    log::set_debug_enabled(context.flags.debug);
    log::debug("Debug mode is enabled.");
    if context.config.name != DEFAULT_CONFIG_NAME {
        log::debug(&format!(
            "Config '{}' is used.",
            context.config.path.display()
        ));
    }
    if context.flags.version {
        return handle_version();
    }

    // Load pods.conf only when required by the selected mode and action.
    // Bypassed for help actions, init, and image prune.
    context.load_config_if_required()?;
    modes::dispatch_mode(&context)
}

/// Verifies that the host OS is Linux, Podman is version 5.8.0+, and elevated execution is confirmed by the user.
fn verify_environment() -> Result<(), PodError> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    verify_environment_with(
        system_env::check_os(),
        podman::check_podman_version(),
        system_env::runs_elevated(),
        &mut stdin,
        &mut stdout,
        &mut stderr,
    )
}

/// Evaluates environment check flags and prompts for confirmation on `reader`/`stdout_writer`/`stderr_writer` when `elevated` is `true`.
fn verify_environment_with<R, W, E>(
    os_supported: bool,
    podman_supported: bool,
    elevated: bool,
    reader: &mut R,
    stdout_writer: &mut W,
    stderr_writer: &mut E,
) -> Result<(), PodError>
where
    R: BufRead,
    W: Write,
    E: Write,
{
    if !os_supported {
        return Err(PodError::from(SystemError::UnsupportedOs));
    }
    if !podman_supported {
        return Err(PodError::from(SystemError::UnsupportedPodmanVersion));
    }
    if elevated {
        system_env::confirm_elevated_execution(reader, stdout_writer, stderr_writer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::io::Cursor;
    use std::os::unix::fs as unix_fs;
    use std::process::{self, Command, ExitCode};

    use super::{execute, run, verify_environment_with};

    #[test]
    fn binary_in_usr_local_bin_uses_current_working_directory() {
        if env::var_os("PODOXIDE_TEST_SUBPROCESS_CWD").is_some() {
            assert_eq!(execute(["init"]), ExitCode::SUCCESS);
            assert_eq!(execute(["init"]), ExitCode::SUCCESS);
            assert_eq!(execute(["up", "recipe"]), ExitCode::SUCCESS);
            assert_eq!(execute(["cfg", "show"]), ExitCode::SUCCESS);
            assert_eq!(execute(["rec", "list"]), ExitCode::SUCCESS);
            return;
        }

        let temp_dir = env::temp_dir().join(format!("podoxide_cwd_test_{}", process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let bin_dir = temp_dir.join("usr").join("local").join("bin");
        let working_dir = temp_dir.join("workspace");
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&working_dir).unwrap();

        let installed_pods = bin_dir.join("pods");
        unix_fs::symlink(env::current_exe().unwrap(), &installed_pods).unwrap();

        let output = Command::new(&installed_pods)
            .current_dir(&working_dir)
            .env("PODOXIDE_TEST_SUBPROCESS_CWD", "1")
            .args([
                "--exact",
                "tests::binary_in_usr_local_bin_uses_current_working_directory",
                "--nocapture",
            ])
            .output()
            .unwrap();
        let stderr_output = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "subprocess failed: {stderr_output}"
        );
        let canonical_working_dir = fs::canonicalize(&working_dir).unwrap();
        assert!(stderr_output.contains(&format!(
            "INFO: Initializing workspace '{}'.",
            canonical_working_dir.display()
        )));
        assert!(stderr_output.contains("INFO: Created 'recipe.pod'."));
        assert!(stderr_output.contains("INFO: Created 'pods.conf'."));
        assert!(stderr_output.contains("WARNING: File 'recipe.pod' already exists."));
        assert!(stderr_output.contains("WARNING: File 'pods.conf' already exists."));
        assert!(working_dir.join("recipe.pod").is_file());
        assert!(working_dir.join("pods.conf").is_file());
        assert!(!bin_dir.join("recipe.pod").exists());
        assert!(!bin_dir.join("pods.conf").exists());

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn execute_pipeline_handles_valid_and_invalid_arguments() {
        assert_eq!(execute(Vec::<String>::new()), ExitCode::SUCCESS);
        assert_eq!(execute(["help"]), ExitCode::SUCCESS);
        assert_eq!(execute(["--version"]), ExitCode::SUCCESS);
        assert_eq!(
            execute(["--debug", "--config=pods", "up", "example-app"]),
            ExitCode::SUCCESS
        );
        assert_eq!(
            execute(["--debug", "--config=staging", "up", "example-app"]),
            ExitCode::FAILURE
        );
        assert_eq!(execute(["up", "example-app"]), ExitCode::SUCCESS);
        assert_eq!(execute(["cmd", "list", "example-app"]), ExitCode::SUCCESS);
        assert_eq!(execute(["cfg", "show"]), ExitCode::SUCCESS);
        assert_eq!(execute(["con", "example-app"]), ExitCode::SUCCESS);
        assert_eq!(execute(["img", "prune"]), ExitCode::SUCCESS);
        assert_eq!(execute(["init", "help"]), ExitCode::SUCCESS);
        assert_eq!(execute(["init"]), ExitCode::SUCCESS);
        assert_eq!(execute(["init", "--config=staging"]), ExitCode::FAILURE);
        assert_eq!(execute(["log", "example-app"]), ExitCode::SUCCESS);
        assert_eq!(execute(["rec", "list"]), ExitCode::SUCCESS);

        assert_eq!(execute(["--unknown-flag"]), ExitCode::FAILURE);
        assert_eq!(execute(["ini"]), ExitCode::FAILURE);
        assert!(run(["up"]).is_err());
    }

    #[test]
    fn verify_environment_enforces_os_podman_and_elevation_checks() {
        let mut empty_input = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let os_error = verify_environment_with(
            false,
            true,
            false,
            &mut empty_input,
            &mut stdout,
            &mut stderr,
        )
        .unwrap_err();
        assert_eq!(os_error.to_string(), "Only Linux is supported.");

        let podman_error = verify_environment_with(
            true,
            false,
            false,
            &mut empty_input,
            &mut stdout,
            &mut stderr,
        )
        .unwrap_err();
        assert_eq!(
            podman_error.to_string(),
            "Podman 5.8.0 or higher is required."
        );

        assert!(
            verify_environment_with(
                true,
                true,
                false,
                &mut empty_input,
                &mut stdout,
                &mut stderr
            )
            .is_ok()
        );

        let mut yes_input = Cursor::new(b"yes\n".to_vec());
        assert!(
            verify_environment_with(
                true,
                true,
                true,
                &mut yes_input,
                &mut stdout,
                &mut stderr
            )
            .is_ok()
        );

        let mut no_input = Cursor::new(b"no\n".to_vec());
        let abort_error =
            verify_environment_with(true, true, true, &mut no_input, &mut stdout, &mut stderr)
                .unwrap_err();
        assert_eq!(abort_error.to_string(), "Aborting execution.");
    }
}
