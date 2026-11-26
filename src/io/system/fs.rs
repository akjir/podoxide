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

//! Host filesystem operations and bounded file reading and writing.

#[cfg(test)]
use std::env;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, DirEntry, Metadata, OpenOptions};
use std::io::{self, Read, Write};
#[cfg(test)]
use std::ops::Deref;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process;
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::{PodError, PslError, PslErrorKind};
use crate::io::psl::MAX_FILE_BYTES;
use crate::utilities::path;
use super::path_resolve;

const INITIAL_DIRECTORY_CAPACITY: usize = 16;
const INITIAL_FILE_CAPACITY: usize = 256;
#[cfg(any(
    target_arch = "aarch64",
    target_arch = "arm",
    target_arch = "m68k",
    target_arch = "powerpc",
    target_arch = "powerpc64"
))]
const LINUX_O_NOFOLLOW: i32 = 0o100_000;
#[cfg(not(any(
    target_arch = "aarch64",
    target_arch = "arm",
    target_arch = "m68k",
    target_arch = "powerpc",
    target_arch = "powerpc64"
)))]
const LINUX_O_NOFOLLOW: i32 = 0o400_000;
#[cfg(any(
    target_arch = "mips",
    target_arch = "mips32r6",
    target_arch = "mips64",
    target_arch = "mips64r6"
))]
const LINUX_O_NONBLOCK: i32 = 0o200;
#[cfg(any(target_arch = "sparc", target_arch = "sparc64"))]
const LINUX_O_NONBLOCK: i32 = 0x4000;
#[cfg(not(any(
    target_arch = "mips",
    target_arch = "mips32r6",
    target_arch = "mips64",
    target_arch = "mips64r6",
    target_arch = "sparc",
    target_arch = "sparc64"
)))]
const LINUX_O_NONBLOCK: i32 = 0o4000;
/// Maximum number of entries collected from a single directory listing.
pub const MAX_DIRECTORY_ENTRIES: usize = 4096;
#[cfg(test)]
static TEMP_DIR_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// RAII temporary directory guard that removes its directory tree when dropped in tests.
#[cfg(test)]
pub(crate) struct TestTempDir {
    path: PathBuf,
}

#[cfg(test)]
impl TestTempDir {
    /// Creates a unique temporary directory under [`env::temp_dir`] prefixed with `label`.
    #[must_use]
    pub(crate) fn new(label: &str) -> Self {
        let sequence = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("podoxide_{label}_{}_{sequence}", process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create test temp directory");
        Self { path }
    }
}

#[cfg(test)]
impl AsRef<Path> for TestTempDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
impl Deref for TestTempDir {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

#[cfg(test)]
impl Drop for TestTempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// Wraps an [`io::Error`] with its filesystem path while preserving the [`Error::source`] chain.
#[derive(Debug)]
struct PathIoError {
    path: PathBuf,
    source: io::Error,
}

impl Display for PathIoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "'{}': {}", self.path.display(), self.source)
    }
}

impl Error for PathIoError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Evaluates whether a directory exists at `target_path`.
#[must_use]
pub fn directory_exists(target_path: &Path) -> bool {
    if target_path.as_os_str().is_empty() {
        return false;
    }
    let working_path = path_resolve::resolve_working_path(target_path);
    fs::metadata(working_path.as_ref()).is_ok_and(|metadata| metadata.is_dir())
}

/// Verifies that `target_path` either does not exist yet (within an existing parent directory) or is an
/// existing regular file (and not a symlink or directory).
///
/// # Errors
///
/// Returns [`PodError::Io`] if `target_path` is a symlink, directory, or special file, or if its parent
/// directory does not exist.
pub fn ensure_writable_target(target_path: &Path) -> Result<(), PodError> {
    if !path::has_regular_target_name(target_path) {
        return Err(invalid_target_error(target_path));
    }
    let working_path = path_resolve::resolve_working_path(target_path);
    let metadata = match fs::symlink_metadata(working_path.as_ref()) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return ensure_parent_directory(working_path.as_ref());
        }
        Err(error) => return Err(io_path_error(target_path, error)),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(invalid_target_error(target_path));
    }
    Ok(())
}

/// Evaluates whether a regular file exists at `target_path`.
#[must_use]
pub fn file_exists(target_path: &Path) -> bool {
    if target_path.as_os_str().is_empty() {
        return false;
    }
    let working_path = path_resolve::resolve_working_path(target_path);
    fs::metadata(working_path.as_ref()).is_ok_and(|metadata| metadata.is_file())
}

