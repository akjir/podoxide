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
