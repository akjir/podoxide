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

//! Handler for `pods config help`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;

/// Formats and outputs the CLI help text for `pods config` (`pods cfg`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header, `pods config` usage synopsis, options (`--config`,
///    `--debug`, `--simulate`), and actions (`help`, `edit`, `show`) via [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_help(&mut io::stdout().lock());
    Ok(())
}

/// Writes the `pods config` help text to `writer`.
fn write_help<W: Write>(writer: &mut W) {
    log::write_print(writer, concat!("Podoxide v", env!("CARGO_PKG_VERSION")));
    log::write_print(writer, "");
    log::write_print(writer, "Usage: pods config [OPTIONS] ACTION");
    log::write_print(writer, "");
    log::write_print(writer, "OPTIONS:");
    log::write_print(
        writer,
        "  --config=NAME      use config with given name or path",
    );
    log::write_print(writer, "  --debug            enable debug output");
    log::write_print(
        writer,
        "  --simulate         preview generated commands without executing them",
    );
    log::write_print(writer, "");
    log::write_print(writer, "ACTIONS:");
    log::write_print(writer, "  help               display this help and exit");
    log::write_print(writer, "  edit               edit config");
    log::write_print(writer, "  show               show config");
}

#[cfg(test)]
mod tests {
    use crate::core::context::Context;
    use super::{handle, write_help};

    #[test]
    fn write_help_and_handle_output_config_usage() {
        let mut buffer = Vec::new();
        write_help(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(concat!("Podoxide v", env!("CARGO_PKG_VERSION"))));
        assert!(output.contains("Usage: pods config [OPTIONS] ACTION\n"));
        assert!(output.contains("OPTIONS:\n"));
        assert!(output.contains("  --config=NAME      use config with given name or path\n"));
        assert!(output.contains("  --debug            enable debug output\n"));
        assert!(output.contains(
            "  --simulate         preview generated commands without executing them\n"
        ));
        assert!(output.contains("ACTIONS:\n"));
        assert!(output.contains("  help               display this help and exit\n"));
        assert!(output.contains("  edit               edit config\n"));
        assert!(output.contains("  show               show config\n"));

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
