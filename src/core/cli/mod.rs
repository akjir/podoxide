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

//! CLI argument scanner, flag validator, and mode/action resolver.

mod flags;
#[cfg(test)]
mod flags_tests;
mod positionals;
#[cfg(test)]
mod tests;

use std::ffi::OsString;

use crate::core::config;
use crate::core::context::{Context, Flags};
use crate::error::CliError;

/// Maximum number of command-line arguments accepted by the parser.
pub const MAX_CLI_ARGS: usize = 256;

/// Parses an iterator of command-line arguments (excluding `argv[0]`) into [`Context`].
///
/// # Errors
///
/// Returns [`CliError`] if argument limits are exceeded, an argument is not valid
/// UTF-8, an unknown or unexpected flag is provided, a required flag value or target
/// is missing, or a mode/action combination is invalid.
pub fn parse_args<I, T>(context: &mut Context, arguments: I) -> Result<(), CliError>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    context.flags = Flags::new();
    let positionals = flags::scan_raw_arguments(&mut context.flags, arguments)?;
    let (action, parameters) = positionals::parse_mode(positionals)?;
    context.action = action;
    context.parameters = parameters;
    flags::validate_mode_flags(context)?;
    context.config.name = config::resolve_config_name(context.flags.config.as_deref());
    context.config.path = config::resolve_config_path(context.flags.config.as_deref());
    if context.flags.simulate() {
        context.config.simulate = true;
    }
    Ok(())
}
