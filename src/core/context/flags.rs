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

//! Bitmask-backed command-line flag state stored in [`super::Context`].

use std::path::PathBuf;

/// Bitmask flag for `--all`.
const FLAG_ALL: u8 = 1 << 0;
/// Bitmask flag for `--debug`.
const FLAG_DEBUG: u8 = 1 << 1;
/// Bitmask flag for `--force`.
const FLAG_FORCE: u8 = 1 << 2;
/// Bitmask flag for `--full`.
const FLAG_FULL: u8 = 1 << 3;
/// Bitmask flag for `--preview`.
const FLAG_PREVIEW: u8 = 1 << 4;
/// Bitmask flag for `--simulate`.
const FLAG_SIMULATE: u8 = 1 << 5;
/// Bitmask flag for `--timestamps`.
const FLAG_TIMESTAMPS: u8 = 1 << 6;

/// Command-line flags stored in [`super::Context`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Flags {
    pub config: Option<PathBuf>,
    pub since: Option<String>,
    pub tail: Option<String>,
    pub until: Option<String>,
    mask: u8,
}

impl Flags {
    /// Creates a new [`Flags`] instance with all flags unset.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when `--all` is set.
    #[inline]
    #[must_use]
    pub const fn all(&self) -> bool {
        self.has_flag(FLAG_ALL)
    }

    /// Returns `true` when `--debug` is set.
    #[inline]
    #[must_use]
    pub const fn debug(&self) -> bool {
        self.has_flag(FLAG_DEBUG)
    }

    /// Returns `true` when `--force` is set.
    #[inline]
    #[must_use]
    pub const fn force(&self) -> bool {
        self.has_flag(FLAG_FORCE)
    }

    /// Returns `true` when `--full` is set.
    #[inline]
    #[must_use]
    pub const fn full(&self) -> bool {
        self.has_flag(FLAG_FULL)
    }

    /// Returns `true` when `--preview` is set.
    #[inline]
    #[must_use]
    pub const fn preview(&self) -> bool {
        self.has_flag(FLAG_PREVIEW)
    }

    /// Sets or clears the `--all` flag bit.
    #[inline]
    pub const fn set_all(&mut self, enabled: bool) {
        self.set_flag(FLAG_ALL, enabled);
    }

    /// Sets or clears the `--debug` flag bit.
    #[inline]
    pub const fn set_debug(&mut self, enabled: bool) {
        self.set_flag(FLAG_DEBUG, enabled);
    }

    /// Sets or clears the `--force` flag bit.
    #[inline]
    pub const fn set_force(&mut self, enabled: bool) {
        self.set_flag(FLAG_FORCE, enabled);
    }

    /// Sets or clears the `--full` flag bit.
    #[inline]
    pub const fn set_full(&mut self, enabled: bool) {
        self.set_flag(FLAG_FULL, enabled);
    }

    /// Sets or clears the `--preview` flag bit.
    #[inline]
    pub const fn set_preview(&mut self, enabled: bool) {
        self.set_flag(FLAG_PREVIEW, enabled);
    }

    /// Sets or clears the `--simulate` flag bit.
    #[inline]
    pub const fn set_simulate(&mut self, enabled: bool) {
        self.set_flag(FLAG_SIMULATE, enabled);
    }

    /// Sets or clears the `--timestamps` flag bit.
    #[inline]
    pub const fn set_timestamps(&mut self, enabled: bool) {
        self.set_flag(FLAG_TIMESTAMPS, enabled);
    }

    /// Returns `true` when `--simulate` is set.
    #[inline]
    #[must_use]
    pub const fn simulate(&self) -> bool {
        self.has_flag(FLAG_SIMULATE)
    }

    /// Returns `true` when `--timestamps` is set.
    #[inline]
    #[must_use]
    pub const fn timestamps(&self) -> bool {
        self.has_flag(FLAG_TIMESTAMPS)
    }

    /// Returns `true` if `bit` is set in `self.mask`.
    #[inline]
    #[must_use]
    const fn has_flag(&self, bit: u8) -> bool {
        (self.mask & bit) != 0
    }

    /// Sets or clears `bit` in `self.mask` according to `enabled`.
    #[inline]
    const fn set_flag(&mut self, bit: u8, enabled: bool) {
        if enabled {
            self.mask |= bit;
        } else {
            self.mask &= !bit;
        }
    }
}
