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

//! Central runtime context holding configuration, parsed CLI flags, mode action, and parameters.

mod actions;
mod flags;
#[cfg(test)]
mod tests;

use std::mem;

use crate::core::config::Config;
use crate::core::log;
use crate::error::PodError;
pub use self::actions::{
    CommandAction, ConfigAction, ConnectAction, DefaultAction, ImageAction, InitAction, LogsAction,
    Mode, ModeAction, RecipeAction,
};
pub use self::flags::Flags;

/// Application runtime context passed through CLI parsing, mode dispatchers, and action handlers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Context {
    pub action: ModeAction,
    pub config: Config,
    pub flags: Flags,
    pub parameters: Vec<String>,
}

impl Context {
    /// Creates a new [`Context`] initialized with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the resolved [`ModeAction`] for this invocation.
    #[inline]
    #[must_use]
    pub const fn action(&self) -> ModeAction {
        self.action
    }

    /// Loads `pods.conf` if required by the selected mode action and syncs the `simulate` flag.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if loading or validating the configuration fails.
    pub fn load_config_if_required(&mut self) -> Result<(), PodError> {
        if self.requires_config() {
            let mut loaded = Config::load(&self.config.path)?;
            loaded.name = mem::take(&mut self.config.name);
            loaded.path = mem::take(&mut self.config.path);
            loaded.simulate |= self.flags.simulate();
            self.config = loaded;
            if self.config.simulate {
                log::info("Simulate mode is active.");
            }
        }
        Ok(())
    }

    /// Returns the top-level [`Mode`] derived from `self.action`.
    #[inline]
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.action.mode()
    }

    /// Returns `true` if this context's mode and action require loading and parsing `pods.conf`.
    #[must_use]
    pub const fn requires_config(&self) -> bool {
        if self.action.is_help() {
            return false;
        }
        !matches!(
            self.action,
            ModeAction::Config(ConfigAction::Show)
                | ModeAction::Default(DefaultAction::Version)
                | ModeAction::Init(_)
        )
    }

    /// Returns `true` if elevated (`UID 0`) execution requires interactive confirmation for this action.
    #[must_use]
    pub const fn requires_elevation_confirmation(&self) -> bool {
        self.requires_podman()
            || matches!(
                self.action,
                ModeAction::Config(ConfigAction::Edit)
                    | ModeAction::Init(InitAction::Create)
                    | ModeAction::Recipe(RecipeAction::Edit)
            )
    }

    /// Returns `true` if this context's mode and action require a supported Podman installation (`5.8.0+`).
    #[must_use]
    pub const fn requires_podman(&self) -> bool {
        match self.action {
            ModeAction::Command(CommandAction::Exec)
            | ModeAction::Connect(ConnectAction::Shell)
            | ModeAction::Image(ImageAction::Prune | ImageAction::Update)
            | ModeAction::Logs(LogsAction::Follow | LogsAction::Show) => true,
            ModeAction::Default(action) => {
                !matches!(action, DefaultAction::Help | DefaultAction::Version)
            }
            ModeAction::Command(CommandAction::Help | CommandAction::List)
            | ModeAction::Config(_)
            | ModeAction::Connect(ConnectAction::Help)
            | ModeAction::Image(ImageAction::Help)
            | ModeAction::Init(_)
            | ModeAction::Logs(LogsAction::Help)
            | ModeAction::Recipe(_) => false,
        }
    }

    /// Returns `true` if this context's mode and action read the configured `pods.conf` file path.
    #[must_use]
    pub const fn uses_config_file(&self) -> bool {
        self.requires_config() || matches!(self.action, ModeAction::Config(ConfigAction::Show))
    }
}
