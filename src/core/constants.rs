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

//! Global application metadata and compile-time version constants for Podoxide.

/// Application display name.
pub const APP_NAME: &str = "Podoxide";

/// Application version with SemVer 2.0.0 build metadata dynamically generated at compile time.
pub const APP_VERSION: &str = env!("PODOXIDE_FULL_VERSION");

#[cfg(test)]
mod tests {
    use super::{APP_NAME, APP_VERSION};

    #[test]
    fn global_application_constants_match_expected_format() {
        assert_eq!(APP_NAME, "Podoxide");
        assert!(APP_VERSION.starts_with(&format!("v{}+", env!("CARGO_PKG_VERSION"))));
    }
}
