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

//! Handler for `pods help` (global CLI help).

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;

/// Formats and outputs the global CLI help text for `pods` (default mode).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. Output the version header, global usage synopsis (`pods [MODE] [OPTIONS] ACTION [TARGETS]`),
///    available modes and 3-letter aliases, global options, default mode lifecycle and status
///    actions, and target syntax via [`log::write_print`].
///
/// # Errors
///
/// Returns `Ok(())` after writing the help output.
pub fn handle(_context: &Context) -> Result<(), PodError> {
    write_help(&mut io::stdout().lock());
    Ok(())
}

/// Writes the default mode `ACTIONS:` section to `writer`.
fn write_actions_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "ACTIONS:");
    log::write_print(
        writer,
        "  create             create a new pod without starting it",
    );
    log::write_print(writer, "  down               stop and remove a running pod");
    log::write_print(writer, "  help               display this help and exit");
    log::write_print(writer, "  reload             remove and recreate a pod");
    log::write_print(writer, "  remove             remove a pod");
    log::write_print(
        writer,
        "  restart            stop and start an existing pod",
    );
    log::write_print(
        writer,
        "  start              start an existing stopped pod",
    );
    log::write_print(
        writer,
        "  status             display status of pods and containers",
    );
    log::write_print(writer, "  stop               stop a running pod");
    log::write_print(writer, "  up                 create and start a new pod");
    log::write_print(
        writer,
        "  update             update all defined images of the pod",
    );
}

/// Writes the global default mode help text to `writer`.
fn write_help<W: Write>(writer: &mut W) {
    log::write_print(writer, concat!("Podoxide v", env!("CARGO_PKG_VERSION")));
    log::write_print(writer, "");
    log::write_print(writer, "Usage: pods [MODE] [OPTIONS] ACTION [TARGETS]");
    log::write_print(writer, "");
    write_modes_section(writer);
    log::write_print(writer, "");
    write_options_section(writer);
    log::write_print(writer, "");
    log::write_print(writer, "Valid in default mode only:");
    log::write_print(writer, "");
    write_actions_section(writer);
    log::write_print(writer, "");
    log::write_print(writer, "TARGETS:");
    log::write_print(
        writer,
        "  *                  names of recipes or groups defined in a config",
    );
    log::write_print(writer, "");
    log::write_print(writer, "For more: pods [MODE] help");
}

/// Writes the `MODES:` section with 3-letter aliases to `writer`.
fn write_modes_section<W: Write>(writer: &mut W) {
    log::write_print(writer, "MODES:");
    log::write_print(writer, "  *                  default mode");
    log::write_print(
        writer,
        "  command (cmd)      execute a command defined in a recipe",
    );
    log::write_print(
        writer,
        "  config (cfg)       manage and inspect configuration",
    );
    log::write_print(
        writer,
        "  connect (con)      connect to a running container with an interactive shell",
    );
    log::write_print(
        writer,
        "  image (img)        manage orphaned and dangling container images",
    );
    log::write_print(
        writer,
        "  init               initialize default configuration and recipe",
    );
    log::write_print(
        writer,
        "  logs (log)         show or follow logs for a pod or container",
    );
    log::write_print(writer, "  recipe (rec)       inspect and edit recipes");
}

/// Writes the `OPTIONS:` section to `writer`.
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
    log::write_print(
        writer,
        "  --all              include all unmanaged podman containers (status only)",
    );
}

#[cfg(test)]
mod tests {
    use crate::core::context::Context;
    use super::{handle, write_help};

    #[test]
    fn write_help_and_handle_output_default_usage() {
        let mut buffer = Vec::new();
        write_help(&mut buffer);
        let output = String::from_utf8(buffer).unwrap();

        assert!(output.contains(concat!("Podoxide v", env!("CARGO_PKG_VERSION"))));
        assert!(output.contains("Usage: pods [MODE] [OPTIONS] ACTION [TARGETS]\n"));
        assert!(output.contains("MODES:\n"));
        assert!(output.contains("  *                  default mode\n"));
        assert!(output.contains("  command (cmd)      execute a command defined in a recipe\n"));
        assert!(output.contains("  config (cfg)       manage and inspect configuration\n"));
        assert!(output.contains(
            "  connect (con)      connect to a running container with an interactive shell\n"
        ));
        assert!(output.contains(
            "  image (img)        manage orphaned and dangling container images\n"
        ));
        assert!(output.contains(
            "  init               initialize default configuration and recipe\n"
        ));
        assert!(output.contains(
            "  logs (log)         show or follow logs for a pod or container\n"
        ));
        assert!(output.contains("  recipe (rec)       inspect and edit recipes\n"));
        assert!(output.contains("OPTIONS:\n"));
        assert!(output.contains("  --config=NAME      use config with given name or path\n"));
        assert!(output.contains("  --debug            enable debug output\n"));
        assert!(output.contains(
            "  --simulate         preview generated commands without executing them\n"
        ));
        assert!(output.contains(
            "  --all              include all unmanaged podman containers (status only)\n"
        ));
        assert!(output.contains("Valid in default mode only:\n"));
        assert!(output.contains("ACTIONS:\n"));
        assert!(output.contains(
            "  create             create a new pod without starting it\n"
        ));
        assert!(output.contains("  down               stop and remove a running pod\n"));
        assert!(output.contains("  help               display this help and exit\n"));
        assert!(output.contains("  reload             remove and recreate a pod\n"));
        assert!(output.contains("  remove             remove a pod\n"));
        assert!(output.contains("  restart            stop and start an existing pod\n"));
        assert!(output.contains("  start              start an existing stopped pod\n"));
        assert!(output.contains("  status             display status of pods and containers\n"));
        assert!(output.contains("  stop               stop a running pod\n"));
        assert!(output.contains("  up                 create and start a new pod\n"));
        assert!(output.contains("  update             update all defined images of the pod\n"));
        assert!(output.contains("TARGETS:\n"));
        assert!(output.contains(
            "  *                  names of recipes or groups defined in a config\n"
        ));
        assert!(output.contains("For more: pods [MODE] help\n"));

        let context = Context::new();
        assert!(handle(&context).is_ok());
    }
}
