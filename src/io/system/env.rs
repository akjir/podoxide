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

//! Host operating system verification and privilege detection.

use std::io::{BufRead, Read, Write};

use crate::core::log;
use crate::error::SystemError;
use crate::io::exec::command;

const ELEVATED_PROMPT: &str = "Are you sure you want to continue? Type 'yes' to proceed: ";
const ELEVATED_WARNING: &str = "Podoxide is running with elevated privileges (sudo).";
/// Maximum bytes read from standard input for an interactive confirmation prompt.
pub const MAX_CONFIRMATION_BYTES: usize = 64;

/// Evaluates whether the host operating system is Linux via `uname -s`.
#[must_use]
pub fn check_os() -> bool {
    let Some(output) = command::exec_capture("uname", &["-s"]) else {
        return false;
    };
    is_supported_os(&output)
}

/// Prompts the user to confirm execution when running with elevated privileges.
///
/// # Errors
///
/// Returns [`SystemError::AbortedExecution`] if the user does not respond with `"yes"`
/// or if writing the prompt or reading confirmation fails.
pub fn confirm_elevated_execution<R, W, E>(
    reader: &mut R,
    stdout_writer: &mut W,
    stderr_writer: &mut E,
) -> Result<(), SystemError>
where
    R: BufRead,
    W: Write,
    E: Write,
{
    log::write_warning(stderr_writer, ELEVATED_WARNING);
    if stdout_writer.write_all(ELEVATED_PROMPT.as_bytes()).is_err()
        || stdout_writer.flush().is_err()
    {
        return Err(SystemError::AbortedExecution);
    }
    let Some(input) = read_bounded_line(reader) else {
        return Err(SystemError::AbortedExecution);
    };
    if input.trim_end_matches(['\r', '\n']) != "yes" {
        return Err(SystemError::AbortedExecution);
    }
    Ok(())
}

/// Evaluates whether a raw `id -u` output string indicates root (`UID 0`).
#[must_use]
pub fn is_root_uid(output: &str) -> bool {
    output.trim() == "0"
}

/// Evaluates whether a raw `uname -s` output string indicates Linux.
#[must_use]
pub fn is_supported_os(output: &str) -> bool {
    output.trim() == "Linux"
}

/// Evaluates whether the current process is running with root privileges (`UID 0`).
#[must_use]
pub fn runs_elevated() -> bool {
    let Some(output) = command::exec_capture("id", &["-u"]) else {
        return false;
    };
    is_root_uid(&output)
}

/// Reads at most [`MAX_CONFIRMATION_BYTES`] bytes of a single line from `reader`.
fn read_bounded_line<R: BufRead>(reader: &mut R) -> Option<String> {
    let mut buffer = String::with_capacity(MAX_CONFIRMATION_BYTES);
    let mut bounded = reader.take((MAX_CONFIRMATION_BYTES + 1) as u64);
    let bytes_read = bounded.read_line(&mut buffer).ok()?;
    if bytes_read == 0 || buffer.len() > MAX_CONFIRMATION_BYTES {
        return None;
    }
    Some(buffer)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::error::SystemError;
    use super::{
        MAX_CONFIRMATION_BYTES, check_os, confirm_elevated_execution, is_root_uid, is_supported_os,
        runs_elevated,
    };

    #[test]
    fn check_os_and_runs_elevated_and_parsers() {
        assert!(check_os());
        assert!(!runs_elevated());

        assert!(is_supported_os("Linux\n"));
        assert!(is_supported_os("  Linux  "));
        assert!(!is_supported_os("Darwin\n"));
        assert!(!is_supported_os("FreeBSD"));
        assert!(!is_supported_os(""));

        assert!(is_root_uid("0\n"));
        assert!(is_root_uid(" 0 "));
        assert!(!is_root_uid("1000\n"));
        assert!(!is_root_uid(""));
    }

    #[test]
    fn confirm_elevated_execution_accepts_yes_and_rejects_other_inputs() {
        let mut yes_input = Cursor::new(b"yes\n".to_vec());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert!(confirm_elevated_execution(&mut yes_input, &mut stdout, &mut stderr).is_ok());
        assert!(
            String::from_utf8(stdout)
                .unwrap()
                .contains("Are you sure you want to continue? Type 'yes' to proceed: ")
        );
        assert!(
            String::from_utf8(stderr)
                .unwrap()
                .contains("WARNING: Podoxide is running with elevated privileges (sudo).")
        );

        let mut crlf_yes = Cursor::new(b"yes\r\n".to_vec());
        let mut crlf_out = Vec::new();
        let mut crlf_err = Vec::new();
        assert!(confirm_elevated_execution(&mut crlf_yes, &mut crlf_out, &mut crlf_err).is_ok());

        for rejected in [&b"no\n"[..], &b"y\n"[..], &b"YES\n"[..], &b"\n"[..], &b""[..]] {
            let mut reader = Cursor::new(rejected.to_vec());
            let mut out = Vec::new();
            let mut err = Vec::new();
            assert_eq!(
                confirm_elevated_execution(&mut reader, &mut out, &mut err),
                Err(SystemError::AbortedExecution)
            );
        }

        let oversized = vec![b'a'; MAX_CONFIRMATION_BYTES + 1];
        let mut oversized_reader = Cursor::new(oversized);
        let mut oversized_out = Vec::new();
        let mut oversized_err = Vec::new();
        assert_eq!(
            confirm_elevated_execution(&mut oversized_reader, &mut oversized_out, &mut oversized_err),
            Err(SystemError::AbortedExecution)
        );
    }
}
