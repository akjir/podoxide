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

//! Handler for `pods logs help`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;
use crate::utilities::format::format_version_header;

/// Formats and outputs the CLI help text for `pods logs` (`pods log`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header, `pods logs` usage synopsis, actions (`show`, `follow`,
///    `help`), options (`--tail`, `--since`, `--until`, `--timestamps`, `--config`,
///    `--debug`, `--simulate`), and `<recipe>[/<container>]` target syntax via
///    [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_help(&mut io::stdout().lock());
    Ok(())
}

/// Writes the `ACTIONS:` section for `pods logs` to `writer`.
fn write_actions_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "ACTIONS:");
    log::write_print(
        writer,
        "  show               fetch and display logs, then exit (default)",
    );
    log::write_print(writer, "  follow             fetch and follow logs");
    log::write_print(writer, "  help               display this help");
}

/// Writes the `pods logs` help text to `writer`.
fn write_help<W: Write>(writer: &mut W) {
    log::write_print(writer, &format_version_header(Some("Logs Mode")));
    log::write_print(writer, "");
    log::write_print(
        writer,
        "Show or follow logs for a recipe's pod or container.",
    );
    log::write_print(
        writer,
        "Usage: pods logs [OPTIONS] [<action>] <recipe>[/container]",
    );
    log::write_print(writer, "");
    write_actions_section(writer);
    log::write_print(writer, "");
    write_options_section(writer);
    log::write_print(writer, "");
    write_target_section(writer);
}

/// Writes the `OPTIONS:` section for `pods logs` to `writer`.
fn write_options_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "OPTIONS:");
    log::write_print(
        writer,
        "  --tail=<n>         output the specified number of lines at the end",
    );
    log::write_print(writer, "  --since=<time>     show logs since timestamp");
    log::write_print(writer, "  --until=<time>     show logs until timestamp");
    log::write_print(
        writer,
        "  --timestamps       show timestamps in the log output",
    );
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

/// Writes the `TARGET:` section for `pods logs` to `writer`.
fn write_target_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "TARGET:");
    log::write_print(
        writer,
        "  <recipe>             show logs for all containers in the recipe's pod",
    );
    log::write_print(
        writer,
        "  <recipe>/<container> filter logs to a specific container (index, relative, absolute)",
    );
}

#[cfg(test)]
mod tests {
    use crate::core::context::Context;
    use crate::utilities::format::format_version_header;
    use super::{handle, write_help};

    #[test]
    fn write_help_and_handle_output_logs_usage() {
        let mut buffer = Vec::new();
        write_help(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(&format!(
            "{}\n",
            format_version_header(Some("Logs Mode"))
        )));
        assert!(output.contains("Show or follow logs for a recipe's pod or container.\n"));
        assert!(output.contains("Usage: pods logs [OPTIONS] [<action>] <recipe>[/container]\n"));
        assert!(output.contains("ACTIONS:\n"));
        assert!(output.contains(
            "  show               fetch and display logs, then exit (default)\n"
        ));
        assert!(output.contains("  follow             fetch and follow logs\n"));
        assert!(output.contains("  help               display this help\n"));
        assert!(output.contains("OPTIONS:\n"));
        assert!(output.contains(
            "  --tail=<n>         output the specified number of lines at the end\n"
        ));
        assert!(output.contains("  --since=<time>     show logs since timestamp\n"));
        assert!(output.contains("  --until=<time>     show logs until timestamp\n"));
        assert!(output.contains(
            "  --timestamps       show timestamps in the log output\n"
        ));
        assert!(output.contains("  --config=NAME      use config with given name or path\n"));
        assert!(output.contains("  --debug            enable debug output\n"));
        assert!(output.contains(
            "  --simulate         preview generated commands without executing them\n"
        ));
        assert!(output.contains("TARGET:\n"));
        assert!(output.contains(
            "  <recipe>             show logs for all containers in the recipe's pod\n"
        ));
        assert!(output.contains(
            "  <recipe>/<container> filter logs to a specific container (index, relative, absolute)\n"
        ));

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
