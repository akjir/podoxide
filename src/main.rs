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

fn handle_help(_context: &Context) -> Result<(), PodError> {
    Ok(())
}

fn handle_version() -> Result<(), PodError> {
    log::print(concat!("Podoxide v", env!("CARGO_PKG_VERSION")));
    Ok(())
}

fn main() -> ExitCode {
    execute(env::args_os().skip(1))
}

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
    if context.flags.help {
        return handle_help(&context);
    }

    context.load_config_if_required()?;
    modes::dispatch_mode(&context)
}

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
    use std::io::Cursor;
    use std::process::ExitCode;

    use super::{execute, run, verify_environment_with};

    #[test]
    fn execute_pipeline_handles_valid_and_invalid_arguments() {
        assert_eq!(execute(Vec::<String>::new()), ExitCode::SUCCESS);
        assert_eq!(execute(["--help"]), ExitCode::SUCCESS);
        assert_eq!(execute(["--version"]), ExitCode::SUCCESS);
        assert_eq!(
            execute(["--debug", "--config=staging", "up", "example-app"]),
            ExitCode::SUCCESS
        );
        assert_eq!(execute(["up", "example-app"]), ExitCode::SUCCESS);
        assert_eq!(execute(["cmd", "list", "example-app"]), ExitCode::SUCCESS);
        assert_eq!(execute(["cfg", "show"]), ExitCode::SUCCESS);
        assert_eq!(execute(["con", "example-app"]), ExitCode::SUCCESS);
        assert_eq!(execute(["img", "prune"]), ExitCode::SUCCESS);
        assert_eq!(execute(["init"]), ExitCode::SUCCESS);
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
