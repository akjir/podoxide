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

//! Structured output and diagnostic logging for Podoxide.

use std::cell::Cell;
use std::io::{self, Write};

use crate::error::PodError;
use crate::utilities::string::{clamp_utf8_prefix, sanitize_char};

/// Maximum payload bytes written per diagnostic or output line.
pub const MAX_LOG_MESSAGE_BYTES: usize = 4096;

thread_local! {
    /// Thread-local flag controlling whether `DEBUG:` messages are emitted.
    static DEBUG_ENABLED: Cell<bool> = const { Cell::new(false) };
}

/// Writes a debug diagnostic message (`DEBUG: <message>`) to standard error when debug mode is enabled.
pub fn debug(message: &str) {
    if is_debug_enabled() {
        write_line(&mut io::stderr().lock(), "DEBUG: ", message);
    }
}

/// Writes an informational diagnostic message (`INFO: <message>`) to standard error.
pub fn info(message: &str) {
    write_info(&mut io::stderr().lock(), message);
}

/// Returns `true` when verbose debug logging is enabled.
#[inline]
#[must_use]
pub fn is_debug_enabled() -> bool {
    DEBUG_ENABLED.get()
}

/// Writes a structured [`PodError`] (including any multi-line compiler diagnostic) to standard error.
pub fn pod_error(error: &PodError) {
    write_pod_error(&mut io::stderr().lock(), error);
}

/// Writes a standard output line (`<message>`) to standard output.
pub fn print(message: &str) {
    write_print(&mut io::stdout().lock(), message);
}

/// Enables or disables verbose debug logging.
pub fn set_debug_enabled(enabled: bool) {
    DEBUG_ENABLED.set(enabled);
}

/// Writes a debug diagnostic message (`DEBUG: <message>`) to `writer` when debug mode is enabled.
pub fn write_debug<W: Write>(writer: &mut W, message: &str) {
    if is_debug_enabled() {
        write_line(writer, "DEBUG: ", message);
    }
}

/// Writes an error diagnostic message (`ERROR: <message>`) to the provided output stream.
pub fn write_error<W: Write>(writer: &mut W, message: &str) {
    write_line(writer, "ERROR: ", message);
}

/// Writes an informational diagnostic message (`INFO: <message>`) to the provided output stream.
pub fn write_info<W: Write>(writer: &mut W, message: &str) {
    write_line(writer, "INFO: ", message);
}

/// Writes a structured [`PodError`] (including any multi-line compiler diagnostic) to `writer`.
pub fn write_pod_error<W: Write>(writer: &mut W, error: &PodError) {
    let rendered = error.to_string();
    if error.diagnostic().is_none() {
        write_error(writer, &rendered);
        return;
    }
    let mut lines = rendered.lines();
    if let Some(header) = lines.next() {
        write_line(writer, "ERROR: ", header);
    }
    for body_line in lines {
        write_line(writer, "", body_line);
    }
}

/// Writes a standard output line (`<message>`) to the provided output stream.
pub fn write_print<W: Write>(writer: &mut W, message: &str) {
    write_line(writer, "", message);
}

/// Writes a warning diagnostic message (`WARNING: <message>`) to the provided output stream.
pub fn write_warning<W: Write>(writer: &mut W, message: &str) {
    write_line(writer, "WARNING: ", message);
}

