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

//! Cargo build script generating compile-time version and Git build metadata.

#![forbid(unsafe_code)]

#[path = "build_metadata.rs"]
mod build_metadata;

use std::env;
use std::io;
use std::path::Path;

/// Fallback package version used when `CARGO_PKG_VERSION` is unset.
const DEFAULT_PACKAGE_VERSION: &str = "0.0.0";

/// Runs the Cargo build script to embed Git version metadata at compile time.
fn main() -> io::Result<()> {
    let package_version =
        env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| DEFAULT_PACKAGE_VERSION.to_owned());
    let mut stdout = io::stdout().lock();
    build_metadata::write_version_metadata(Path::new("."), &package_version, &mut stdout)
}
