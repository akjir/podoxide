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

//! Handler for `pods image help`.

use std::io::{self, Write};

use crate::core::constants::{APP_NAME, APP_VERSION};
use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;
use crate::utilities::format;

/// Formats and outputs the CLI help text for `pods image` (`pods img`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the mode version header, description, alias, `pods image` usage synopsis,
///    actions (`help`, `prune`, `update`), options (`--all`, `--config`, `--debug`, `--force`,
///    `--preview`, `--simulate`), and target syntax via [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_help(&mut io::stdout().lock());
    Ok(())
}

/// Writes the `pods image` help text to `writer`.
pub(crate) fn write_help<W: Write>(writer: &mut W) {
    log::write_print(
        writer,
        &format::format_version_header(APP_NAME, APP_VERSION, Some("Image Mode")),
    );
    log::write_print(writer, "");
    log::write_print(
        writer,
        "Pull recipe images and prune dangling or unused container images.",
    );
    log::write_print(writer, "Alias: img");
    log::write_print(writer, "Usage: pods image [OPTIONS] [ACTION] [TARGETS]");
    log::write_print(writer, "");
    write_actions_section(writer);
    log::write_print(writer, "");
    write_options_section(writer);
    log::write_print(writer, "");
    write_targets_section(writer);
}

/// Writes the `ACTIONS:` section for `pods image` to `writer`.
fn write_actions_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "ACTIONS:");
    log::write_print(writer, "  help               display this help and exit");
    log::write_print(
        writer,
        "  prune              prune dangling or all unused images",
    );
    log::write_print(
        writer,
        "  update             pull latest images for recipes, @groups, or containers without reloading",
    );
}

/// Writes the `OPTIONS:` section for `pods image` to `writer`.
fn write_options_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "OPTIONS:");
    log::write_print(
        writer,
        "  --all              prune all unused images, not just dangling ones (prune only)",
    );
    log::write_print(
        writer,
        "  --config=NAME      use config with given name or path",
    );
    log::write_print(writer, "  --debug            enable debug output");
    log::write_print(
        writer,
        "  --force            skip confirmation prompt (prune only; invalid with --preview)",
    );
    log::write_print(
        writer,
        "  --preview          preview images and space that would be deleted (prune only; invalid with --force)",
    );
    log::write_print(
        writer,
        "  --simulate         preview generated commands without executing them",
    );
}

/// Writes the `TARGETS:` section for `pods image` to `writer`.
fn write_targets_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "TARGETS:");
    log::write_print(
        writer,
        "  *                  names of recipes, @groups, or <recipe>/<container> targets (update only)",
    );
}

#[cfg(test)]
mod tests {
    use crate::core::constants::{APP_NAME, APP_VERSION};
    use crate::core::context::Context;
    use crate::utilities::format;
    use super::{handle, write_help};

    #[test]
    fn write_help_and_handle_output_image_usage() {
        let mut buffer = Vec::new();
        write_help(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(&format!(
            "{}\n",
            format::format_version_header(APP_NAME, APP_VERSION, Some("Image Mode"))
        )));
        assert!(
            output.contains("Pull recipe images and prune dangling or unused container images.\n")
        );
        assert!(output.contains("Alias: img\n"));
        assert!(output.contains("Usage: pods image [OPTIONS] [ACTION] [TARGETS]\n"));
        assert!(output.contains("ACTIONS:\n"));
        assert!(output.contains("  help               display this help and exit\n"));
        assert!(output.contains("  prune              prune dangling or all unused images\n"));
        assert!(output.contains(
            "  update             pull latest images for recipes, @groups, or containers without reloading\n"
        ));
        assert!(output.find("ACTIONS:").unwrap() < output.find("OPTIONS:").unwrap());
        assert!(output.contains("OPTIONS:\n"));
        assert!(output.contains(
            "  --all              prune all unused images, not just dangling ones (prune only)\n"
        ));
        assert!(output.contains("  --config=NAME      use config with given name or path\n"));
        assert!(output.contains("  --debug            enable debug output\n"));
        assert!(output.contains(
            "  --force            skip confirmation prompt (prune only; invalid with --preview)\n"
        ));
        assert!(output.contains(
            "  --preview          preview images and space that would be deleted (prune only; invalid with --force)\n"
        ));
        assert!(
            output.contains(
                "  --simulate         preview generated commands without executing them\n"
            )
        );
        assert!(output.contains("TARGETS:\n"));
        assert!(output.contains(
            "  *                  names of recipes, @groups, or <recipe>/<container> targets (update only)\n"
        ));

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
