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
use std::path::Path;
use std::process::ExitCode;

use podoxide::core::cli;
use podoxide::core::config::DEFAULT_CONFIG_PATH;
use podoxide::core::context::Context;
use podoxide::core::log;
use podoxide::core::modes;
use podoxide::error::{PodError, SystemError};
use podoxide::io::exec::command::ExecOutcome;
use podoxide::io::exec::podman;
use podoxide::io::system::env as system_env;

/// Runs the CLI pipeline for `arguments`, logs any error to standard error, and returns the process exit code.
fn execute<I, T>(arguments: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    finish_execution(run(arguments), &mut io::stderr().lock())
}

/// Maps a CLI run result to an [`ExitCode`], logging structured errors to `stderr` without duplicating output for [`ExecOutcome::Failed`].
fn finish_execution<E: Write>(result: Result<ExecOutcome, PodError>, stderr: &mut E) -> ExitCode {
    match result {
        Ok(ExecOutcome::Succeeded) => ExitCode::SUCCESS,
        Ok(ExecOutcome::Failed) => ExitCode::FAILURE,
        Err(error) => {
            log::write_pod_error(stderr, &error);
            ExitCode::FAILURE
        }
    }
}

/// Entry point of the `pods` binary that forwards process command-line arguments to [`execute`].
fn main() -> ExitCode {
    execute(env::args_os().skip(1))
}

/// Parses CLI arguments, verifies host environment prerequisites for the selected action, loads `pods.conf` when required, and dispatches the selected mode.
fn run<I, T>(arguments: I) -> Result<ExecOutcome, PodError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    run_with(arguments, &mut io::stderr())
}

/// Executes the CLI pipeline with startup debug logs written to `stderr`.
fn run_with<I, T, E>(arguments: I, stderr: &mut E) -> Result<ExecOutcome, PodError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
    E: Write,
{
    let mut context = Context::new();
    cli::parse_args(&mut context, arguments)?;
    log::set_debug_enabled(context.flags.debug());
    log::write_debug(stderr, "Debug mode is enabled.");
    if context.uses_config_file() && context.config.path != Path::new(DEFAULT_CONFIG_PATH) {
        log::write_debug(
            stderr,
            &format!("Config '{}' is used.", context.config.path.display()),
        );
    }

    verify_environment_for(&context)?;

    // Load pods.conf only when required by the selected mode and action.
    // Bypassed for help actions, version, init, and config show.
    context.load_config_if_required()?;
    modes::dispatch_mode(&context)
}

/// Verifies host OS, Podman `5.8.0+`, and elevated execution confirmation as required by `context`.
fn verify_environment_for(context: &Context) -> Result<(), PodError> {
    let mut stdin = io::stdin().lock();
    let mut stderr = io::stderr().lock();
    let podman_supported = !context.requires_podman() || podman::check_version();
    let elevated = context.requires_elevation_confirmation() && system_env::runs_elevated();
    verify_environment_with(
        context,
        system_env::check_os(),
        podman_supported,
        elevated,
        &mut stdin,
        &mut stderr,
    )
}