/// Formats a single sanitized output line with `prefix`, clamped to [`MAX_LOG_MESSAGE_BYTES`], and writes it to `writer`.
fn write_line<W: Write>(writer: &mut W, prefix: &str, message: &str) {
    let clamped = clamp_utf8_prefix(message, MAX_LOG_MESSAGE_BYTES);
    let mut line = String::with_capacity(prefix.len() + clamped.len() + 1);
    line.push_str(prefix);
    for character in clamped.chars() {
        line.push(sanitize_char(character));
    }
    line.push('\n');
    let _ = writer.write_all(line.as_bytes());
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::thread;

    use crate::error::{CliError, CliErrorKind, PodError, RecipeError, RecipeErrorKind};
    use super::{
        MAX_LOG_MESSAGE_BYTES, debug, info, is_debug_enabled, pod_error, print,
        set_debug_enabled, write_debug, write_error, write_info, write_pod_error, write_print,
        write_warning,
    };

    struct FailingWriter;

    impl Write for FailingWriter {
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
        }

        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
        }
    }

    #[test]
    fn red_team_control_escape_bidi_and_oversized_utf8_bounds() {
        let mut buffer = Vec::new();
        write_error(
            &mut buffer,
            "bad\nINFO: Forged line\rWARNING: Overwritten\x1b[31mANSI\0null\u{009b}CSI\u{202e}RLO",
        );
        let output = String::from_utf8(buffer).unwrap();
        assert_eq!(output.lines().count(), 1);
        assert!(output.starts_with("ERROR: "));
        assert!(!output.contains('\r'));
        assert!(!output.contains('\x1b'));
        assert!(!output.contains('\0'));
        assert!(!output.contains('\u{009b}'));
        assert!(!output.contains('\u{202e}'));

        let multi_byte = "ä".repeat((MAX_LOG_MESSAGE_BYTES / 2) + 50);
        let mut oversized_buffer = Vec::new();
        write_print(&mut oversized_buffer, &format!("x{multi_byte}"));
        let oversized_output = String::from_utf8(oversized_buffer).unwrap();
        let trimmed = oversized_output.trim_end_matches('\n');
        assert!(trimmed.len() <= MAX_LOG_MESSAGE_BYTES);
        assert!(trimmed.ends_with('ä'));

        let mut failing = FailingWriter;
        write_error(&mut failing, "graceful broken pipe handling");
        write_info(&mut failing, "graceful broken pipe handling");
        write_print(&mut failing, "graceful broken pipe handling");
        write_warning(&mut failing, "graceful broken pipe handling");
    }

    #[test]
    fn thread_local_debug_state_isolation() {
        set_debug_enabled(false);
        assert!(!is_debug_enabled());

        let spawned_state = thread::spawn(|| {
            assert!(!is_debug_enabled());
            set_debug_enabled(true);
            is_debug_enabled()
        })
        .join()
        .unwrap();

        assert!(spawned_state);
        assert!(!is_debug_enabled());

        set_debug_enabled(true);
        assert!(is_debug_enabled());

        let child_inherits_false = thread::spawn(is_debug_enabled).join().unwrap();
        assert!(!child_inherits_false);

        set_debug_enabled(false);
        assert!(!is_debug_enabled());
    }

    #[test]
    fn write_and_stderr_logging_levels_and_debug_toggle() {
        let mut buffer = Vec::new();

        set_debug_enabled(false);
        assert!(!is_debug_enabled());
        write_debug(&mut buffer, "Suppressed debug message.");
        debug("suppressed stderr debug output");
        assert!(buffer.is_empty());

        set_debug_enabled(true);
        assert!(is_debug_enabled());
        write_debug(&mut buffer, "Debug mode is enabled.");
        write_info(&mut buffer, "Simulate mode is active.");
        write_warning(
            &mut buffer,
            "Podoxide is running with elevated privileges (sudo).",
        );
        write_error(&mut buffer, "Podman 5.8.0 or higher is required.");
        write_print(&mut buffer, "Podoxide v0.1.0");

        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("DEBUG: Debug mode is enabled.\n"));
        assert!(!output.contains("Suppressed debug message."));
        assert!(output.contains("INFO: Simulate mode is active.\n"));
        assert!(output.contains("WARNING: Podoxide is running with elevated privileges (sudo).\n"));
        assert!(output.contains("ERROR: Podman 5.8.0 or higher is required.\n"));
        assert!(output.contains("Podoxide v0.1.0\n"));

        let mut diagnostic_buffer = Vec::new();
        let recipe_error = RecipeError::with_token(
            RecipeErrorKind::InvalidVolume,
            28,
            "/var/lib/db/data:Z",
        )
        .with_diagnostic(
            &format!(
                "{}    volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'\n",
                "\n".repeat(27)
            ),
            "./recipe.pod",
        );
        let pod_error_value = PodError::from(recipe_error);
        write_pod_error(&mut diagnostic_buffer, &pod_error_value);
        let diagnostic_output = String::from_utf8(diagnostic_buffer).unwrap();
        assert!(diagnostic_output.starts_with(
            "ERROR: Recipe error: line 28: invalid volume mount in recipe: '/var/lib/db/data:Z'\n"
        ));
        assert!(diagnostic_output.contains(" --> ./recipe.pod:28:19\n"));
        assert!(diagnostic_output.contains("28 |     volume   data /var/lib/db/data:Z"));
        assert!(diagnostic_output.contains("   |                   ^^^^^^^^^^^^^^^^^^"));
        assert!(diagnostic_output.contains("= help: pass volume options"));

        let mut plain_buffer = Vec::new();
        let plain_error =
            PodError::from(CliError::with_argument(CliErrorKind::UnknownFlag, "--bad"));
        write_pod_error(&mut plain_buffer, &plain_error);
        let plain_output = String::from_utf8(plain_buffer).unwrap();
        assert_eq!(plain_output.lines().count(), 1);
        assert!(plain_output.starts_with("ERROR: CLI error:"));

        debug("test stderr debug output");
        info("test stderr info output");
        pod_error(&plain_error);
        print("test stdout print output");

        set_debug_enabled(false);
        assert!(!is_debug_enabled());
    }
}
