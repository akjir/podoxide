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

use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;
use crate::utilities::format::format_version_header;

/// Formats and outputs the CLI help text for `pods recipe` (`pods rec`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header, `pods recipe` usage synopsis, options (`--all`, `--config`,
///    `--debug`, `--simulate`), actions (`help`, `edit`, `inspect`, `list`, `show`), and `<recipe>`
///    argument description via [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_help(&mut io::stdout().lock());
    Ok(())
}

/// Writes the `ACTIONS:` section for `pods recipe` to `writer`.
fn write_actions_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "ACTIONS:");
    log::write_print(writer, "  help               display this help and exit");
    log::write_print(writer, "  edit               edit recipe");
    log::write_print(writer, "  inspect            inspect recipe");
    log::write_print(writer, "  list               list all recipes");
    log::write_print(writer, "  show               show recipe");
}

/// Writes the `pods recipe` help text to `writer`.
fn write_help<W: Write>(writer: &mut W) {
    log::write_print(writer, &format_version_header(None));
    log::write_print(writer, "");
    log::write_print(writer, "Usage: pods recipe [OPTIONS] ACTION NAME");
    log::write_print(writer, "");
    write_options_section(writer);
    log::write_print(writer, "");
    write_actions_section(writer);
    log::write_print(writer, "");
    log::write_print(writer, "NAME:");
    log::write_print(writer, "  *                  name of the recipe");
}

/// Writes the `OPTIONS:` section for `pods recipe` to `writer`.
fn write_options_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "OPTIONS:");
    log::write_print(
        writer,
        "  --all              include unconfigured recipes found on disk",
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
    use crate::core::context::Context;
    use crate::utilities::format::format_version_header;
    use super::{handle, write_help};

    #[test]
    fn write_help_and_handle_output_recipe_usage() {
        let mut buffer = Vec::new();
        write_help(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(&format!("{}\n", format_version_header(None))));
        assert!(output.contains("Usage: pods recipe [OPTIONS] ACTION NAME\n"));
        assert!(output.contains("OPTIONS:\n"));
        assert!(
            output.contains("  --all              include unconfigured recipes found on disk\n")
        );
        assert!(output.contains("  --config=NAME      use config with given name or path\n"));
        assert!(output.contains("  --debug            enable debug output\n"));
        assert!(output.contains(
            "  --simulate         preview generated commands without executing them\n"
        ));
        assert!(output.contains("ACTIONS:\n"));
        assert!(output.contains("  help               display this help and exit\n"));
        assert!(output.contains("  edit               edit recipe\n"));
        assert!(output.contains("  inspect            inspect recipe\n"));
        assert!(output.contains("  list               list all recipes\n"));
        assert!(output.contains("  show               show recipe\n"));
        assert!(output.contains("NAME:\n"));
        assert!(output.contains("  *                  name of the recipe\n"));

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
