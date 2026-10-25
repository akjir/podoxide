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

//! Handler for `pods init help`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;
use crate::utilities::format::format_version_header;

/// Formats and outputs the CLI help text for `pods init`.
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header, `pods init` usage synopsis, options (`--debug`, `--simulate`),
///    and actions (`create`, `help`) via [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_help(&mut io::stdout().lock());
    Ok(())
}

/// Writes the `pods init` help text to `writer`.
fn write_help<W: Write>(writer: &mut W) {
    log::write_print(writer, &format_version_header(None));
    log::write_print(writer, "");
    log::write_print(writer, "Usage: pods init [OPTIONS] [ACTION]");
    log::write_print(writer, "");
    log::write_print(writer, "OPTIONS:");
    log::write_print(writer, "  --debug            enable debug output");
    log::write_print(
        writer,
        "  --simulate         preview generated commands without executing them",
    );
    log::write_print(writer, "");
    log::write_print(writer, "ACTIONS:");
    log::write_print(
        writer,
        "  create             initialize default configuration and recipe (default)",
    );
    log::write_print(writer, "  help               display this help and exit");
}

#[cfg(test)]
mod tests {
    use crate::core::context::Context;
    use crate::utilities::format::format_version_header;
    use super::{handle, write_help};

    #[test]
    fn write_help_and_handle_output_init_usage() {
        let mut buffer = Vec::new();
        write_help(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(&format!("{}\n", format_version_header(None))));
        assert!(output.contains("Usage: pods init [OPTIONS] [ACTION]\n"));
        assert!(output.contains("OPTIONS:\n"));
        assert!(!output.contains("--config"));
        assert!(output.contains("  --debug            enable debug output\n"));
        assert!(output.contains(
            "  --simulate         preview generated commands without executing them\n"
        ));
        assert!(output.contains("ACTIONS:\n"));
        assert!(output.contains(
            "  create             initialize default configuration and recipe (default)\n"
        ));
        assert!(output.contains("  help               display this help and exit\n"));

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