/// Resolves the normalized absolute path for `target_path` without requiring the target to exist on disk.
#[must_use]
pub fn get_absolute_path(target_path: &Path) -> PathBuf {
    path_resolve::get_absolute_path(target_path)
}

/// Lists regular file names (including symlinks resolving to regular files) in `target_path`, optionally filtered by `extension` (such as `Some(".pod")`),
/// sorted alphabetically.
///
/// # Errors
///
/// Returns [`PodError`] if `target_path` cannot be read or if the number of scanned or matching entries
/// exceeds [`MAX_DIRECTORY_ENTRIES`].
pub fn list_directory(
    target_path: &Path,
    extension: Option<&str>,
) -> Result<Vec<String>, PodError> {
    let working_path = path_resolve::resolve_working_path(target_path);
    let read_dir =
        fs::read_dir(working_path.as_ref()).map_err(|error| io_path_error(target_path, error))?;
    let mut entries = Vec::with_capacity(INITIAL_DIRECTORY_CAPACITY.min(MAX_DIRECTORY_ENTRIES));
    for (scanned_count, entry_result) in read_dir.enumerate() {
        if scanned_count >= MAX_DIRECTORY_ENTRIES {
            return Err(directory_limit_error());
        }
        collect_matching_entry(target_path, entry_result, extension, &mut entries)?;
    }
    entries.sort_unstable();
    Ok(entries)
}

/// Reads the entire UTF-8 text content of `target_path`, opening non-blocking to avoid FIFO hangs
/// and enforcing [`MAX_FILE_BYTES`] before and during allocation.
///
/// # Errors
///
/// Returns [`PodError::Io`] if opening, verifying, or reading `target_path` fails, or [`PodError::Psl`]
/// with [`PslErrorKind::LimitExceeded`] if the file size exceeds [`MAX_FILE_BYTES`].
pub fn read_file(target_path: &Path) -> Result<String, PodError> {
    let working_path = path_resolve::resolve_working_path(target_path);
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(LINUX_O_NONBLOCK)
        .open(working_path.as_ref())
        .map_err(|error| io_path_error(target_path, error))?;
    let metadata = file
        .metadata()
        .map_err(|error| io_path_error(target_path, error))?;
    ensure_regular_file(target_path, &metadata)?;
    if metadata.len() > MAX_FILE_BYTES as u64 {
        return Err(PodError::from(PslError::new(
            PslErrorKind::LimitExceeded,
            1,
        )));
    }
    let initial_capacity = usize::try_from(metadata.len()).unwrap_or(INITIAL_FILE_CAPACITY);
    read_bounded_utf8(target_path, file, initial_capacity)
}

