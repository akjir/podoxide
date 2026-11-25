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

use std::env;
use std::fs::File;
use std::io::{self, BufRead, Read, Write};

use crate::core::log;
use crate::error::{PodError, SystemError};
use crate::utilities::string;

const ELEVATED_PROMPT: &str = "Are you sure you want to continue? Type 'yes' to proceed: ";
const ELEVATED_WARNING: &str = "Podoxide is running with elevated privileges (UID 0).";
/// Maximum bytes read from standard input for an interactive confirmation prompt.
pub const MAX_CONFIRMATION_BYTES: usize = 64;
const MAX_PROC_STATUS_BYTES: usize = 16 * 1024;
const PROC_SELF_STATUS_PATH: &str = "/proc/self/status";

/// Evaluates whether the host operating system is Linux.
#[must_use]
pub fn check_os() -> bool {
    env::consts::OS == "linux"
}

/// Prompts the user on `stderr_writer` to confirm execution when running with elevated privileges.
///
/// # Errors
///
/// Returns [`PodError::Io`] if writing or flushing the prompt or reading from `reader` fails, or
/// [`PodError::System`] with [`SystemError::AbortedExecution`] if the user does not enter `"yes"`.
pub fn confirm_elevated_execution<R, E>(
    reader: &mut R,
    stderr_writer: &mut E,
) -> Result<(), PodError>
where
    R: BufRead,
    E: Write,
{
    log::write_warning(stderr_writer, ELEVATED_WARNING);
    stderr_writer.write_all(ELEVATED_PROMPT.as_bytes())?;
    stderr_writer.flush()?;
    let Some(input) = read_bounded_confirmation_line(reader)? else {
        let _ = stderr_writer.write_all(b"\n");
        let _ = stderr_writer.flush();
        return Err(PodError::from(SystemError::AbortedExecution));
    };
    if !input.ends_with('\n') {
        let _ = stderr_writer.write_all(b"\n");
        let _ = stderr_writer.flush();
    }
    if input.trim_end_matches(['\r', '\n']) != "yes" {
        return Err(PodError::from(SystemError::AbortedExecution));
    }
    Ok(())
}

/// Reads at most [`MAX_CONFIRMATION_BYTES`] bytes of a single line from `reader`, returning `Ok(None)` on EOF or when the line exceeds the byte bound.
///
/// # Errors
///
/// Returns [`io::Error`] if reading from `reader` fails.
pub fn read_bounded_confirmation_line<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    let mut buffer = String::with_capacity(MAX_CONFIRMATION_BYTES);
    let mut bounded = reader.take((MAX_CONFIRMATION_BYTES + 1) as u64);
    let bytes_read = bounded.read_line(&mut buffer)?;
    if bytes_read == 0 || buffer.len() > MAX_CONFIRMATION_BYTES {
        return Ok(None);
    }
    Ok(Some(buffer))
}

/// Evaluates whether the current process is running with effective root privileges (`UID 0`), failing closed when `/proc/self/status` cannot be verified.
#[must_use]
pub fn runs_elevated() -> bool {
    let Some(status_content) = read_proc_status(PROC_SELF_STATUS_PATH) else {
        return true;
    };
    parse_effective_uid_is_root(&status_content)
}

/// Parses `/proc/self/status` text and returns `true` when the effective UID (second `Uid:` field) is `0`, failing closed on missing or malformed input.
#[must_use]
fn parse_effective_uid_is_root(status_content: &str) -> bool {
    for line in status_content.lines() {
        let Some(values) = line.strip_prefix("Uid:") else {
            continue;
        };
        let mut fields = values.split_whitespace();
        let (Some(real_uid), Some(effective_uid)) = (fields.next(), fields.next()) else {
            return true;
        };
        if string::parse_ascii_u32(real_uid).is_none() {
            return true;
        }
        return match string::parse_ascii_u32(effective_uid) {
            Some(uid) => uid == 0,
            None => true,
        };
    }
    true
}

