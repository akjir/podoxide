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

//! Internal library backing the Podoxide CLI tool (`pods`).
//!
//! Podoxide is distributed as the `pods` command-line binary and does not provide a stable public
//! Rust library API.

#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod core;
#[doc(hidden)]
pub mod error;
#[doc(hidden)]
pub mod io;
#[doc(hidden)]
pub mod utilities;
