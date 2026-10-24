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

//! Handler for `pods help` (global CLI help).

use crate::core::context::Context;
use crate::error::PodError;

/// Formats and outputs the global CLI help text for `pods` (default mode).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header, global usage synopsis (`pods [MODE] [OPTIONS] [ACTION] [TARGETS...]`),
///    available modes and 3-letter aliases, global options, default mode lifecycle and status
///    actions, and target syntax via `log::print`.
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    // Step 1: Format and print the global default mode help text without reading any files.
    Ok(())
}