/// Evaluates OS, Podman version, and elevated-execution checks required by `context`, prompting on `reader` and `stderr_writer` when elevated confirmation is required.
fn verify_environment_with<R, E>(
    context: &Context,
    os_supported: bool,
    podman_supported: bool,
    elevated: bool,
    reader: &mut R,
    stderr_writer: &mut E,
) -> Result<(), PodError>
where
    R: BufRead,
    E: Write,
{
    if !os_supported {
        return Err(PodError::from(SystemError::UnsupportedOs));
    }
    if context.requires_podman() && !podman_supported {
        return Err(PodError::from(SystemError::UnsupportedPodmanVersion));
    }
    if context.requires_elevation_confirmation() && elevated {
        system_env::confirm_elevated_execution(reader, stderr_writer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::process::ExitCode;

    use podoxide::core::context::{
        CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, ImageAction,
        InitAction, LogsAction, ModeAction, RecipeAction,
    };
    use podoxide::core::log;
    use podoxide::error::{CliErrorKind, PodError, SystemError};
    use podoxide::io::exec::command::ExecOutcome;
    use super::{execute, finish_execution, run, run_with, verify_environment_with};

    struct DebugGuard {
        previous: bool,
    }

    impl DebugGuard {
        fn new(enabled: bool) -> Self {
            let previous = log::is_debug_enabled();
            log::set_debug_enabled(enabled);
            Self { previous }
        }
    }

    impl Drop for DebugGuard {
        fn drop(&mut self) {
            log::set_debug_enabled(self.previous);
        }
    }

    fn assert_env(
        action: ModeAction,
        os_supported: bool,
        podman_supported: bool,
        elevated: bool,
        input: &[u8],
        expected_err: Option<&str>,
    ) {
        let (result, _) = check_env(action, os_supported, podman_supported, elevated, input);
        match expected_err {
            None => assert!(result.is_ok(), "expected Ok for {action:?}"),
            Some(message) => assert_eq!(result.unwrap_err().to_string(), message),
        }
    }

    fn check_env(
        action: ModeAction,
        os_supported: bool,
        podman_supported: bool,
        elevated: bool,
        input: &[u8],
    ) -> (Result<(), PodError>, Vec<u8>) {
        let mut context = Context::new();
        context.action = action;
        let mut reader = Cursor::new(input);
        let mut stderr = Vec::new();
        let result = verify_environment_with(
            &context,
            os_supported,
            podman_supported,
            elevated,
            &mut reader,
            &mut stderr,
        );
        (result, stderr)
    }

    #[test]
    fn execute_and_run_handle_offline_actions_and_cli_errors() {
        let _guard = DebugGuard::new(false);

        assert_eq!(execute(Vec::<String>::new()), ExitCode::SUCCESS);
        assert_eq!(execute(["help"]), ExitCode::SUCCESS);
        assert_eq!(execute(["version"]), ExitCode::SUCCESS);
        assert_eq!(execute(["--debug", "version"]), ExitCode::SUCCESS);
        assert!(log::is_debug_enabled());
        assert_eq!(execute(["cmd", "help"]), ExitCode::SUCCESS);
        assert!(!log::is_debug_enabled());
        assert_eq!(execute(["init", "help"]), ExitCode::SUCCESS);

        assert_eq!(execute(["--unknown-flag"]), ExitCode::FAILURE);
        assert_eq!(execute(["ini"]), ExitCode::FAILURE);
        let missing_target = run(["up"]).unwrap_err();
        assert!(matches!(
            missing_target,
            PodError::Cli(ref error) if error.kind == CliErrorKind::MissingTarget
        ));

        let mut version_stderr = Vec::new();
        assert_eq!(
            run_with(
                ["--debug", "--config=staging", "version"],
                &mut version_stderr
            )
            .unwrap(),
            ExecOutcome::Succeeded
        );
        let version_debug = String::from_utf8(version_stderr).unwrap();
        assert!(version_debug.contains("DEBUG: Debug mode is enabled.\n"));
        assert!(
            !version_debug.contains("DEBUG: Config './staging.conf' is used."),
            "version must not log unused config path: {version_debug:?}"
        );

        let mut show_stderr = Vec::new();
        let _ = run_with(
            ["--debug", "--config=staging", "cfg", "show"],
            &mut show_stderr,
        );
        let show_debug = String::from_utf8(show_stderr).unwrap();
        assert!(
            show_debug.contains("DEBUG: Config './staging.conf' is used.\n"),
            "config show must log custom config path: {show_debug:?}"
        );
    }

    #[test]
    fn finish_execution_maps_outcomes_and_avoids_duplicate_error_on_failed_outcome() {
        let mut ok_stderr = Vec::new();
        assert_eq!(
            finish_execution(Ok(ExecOutcome::Succeeded), &mut ok_stderr),
            ExitCode::SUCCESS
        );
        assert!(ok_stderr.is_empty());

        let mut failed_stderr = Vec::new();
        assert_eq!(
            finish_execution(Ok(ExecOutcome::Failed), &mut failed_stderr),
            ExitCode::FAILURE
        );
        assert!(
            failed_stderr.is_empty(),
            "expected no duplicate ERROR line on ExecOutcome::Failed, got {failed_stderr:?}"
        );

        let mut err_stderr = Vec::new();
        assert_eq!(
            finish_execution(
                Err(PodError::from(SystemError::UnsupportedOs)),
                &mut err_stderr
            ),
            ExitCode::FAILURE
        );
        assert_eq!(
            String::from_utf8(err_stderr).unwrap(),
            "ERROR: Only Linux is supported.\n"
        );
    }

    #[test]
    fn verify_environment_enforces_os_podman_and_elevation_checks() {
        // Read-only offline actions bypass both Podman version and elevated-execution checks.
        let bypass_actions = [
            ModeAction::Command(CommandAction::Help),
            ModeAction::Command(CommandAction::List),
            ModeAction::Config(ConfigAction::Help),
            ModeAction::Config(ConfigAction::Inspect),
            ModeAction::Config(ConfigAction::Show),
            ModeAction::Connect(ConnectAction::Help),
            ModeAction::Default(DefaultAction::Help),
            ModeAction::Default(DefaultAction::Version),
            ModeAction::Image(ImageAction::Help),
            ModeAction::Init(InitAction::Help),
            ModeAction::Logs(LogsAction::Help),
            ModeAction::Recipe(RecipeAction::Help),
            ModeAction::Recipe(RecipeAction::Inspect),
            ModeAction::Recipe(RecipeAction::List),
            ModeAction::Recipe(RecipeAction::Show),
        ];
        for action in bypass_actions {
            let (ok_res, prompt_err) = check_env(action, true, false, true, b"");
            assert!(ok_res.is_ok(), "expected {action:?} to bypass host checks");
            assert!(prompt_err.is_empty(), "expected no prompt for {action:?}");
            let (os_err, _) = check_env(action, false, false, false, b"");
            assert_eq!(os_err.unwrap_err().to_string(), "Only Linux is supported.");
        }

        // Write/editor offline actions bypass Podman version check while still checking OS and elevation.
        let os_err = Some("Only Linux is supported.");
        let podman_err = Some("Podman 5.8.0 or higher is required.");
        let abort_err = Some("Aborting execution.");

        for offline_write in [
            ModeAction::Config(ConfigAction::Edit),
            ModeAction::Init(InitAction::Create),
            ModeAction::Recipe(RecipeAction::Edit),
        ] {
            assert_env(offline_write, true, false, false, b"", None);
            assert_env(offline_write, false, false, false, b"", os_err);
            assert_env(offline_write, true, false, true, b"yes\n", None);
            assert_env(offline_write, true, false, true, b"no\n", abort_err);
        }

        let up = ModeAction::Default(DefaultAction::Up);

        assert_env(up, false, true, false, b"", os_err);
        assert_env(up, true, false, false, b"", podman_err);
        assert_env(up, true, true, false, b"", None);
        assert_env(up, true, true, true, b"yes\n", None);
        assert_env(up, true, true, true, b"no\n", abort_err);
    }
}
