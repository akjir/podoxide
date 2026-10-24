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

//! Handler for `pods connect help`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;
use crate::utilities::format::format_version_header;

/// Formats and outputs the CLI help text for `pods connect` (`pods con`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header, `pods connect` usage synopsis, actions (`shell`, `help`),
///    options (`--config`, `--debug`, `--simulate`), and `<target>` resolution rules
///    (`<recipe>`, `<recipe>/<container>`, `<absolute_name>`) via [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_help(&mut io::stdout().lock());
    Ok(())
}

/// Writes the `ACTIONS:` section for `pods connect` to `writer`.
fn write_actions_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "ACTIONS:");
    log::write_print(
        writer,
        "  shell              open an interactive shell (default)",
    );
    log::write_print(writer, "  help               display this help");
}

/// Writes the `pods connect` help text to `writer`.
fn write_help<W: Write>(writer: &mut W) {
    log::write_print(writer, &format_version_header(Some("Connect Mode")));
    log::write_print(writer, "");
    log::write_print(
        writer,
        "Connect to a running container with an interactive shell.",
    );
    log::write_print(writer, "Usage: pods connect [OPTIONS] [<action>] <target>");
    log::write_print(writer, "");
    write_actions_section(writer);
    log::write_print(writer, "");
    write_options_section(writer);
    log::write_print(writer, "");
    write_target_section(writer);
}

/// Writes the `OPTIONS:` section for `pods connect` to `writer`.
fn write_options_section<W: Write>(writer: &mut W) {
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
}

/// Writes the `TARGET:` section for `pods connect` to `writer`.
fn write_target_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "TARGET:");
    log::write_print(
        writer,
        "  <recipe>             connect to the container (if the recipe has exactly 1 container)",
    );
    log::write_print(
        writer,
        "  <recipe>/<container> connect to a specific container (index, relative, absolute)",
    );
    log::write_print(
        writer,
        "  <absolute_name>      connect directly to an absolute container name",
    );
}

#[cfg(test)]
mod tests {
    use crate::core::context::Context;
    use crate::utilities::format::format_version_header;
    use super::{handle, write_help};

    #[test]
    fn write_help_and_handle_output_connect_usage() {
        let mut buffer = Vec::new();
        write_help(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(&format!(
            "{}\n",
            format_version_header(Some("Connect Mode"))
        )));
        assert!(output.contains("Connect to a running container with an interactive shell.\n"));
        assert!(output.contains("Usage: pods connect [OPTIONS] [<action>] <target>\n"));
        assert!(output.contains("ACTIONS:\n"));
        assert!(output.contains("  shell              open an interactive shell (default)\n"));
        assert!(output.contains("  help               display this help\n"));
        assert!(output.contains("OPTIONS:\n"));
        assert!(output.contains("  --config=NAME      use config with given name or path\n"));
        assert!(output.contains("  --debug            enable debug output\n"));
        assert!(output.contains(
            "  --simulate         preview generated commands without executing them\n"
        ));
        assert!(output.contains("TARGET:\n"));
        assert!(output.contains(
            "  <recipe>             connect to the container (if the recipe has exactly 1 container)\n"
        ));
        assert!(output.contains(
            "  <recipe>/<container> connect to a specific container (index, relative, absolute)\n"
        ));
        assert!(output.contains(
            "  <absolute_name>      connect directly to an absolute container name\n"
        ));

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
