//! External process and Podman CLI execution modules.

pub mod command;
pub mod podman;

pub use self::command::exec_capture;
pub use self::podman::{check_podman_version, is_supported_podman_version};