/// Atomically creates a new file at `target_path` (`O_CREAT | O_EXCL | O_NOFOLLOW`) and writes `content`.
///
/// # Errors
///
/// Returns [`PodError::Io`] with [`io::ErrorKind::AlreadyExists`] if `target_path` already exists
/// (including as a symlink or directory), [`io::ErrorKind::InvalidInput`] if `target_path` is not a
/// regular target name, or another [`PodError::Io`] if creating or writing the file fails.
pub fn write_file_exclusive(target_path: &Path, content: &str) -> Result<(), PodError> {
    if !path::has_regular_target_name(target_path) {
        return Err(invalid_target_error(target_path));
    }
    let working_path = path_resolve::resolve_working_path(target_path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(LINUX_O_NOFOLLOW)
        .open(working_path.as_ref())
        .map_err(|error| io_path_error(target_path, error))?;
    file.write_all(content.as_bytes())
        .map_err(|error| io_path_error(target_path, error))?;
    Ok(())
}

/// Writes `content` to `target_path`, creating or truncating the target regular file with `O_NOFOLLOW`.
///
/// # Errors
///
/// Returns [`PodError::Io`] if `target_path` is a symlink or non-regular file, or if writing fails.
#[cfg(test)]
pub(crate) fn write_file(target_path: &Path, content: &str) -> Result<(), PodError> {
    ensure_writable_target(target_path)?;
    write_file_nofollow(target_path, content)
}

/// Appends a regular-file (or regular-file symlink) directory entry name to `entries` if it is valid UTF-8, matches `extension`, and stays within [`MAX_DIRECTORY_ENTRIES`].
fn collect_matching_entry(
    directory_path: &Path,
    entry_result: io::Result<DirEntry>,
    extension: Option<&str>,
    entries: &mut Vec<String>,
) -> Result<(), PodError> {
    if entries.len() >= MAX_DIRECTORY_ENTRIES {
        return Err(directory_limit_error());
    }
    let entry = entry_result.map_err(|error| io_path_error(directory_path, error))?;
    let file_type = entry
        .file_type()
        .map_err(|error| io_path_error(&entry.path(), error))?;
    let is_regular_file = file_type.is_file()
        || (file_type.is_symlink()
            && fs::metadata(entry.path()).is_ok_and(|metadata| metadata.is_file()));
    if !is_regular_file {
        return Ok(());
    }
    let file_name = entry.file_name();
    let Some(name_str) = file_name.to_str() else {
        return Ok(());
    };
    if !path::matches_extension(name_str, extension) {
        return Ok(());
    }
    entries.push(name_str.to_owned());
    Ok(())
}

/// Creates a [`PodError::Io`] indicating that [`MAX_DIRECTORY_ENTRIES`] was exceeded.
fn directory_limit_error() -> PodError {
    PodError::from(io::Error::new(
        io::ErrorKind::InvalidData,
        "directory entry limit exceeded",
    ))
}

/// Verifies that the parent directory of `target_path` exists on disk.
fn ensure_parent_directory(target_path: &Path) -> Result<(), PodError> {
    let parent = path::parent_or_cur_dir(target_path);
    if !directory_exists(parent) {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Parent directory '{}' does not exist!", parent.display()),
        )));
    }
    Ok(())
}

/// Verifies that `metadata` describes a regular file, rejecting directories, FIFOs, and device nodes.
fn ensure_regular_file(target_path: &Path, metadata: &Metadata) -> Result<(), PodError> {
    if !metadata.is_file() {
        return Err(PodError::from(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("'{}' is not a regular file", target_path.display()),
        )));
    }
    Ok(())
}

/// Creates a [`PodError::Io`] indicating that `target_path` is not a valid regular-file write target.
fn invalid_target_error(target_path: &Path) -> PodError {
    PodError::from(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
            "target path '{}' must be a regular file and not a symlink",
            target_path.display()
        ),
    ))
}

/// Annotates an [`io::Error`] with `target_path` while preserving both [`io::ErrorKind`] and [`Error::source`].
fn io_path_error(target_path: &Path, error: io::Error) -> PodError {
    let kind = error.kind();
    PodError::from(io::Error::new(
        kind,
        PathIoError {
            path: target_path.to_path_buf(),
            source: error,
        },
    ))
}

/// Reads at most [`MAX_FILE_BYTES`] bytes from `reader` and decodes the buffer as UTF-8, annotating I/O and UTF-8 errors with `target_path`.
fn read_bounded_utf8<R: Read>(
    target_path: &Path,
    reader: R,
    initial_capacity: usize,
) -> Result<String, PodError> {
    let mut bytes = Vec::with_capacity(initial_capacity.min(MAX_FILE_BYTES));
    let mut bounded = reader.take((MAX_FILE_BYTES + 1) as u64);
    bounded
        .read_to_end(&mut bytes)
        .map_err(|error| io_path_error(target_path, error))?;
    if bytes.len() > MAX_FILE_BYTES {
        return Err(PodError::from(PslError::new(
            PslErrorKind::LimitExceeded,
            1,
        )));
    }
    String::from_utf8(bytes).map_err(|error| {
        io_path_error(
            target_path,
            io::Error::new(io::ErrorKind::InvalidData, error),
        )
    })
}

/// Opens `target_path` for writing with `O_NOFOLLOW`, truncating or creating it, and writes `content`.
#[cfg(test)]
fn write_file_nofollow(target_path: &Path, content: &str) -> Result<(), PodError> {
    let working_path = path_resolve::resolve_working_path(target_path);
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(LINUX_O_NOFOLLOW)
        .open(working_path.as_ref())
        .map_err(|error| io_path_error(target_path, error))?;
    file.write_all(content.as_bytes())
        .map_err(|error| io_path_error(target_path, error))?;
    Ok(())
}

#[cfg(test)]
#[path = "fs_tests.rs"]
mod tests;