/// Reads at most [`MAX_PROC_STATUS_BYTES`] UTF-8 bytes from `path`, returning `None` on I/O failure, overflow, or invalid UTF-8.
fn read_proc_status(path: &str) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(2048);
    let limit = (MAX_PROC_STATUS_BYTES + 1) as u64;
    file.take(limit).read_to_end(&mut bytes).ok()?;
    if bytes.len() > MAX_PROC_STATUS_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{self, BufRead, Cursor, Read, Write};

    use crate::error::{PodError, SystemError};
    use super::{
        MAX_CONFIRMATION_BYTES, PROC_SELF_STATUS_PATH, check_os, confirm_elevated_execution,
        parse_effective_uid_is_root, read_bounded_confirmation_line, runs_elevated,
    };

    struct FailingFlushWriter;

    impl Write for FailingFlushWriter {
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "flush failed"))
        }

        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            Ok(buffer.len())
        }
    }

    struct FailingReader;

    impl BufRead for FailingReader {
        fn consume(&mut self, _amount: usize) {}

        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "read failed"))
        }
    }

    impl Read for FailingReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "read failed"))
        }
    }

    struct FailingWriteWriter;

    impl Write for FailingWriteWriter {
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }

        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "write failed"))
        }
    }

    #[test]
    fn check_os_and_proc_status_uid_parsing() {
        assert!(check_os());

        let proc_status = fs::read_to_string(PROC_SELF_STATUS_PATH).unwrap();
        assert_eq!(runs_elevated(), parse_effective_uid_is_root(&proc_status));

        assert!(parse_effective_uid_is_root(
            "Name:\tpods\nUid:\t0\t0\t0\t0\n"
        ));
        assert!(!parse_effective_uid_is_root(
            "Name:\tpods\nUid:\t1000\t1000\t1000\t1000\n"
        ));
        assert!(parse_effective_uid_is_root(
            "Name:\tpods\nUid:\t1000\t0\t0\t0\n"
        ));
        assert!(!parse_effective_uid_is_root(
            "Name:\tpods\nUid:\t0\t1000\t1000\t1000\n"
        ));

        for malformed in [
            "",
            "Name:\tpods\nGid:\t1000\t1000\t1000\t1000\n",
            "Uid:\n",
            "Uid:\t1000\n",
            "Uid:\t1000\tabc\t1000\t1000\n",
            "Uid:\tabc\t1000\t1000\t1000\n",
            "Uid:\t1000\t-1\t1000\t1000\n",
        ] {
            assert!(
                parse_effective_uid_is_root(malformed),
                "expected fail-closed true for malformed status {malformed:?}"
            );
        }
    }

    #[test]
    fn confirm_elevated_execution_writes_to_stderr_and_handles_inputs_and_errors() {
        let mut yes_input = Cursor::new(b"yes\n".to_vec());
        let mut stderr = Vec::new();
        assert!(confirm_elevated_execution(&mut yes_input, &mut stderr).is_ok());
        let stderr_text = String::from_utf8(stderr).unwrap();
        assert!(
            stderr_text.contains("WARNING: Podoxide is running with elevated privileges (UID 0).")
        );
        assert!(stderr_text.contains("Are you sure you want to continue? Type 'yes' to proceed: "));

        let mut crlf_yes = Cursor::new(b"yes\r\n".to_vec());
        let mut crlf_err = Vec::new();
        assert!(confirm_elevated_execution(&mut crlf_yes, &mut crlf_err).is_ok());

        for rejected in [&b"no\n"[..], &b"y\n"[..], &b"YES\n"[..], &b"\n"[..]] {
            let mut reader = Cursor::new(rejected.to_vec());
            let mut err = Vec::new();
            let result = confirm_elevated_execution(&mut reader, &mut err);
            assert!(
                matches!(result, Err(PodError::System(SystemError::AbortedExecution))),
                "expected AbortedExecution for {rejected:?}, got {result:?}"
            );
        }

        let mut eof_reader = Cursor::new(Vec::<u8>::new());
        let mut eof_err = Vec::new();
        let eof_result = confirm_elevated_execution(&mut eof_reader, &mut eof_err);
        assert!(matches!(
            eof_result,
            Err(PodError::System(SystemError::AbortedExecution))
        ));
        let eof_text = String::from_utf8(eof_err).unwrap();
        assert!(
            eof_text.ends_with("Type 'yes' to proceed: \n"),
            "EOF must emit trailing newline to avoid colliding with ERROR:, got {eof_text:?}"
        );

        let oversized = vec![b'a'; MAX_CONFIRMATION_BYTES + 1];
        let mut oversized_reader = Cursor::new(oversized);
        let mut oversized_err = Vec::new();
        assert!(matches!(
            confirm_elevated_execution(&mut oversized_reader, &mut oversized_err),
            Err(PodError::System(SystemError::AbortedExecution))
        ));

        let mut yes_for_write_err = Cursor::new(b"yes\n".to_vec());
        let mut broken_write = FailingWriteWriter;
        assert!(matches!(
            confirm_elevated_execution(&mut yes_for_write_err, &mut broken_write),
            Err(PodError::Io(ref error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));

        let mut yes_for_flush_err = Cursor::new(b"yes\n".to_vec());
        let mut broken_flush = FailingFlushWriter;
        assert!(matches!(
            confirm_elevated_execution(&mut yes_for_flush_err, &mut broken_flush),
            Err(PodError::Io(ref error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));

        let mut broken_reader = FailingReader;
        let mut read_err_out = Vec::new();
        assert!(matches!(
            confirm_elevated_execution(&mut broken_reader, &mut read_err_out),
            Err(PodError::Io(ref error)) if error.kind() == io::ErrorKind::BrokenPipe
        ));

        assert_eq!(
            read_bounded_confirmation_line(&mut Cursor::new(b"yes\n".to_vec())).unwrap(),
            Some(String::from("yes\n"))
        );
        assert_eq!(
            read_bounded_confirmation_line(&mut Cursor::new(Vec::<u8>::new())).unwrap(),
            None
        );
    }
}
