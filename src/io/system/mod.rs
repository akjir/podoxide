//! Host environment and filesystem I/O modules.

pub mod env;

pub use self::env::{
    MAX_CONFIRMATION_BYTES, check_os, confirm_elevated_execution, is_root_uid, is_supported_os,
    runs_elevated,
};
