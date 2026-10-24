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

//! Handler for `pods version`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;
use crate::utilities::format::format_version_header;

/// Formats and outputs the application version string for `pods version`.
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header (`Podoxide v<version>+<count>.<hash>[.dev]`) via
///    [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the version output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_version(&mut io::stdout().lock());
    Ok(())
}

/// Writes the formatted application version string to `writer`.
fn write_version<W: Write>(writer: &mut W) {
    log::write_print(writer, &format_version_header(None));
}

#[cfg(test)]
mod tests {
    use crate::core::context::Context;
    use crate::utilities::format::format_version_header;
    use super::{handle, write_version};

    #[test]
    fn write_version_and_handle_output_application_version() {
        let mut buffer = Vec::new();
        write_version(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert_eq!(output, format!("{}\n", format_version_header(None)));

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
