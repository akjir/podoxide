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

use crate::core::constants::{APP_NAME, APP_VERSION};
use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;
use crate::utilities::format;

/// Formats and outputs the CLI help text for `pods config` (`pods cfg`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the mode version header, description, alias, `pods config` usage synopsis,
///    actions (`edit`, `help`, `inspect`, `show`), and options (`--config`, `--debug`,
///    `--simulate`) via [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_help(&mut io::stdout().lock());
    Ok(())
}

/// Writes the `pods config` help text to `writer`.
pub(crate) fn write_help<W: Write>(writer: &mut W) {
    log::write_print(
        writer,
        &format::format_version_header(APP_NAME, APP_VERSION, Some("Config Mode")),
    );
    log::write_print(writer, "");
    log::write_print(
        writer,
        "Inspect, display, and edit the active configuration file.",
    );
    log::write_print(writer, "Alias: cfg");
    log::write_print(writer, "Usage: pods config [OPTIONS] [ACTION]");
    log::write_print(writer, "");
    write_actions_section(writer);
    log::write_print(writer, "");
    write_options_section(writer);
}

/// Writes the `ACTIONS:` section for `pods config` to `writer`.
fn write_actions_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "ACTIONS:");
    log::write_print(
        writer,
        "  edit               open the configuration file in the configured editor",
    );
    log::write_print(writer, "  help               display this help and exit");
    log::write_print(
        writer,
        "  inspect            validate and display settings, directories, and groups (default)",
    );
    log::write_print(
        writer,
        "  show               display the raw configuration file with line numbers",
    );
}

/// Writes the `OPTIONS:` section for `pods config` to `writer`.
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
    use crate::core::constants::{APP_NAME, APP_VERSION};
    use crate::core::context::Context;
    use crate::utilities::format;
    use super::{handle, write_help};

    #[test]
    fn write_help_and_handle_output_config_usage() {
        let mut buffer = Vec::new();
        write_help(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(&format!(
            "{}\n",
            format::format_version_header(APP_NAME, APP_VERSION, Some("Config Mode"))
        )));
        assert!(output.contains("Inspect, display, and edit the active configuration file.\n"));
        assert!(output.contains("Alias: cfg\n"));
        assert!(output.contains("Usage: pods config [OPTIONS] [ACTION]\n"));
        assert!(output.contains("ACTIONS:\n"));
        assert!(output.contains(
            "  edit               open the configuration file in the configured editor\n"
        ));
        assert!(output.contains("  help               display this help and exit\n"));
        assert!(output.contains(
            "  inspect            validate and display settings, directories, and groups (default)\n"
        ));
        assert!(output.contains(
            "  show               display the raw configuration file with line numbers\n"
        ));
        assert!(output.find("ACTIONS:").unwrap() < output.find("OPTIONS:").unwrap());
        assert!(output.contains("OPTIONS:\n"));
        assert!(output.contains("  --config=NAME      use config with given name or path\n"));
        assert!(output.contains("  --debug            enable debug output\n"));
        assert!(
            output.contains(
                "  --simulate         preview generated commands without executing them\n"
            )
        );

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
