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
use std::io::{self, BufRead, BufReader, Read, Write};
use std::str;

use crate::core::log;
use crate::error::{PodError, SystemError};
use crate::utilities::string;

const ELEVATED_PROMPT: &str = "Are you sure you want to continue? Type 'yes' to proceed: ";
const ELEVATED_WARNING: &str = "Podoxide is running with elevated privileges (UID 0).";
const INITIAL_PROC_LINE_CAPACITY: usize = 128;
/// Maximum bytes read from standard input for an interactive confirmation prompt.
pub const MAX_CONFIRMATION_BYTES: usize = 64;
const MAX_PROC_STATUS_BYTES: usize = 16 * 1024;
const MAX_PROC_STATUS_LINE_BYTES: usize = 4096;
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
/// Returns [`io::Error`] if reading from `reader` fails or if a bounded line is not valid UTF-8.
pub fn read_bounded_confirmation_line<R: BufRead>(reader: &mut R) -> io::Result<Option<String>> {
    let mut bytes = Vec::with_capacity(MAX_CONFIRMATION_BYTES + 1);
    let mut bounded = reader.take((MAX_CONFIRMATION_BYTES + 1) as u64);
    let bytes_read = bounded.read_until(b'\n', &mut bytes)?;
    if bytes_read == 0 || bytes.len() > MAX_CONFIRMATION_BYTES {
        return Ok(None);
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Evaluates whether the current process is running with effective root privileges (`UID 0`), failing closed when `/proc/self/status` cannot be verified.
#[must_use]
pub fn runs_elevated() -> bool {
    read_proc_status_is_root(PROC_SELF_STATUS_PATH)
}

/// Streams `/proc/self/status` bytes line-by-line from `reader` and returns `true` when the effective UID (second `Uid:` field) is `0`, failing closed on I/O, bound, or format errors.
#[must_use]
fn parse_effective_uid_is_root<R: BufRead>(mut reader: R) -> bool {
    let mut line = Vec::with_capacity(INITIAL_PROC_LINE_CAPACITY);
    let mut total_bytes = 0usize;
    loop {
        line.clear();
        let remaining = MAX_PROC_STATUS_BYTES.saturating_sub(total_bytes);
        let line_limit = remaining.min(MAX_PROC_STATUS_LINE_BYTES).saturating_add(1);
        let Ok(bytes_read) = reader
            .by_ref()
            .take(line_limit as u64)
            .read_until(b'\n', &mut line)
        else {
            return true;
        };
        if bytes_read == 0 {
            return true;
        }
        total_bytes = total_bytes.saturating_add(bytes_read);
        if line.len() > MAX_PROC_STATUS_LINE_BYTES || total_bytes > MAX_PROC_STATUS_BYTES {
            return true;
        }
        if let Some(values) = line.strip_prefix(b"Uid:") {
            return parse_uid_fields_is_root(values);
        }
    }
}

/// Parses raw ASCII `Uid:` field bytes and returns `true` when the effective UID (second field) is `0` or either of the first two fields is malformed.
#[must_use]
fn parse_uid_fields_is_root(values: &[u8]) -> bool {
    let mut fields = values
        .split(u8::is_ascii_whitespace)
        .filter(|field| !field.is_empty());
    let (Some(real_uid), Some(effective_uid)) = (fields.next(), fields.next()) else {
        return true;
    };
    let (Ok(real_str), Ok(effective_str)) =
        (str::from_utf8(real_uid), str::from_utf8(effective_uid))
    else {
        return true;
    };
    if string::parse_ascii_u32(real_str).is_none() {
        return true;
    }
    string::parse_ascii_u32(effective_str).is_none_or(|uid| uid == 0)
}

/// Opens `path` and streams it through [`parse_effective_uid_is_root`], failing closed when opening fails.
#[must_use]
fn read_proc_status_is_root(path: &str) -> bool {
    let Ok(file) = File::open(path) else {
        return true;
    };
    parse_effective_uid_is_root(BufReader::new(file))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{self, BufRead, Cursor, Read, Write};

    use crate::error::{PodError, SystemError};
    use crate::io::system::fs::TestTempDir;
    use super::{
        MAX_CONFIRMATION_BYTES, MAX_PROC_STATUS_BYTES, MAX_PROC_STATUS_LINE_BYTES,
        PROC_SELF_STATUS_PATH, check_os, confirm_elevated_execution, parse_effective_uid_is_root,
        read_bounded_confirmation_line, read_proc_status_is_root, runs_elevated,
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

        let proc_status = fs::read(PROC_SELF_STATUS_PATH).unwrap();
        assert_eq!(
            runs_elevated(),
            parse_effective_uid_is_root(proc_status.as_slice())
        );

        assert!(parse_effective_uid_is_root(
            &b"Name:\tpods\nUid:\t0\t0\t0\t0\n"[..]
        ));
        assert!(!parse_effective_uid_is_root(
            &b"Name:\tpods\nUid:\t1000\t1000\t1000\t1000\n"[..]
        ));
        assert!(parse_effective_uid_is_root(
            &b"Name:\tpods\nUid:\t1000\t0\t0\t0\n"[..]
        ));
        assert!(!parse_effective_uid_is_root(
            &b"Name:\tpods\nUid:\t0\t1000\t1000\t1000\n"[..]
        ));

        for malformed in [
            &b""[..],
            &b"Name:\tpods\nGid:\t1000\t1000\t1000\t1000\n"[..],
            &b"Uid:\n"[..],
            &b"Uid:\t1000\n"[..],
            &b"Uid:\t1000\tabc\t1000\t1000\n"[..],
            &b"Uid:\tabc\t1000\t1000\t1000\n"[..],
            &b"Uid:\t1000\t-1\t1000\t1000\n"[..],
            &b"Uid:\t1000\t\xff\t1000\t1000\n"[..],
        ] {
            assert!(
                parse_effective_uid_is_root(malformed),
                "expected fail-closed true for malformed status {malformed:?}"
            );
        }
        assert!(parse_effective_uid_is_root(FailingReader));
        assert!(read_proc_status_is_root(
            "/nonexistent/podoxide_proc_status"
        ));

        let oversized_line = format!(
            "Name:\t{}\nUid:\t1000\t1000\t1000\t1000\n",
            "x".repeat(MAX_PROC_STATUS_LINE_BYTES)
        );
        assert!(parse_effective_uid_is_root(oversized_line.as_bytes()));

        let oversized_prefix = format!(
            "{}Uid:\t1000\t1000\t1000\t1000\n",
            "Note:\tpadding\n".repeat((MAX_PROC_STATUS_BYTES / 14) + 2)
        );
        assert!(parse_effective_uid_is_root(oversized_prefix.as_bytes()));
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

        let mut straddling = "a".repeat(MAX_CONFIRMATION_BYTES);
        straddling.push_str("ä\n");
        assert_eq!(
            read_bounded_confirmation_line(&mut Cursor::new(straddling.as_bytes().to_vec()))
                .expect("oversized UTF-8 straddling byte 65 must return Ok(None)"),
            None
        );
        let mut straddling_err = Vec::new();
        let straddling_result = confirm_elevated_execution(
            &mut Cursor::new(straddling.into_bytes()),
            &mut straddling_err,
        );
        assert!(matches!(
            straddling_result,
            Err(PodError::System(SystemError::AbortedExecution))
        ));
        assert!(
            String::from_utf8(straddling_err)
                .unwrap()
                .ends_with("Type 'yes' to proceed: \n"),
            "oversized confirmation line must print trailing newline"
        );
    }

    #[test]
    fn proc_status_handles_large_groups_and_non_utf8_name_without_false_root() {
        let temp_dir = TestTempDir::new("env_proc_status");

        let large_groups_path = temp_dir.join("large_groups_status");
        let mut large_groups = String::from("Name:\tpods\nUid:\t1000\t1000\t1000\t1000\nGroups:\t");
        for group_id in 0..5000 {
            large_groups.push_str(&format!("{group_id} "));
        }
        large_groups.push('\n');
        assert!(large_groups.len() > 20 * 1024);
        fs::write(&large_groups_path, &large_groups).unwrap();
        assert!(
            !read_proc_status_is_root(large_groups_path.to_str().unwrap()),
            "large Groups: line after Uid: must not false-positive as root"
        );

        let non_utf8_path = temp_dir.join("non_utf8_name_status");
        let mut non_utf8_bytes = b"Name:\t".to_vec();
        non_utf8_bytes.extend_from_slice(&[0xFF, 0xFE, b'\n']);
        non_utf8_bytes.extend_from_slice(b"Uid:\t1000\t1000\t1000\t1000\n");
        fs::write(&non_utf8_path, &non_utf8_bytes).unwrap();
        assert!(
            !read_proc_status_is_root(non_utf8_path.to_str().unwrap()),
            "non-UTF-8 Name: on line 1 must not false-positive as root"
        );

        let invalid_short = vec![0xFF, 0xFE, b'\n'];
        assert!(matches!(
            read_bounded_confirmation_line(&mut Cursor::new(invalid_short)),
            Err(ref error) if error.kind() == io::ErrorKind::InvalidData
        ));
    }
}
