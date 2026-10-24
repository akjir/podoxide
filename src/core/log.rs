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

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::utilities::string::{clamp_utf8_prefix, sanitize_char};

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
/// Maximum payload bytes written per diagnostic or output line.
pub const MAX_LOG_MESSAGE_BYTES: usize = 4096;

/// Writes a debug diagnostic message (`DEBUG: <message>`) to standard error when debug mode is enabled.
pub fn debug(message: &str) {
    if is_debug_enabled() {
        write_debug(&mut io::stderr().lock(), message);
    }
}

/// Writes an error diagnostic message (`ERROR: <message>`) to standard error.
pub fn error(message: &str) {
    write_error(&mut io::stderr().lock(), message);
}

/// Writes an informational diagnostic message (`INFO: <message>`) to standard error.
pub fn info(message: &str) {
    write_info(&mut io::stderr().lock(), message);
}

/// Returns `true` when verbose debug logging is enabled.
#[inline]
#[must_use]
pub fn is_debug_enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Acquire)
}

/// Writes a standard output line (`<message>`) to standard output.
pub fn print(message: &str) {
    write_print(&mut io::stdout().lock(), message);
}

/// Enables or disables verbose debug logging.
pub fn set_debug_enabled(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Release);
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

    use super::{
        MAX_LOG_MESSAGE_BYTES, debug, error, info, is_debug_enabled, print, set_debug_enabled,
        write_debug, write_error, write_info, write_print, write_warning,
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

        debug("test stderr debug output");
        info("test stderr info output");
        error("test stderr error output");
        print("test stdout print output");

        set_debug_enabled(false);
        assert!(!is_debug_enabled());
    }
}
