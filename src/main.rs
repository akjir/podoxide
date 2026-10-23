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
use std::io::{self, Write};
use std::process::ExitCode;

use podoxide::core::cli;
use podoxide::core::context::Context;
use podoxide::core::modes;
use podoxide::error::PodError;

fn execute<I, T>(arguments: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    match run(arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr(), "ERROR: {error}");
            ExitCode::FAILURE
        }
    }
}

fn handle_help(_context: &Context) -> Result<(), PodError> {
    Ok(())
}

fn handle_version() -> Result<(), PodError> {
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
    let mut context = Context::new();
    cli::parse_args(&mut context, arguments)?;
    if context.flags.version {
        return handle_version();
    }
    if context.flags.help {
        return handle_help(&context);
    }

    context.load_config_if_required()?;
    modes::dispatch_mode(&context)
}

#[cfg(test)]
mod tests {
    use std::process::ExitCode;

    use super::{execute, run};

    #[test]
    fn execute_pipeline_handles_valid_and_invalid_arguments() {
        assert_eq!(execute(Vec::<String>::new()), ExitCode::SUCCESS);
        assert_eq!(execute(["--help"]), ExitCode::SUCCESS);
        assert_eq!(execute(["--version"]), ExitCode::SUCCESS);
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
}
