//! Host environment and filesystem I/O modules.

pub mod env;
pub mod fs;

pub use self::env::{
    MAX_CONFIRMATION_BYTES, check_os, confirm_elevated_execution, is_root_uid, is_supported_os,
    runs_elevated,
};
pub use self::fs::{
    MAX_DIRECTORY_ENTRIES, directory_exists, file_exists, get_absolute_path, list_directory,
    read_file, write_file,
};
