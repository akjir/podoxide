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

//! Handler for `pods recipe help`.

use std::io::{self, Write};

use crate::core::constants::{APP_NAME, APP_VERSION};
use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;
use crate::utilities::format;

/// Formats and outputs the CLI help text for `pods recipe` (`pods rec`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the mode version header, description, alias, `pods recipe` usage synopsis,
///    actions (`edit`, `help`, `inspect`, `list`, `show`), options (`--all`, `--config`,
///    `--debug`, `--simulate`), and `<recipe>` argument description via [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_help(&mut io::stdout().lock());
    Ok(())
}

/// Writes the `pods recipe` help text to `writer`.
pub(crate) fn write_help<W: Write>(writer: &mut W) {
    log::write_print(
        writer,
        &format::format_version_header(APP_NAME, APP_VERSION, Some("Recipe Mode")),
    );
    log::write_print(writer, "");
    log::write_print(writer, "List, inspect, display, and edit recipe files.");
    log::write_print(writer, "Alias: rec");
    log::write_print(writer, "Usage: pods recipe [OPTIONS] [ACTION] [NAME]");
    log::write_print(writer, "");
    write_actions_section(writer);
    log::write_print(writer, "");
    write_options_section(writer);
    log::write_print(writer, "");
    log::write_print(writer, "NAME:");
    log::write_print(writer, "  *                  name of the recipe");
}

/// Writes the `ACTIONS:` section for `pods recipe` to `writer`.
fn write_actions_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "ACTIONS:");
    log::write_print(
        writer,
        "  edit               open a recipe file in the configured editor",
    );
    log::write_print(writer, "  help               display this help and exit");
    log::write_print(
        writer,
        "  inspect            validate a recipe file and report diagnostics",
    );
    log::write_print(
        writer,
        "  list               list configured recipes and status (default)",
    );
    log::write_print(
        writer,
        "  show               display a recipe file with line numbers (default when NAME is provided)",
    );
}

/// Writes the `OPTIONS:` section for `pods recipe` to `writer`.
fn write_options_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "OPTIONS:");
    log::write_print(
        writer,
        "  --all              include unconfigured recipes found on disk (list only)",
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

#[cfg(test)]
mod tests {
    use crate::core::constants::{APP_NAME, APP_VERSION};
    use crate::core::context::Context;
    use crate::utilities::format;
    use super::{handle, write_help};

    #[test]
    fn write_help_and_handle_output_recipe_usage() {
        let mut buffer = Vec::new();
        write_help(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(&format!(
            "{}\n",
            format::format_version_header(APP_NAME, APP_VERSION, Some("Recipe Mode"))
        )));
        assert!(output.contains("List, inspect, display, and edit recipe files.\n"));
        assert!(output.contains("Alias: rec\n"));
        assert!(output.contains("Usage: pods recipe [OPTIONS] [ACTION] [NAME]\n"));
        assert!(output.contains("ACTIONS:\n"));
        assert!(
            output.contains("  edit               open a recipe file in the configured editor\n")
        );
        assert!(output.contains("  help               display this help and exit\n"));
        assert!(
            output.contains("  inspect            validate a recipe file and report diagnostics\n")
        );
        assert!(
            output.contains("  list               list configured recipes and status (default)\n")
        );
        assert!(output.contains(
            "  show               display a recipe file with line numbers (default when NAME is provided)\n"
        ));
        assert!(output.find("ACTIONS:").unwrap() < output.find("OPTIONS:").unwrap());
        assert!(output.contains("OPTIONS:\n"));
        assert!(output.contains(
            "  --all              include unconfigured recipes found on disk (list only)\n"
        ));
        assert!(output.contains("  --config=NAME      use config with given name or path\n"));
        assert!(output.contains("  --debug            enable debug output\n"));
        assert!(
            output.contains(
                "  --simulate         preview generated commands without executing them\n"
            )
        );
        assert!(output.contains("NAME:\n"));
        assert!(output.contains("  *                  name of the recipe\n"));

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
