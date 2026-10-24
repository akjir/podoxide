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

//! Handler for `pods command help`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;

/// Formats and outputs the CLI help text for `pods command` (`pods cmd`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header, `pods command` usage synopsis, available actions (`exec`,
///    `list`, `help`), options (`--config`, `--debug`, `--simulate`), and `<recipe>` /
///    `<command|index>` argument descriptions via [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_help(&mut io::stdout().lock());
    Ok(())
}

/// Writes the `ACTIONS:` section for `pods command` to `writer`.
fn write_actions_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "ACTIONS:");
    log::write_print(
        writer,
        "  exec               execute a command defined in a recipe (default when COMMAND is provided)",
    );
    log::write_print(
        writer,
        "  list               list all valid commands for a recipe (default)",
    );
    log::write_print(writer, "  help               display this help text");
}

/// Writes the `pods command` help text to `writer`.
fn write_help<W: Write>(writer: &mut W) {
    log::write_print(
        writer,
        concat!("Podoxide v", env!("CARGO_PKG_VERSION"), " - Command Mode"),
    );
    log::write_print(writer, "");
    log::write_print(
        writer,
        "Execute a command defined in a recipe for a container.",
    );
    log::write_print(
        writer,
        "Usage: pods command [OPTIONS] [ACTION] RECIPE [COMMAND|INDEX]",
    );
    log::write_print(writer, "");
    write_actions_section(writer);
    log::write_print(writer, "");
    write_options_section(writer);
    log::write_print(writer, "");
    log::write_print(writer, "RECIPE:");
    log::write_print(writer, "  *                  name of the recipe");
    log::write_print(writer, "");
    log::write_print(writer, "COMMAND|INDEX:");
    log::write_print(
        writer,
        "  *                  command by name defined in recipe to execute",
    );
    log::write_print(
        writer,
        "  <number>           command by numeric index defined in recipe to execute",
    );
}

/// Writes the `OPTIONS:` section for `pods command` to `writer`.
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

#[cfg(test)]
mod tests {
    use crate::core::context::Context;
    use super::{handle, write_help};

    #[test]
    fn write_help_and_handle_output_command_usage() {
        let mut buffer = Vec::new();
        write_help(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(concat!(
            "Podoxide v",
            env!("CARGO_PKG_VERSION"),
            " - Command Mode\n"
        )));
        assert!(output.contains("Execute a command defined in a recipe for a container.\n"));
        assert!(output.contains("Usage: pods command [OPTIONS] [ACTION] RECIPE [COMMAND|INDEX]\n"));
        assert!(output.contains("ACTIONS:\n"));
        assert!(output.contains(
            "  exec               execute a command defined in a recipe (default when COMMAND is provided)\n"
        ));
        assert!(output.contains(
            "  list               list all valid commands for a recipe (default)\n"
        ));
        assert!(output.contains("  help               display this help text\n"));
        assert!(output.contains("OPTIONS:\n"));
        assert!(output.contains("  --config=NAME      use config with given name or path\n"));
        assert!(output.contains("  --debug            enable debug output\n"));
        assert!(output.contains(
            "  --simulate         preview generated commands without executing them\n"
        ));
        assert!(output.contains("RECIPE:\n"));
        assert!(output.contains("  *                  name of the recipe\n"));
        assert!(output.contains("COMMAND|INDEX:\n"));
        assert!(output.contains(
            "  *                  command by name defined in recipe to execute\n"
        ));
        assert!(output.contains(
            "  <number>           command by numeric index defined in recipe to execute\n"
        ));

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
