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

//! Mode and sub-action enumerations with [`Display`], [`FromStr`], and derived mode resolution.

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use crate::error::{CliError, CliErrorKind};

/// Implements [`Display`] and [`FromStr`] for a CLI action enumeration.
macro_rules! impl_action_traits {
    ($type:ident { $($variant:ident => $label:literal),+ $(,)? }) => {
        impl Display for $type {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
                let label = match self {
                    $(Self::$variant => $label,)+
                };
                formatter.write_str(label)
            }
        }

        impl FromStr for $type {
            type Err = CliError;

            fn from_str(input: &str) -> Result<Self, Self::Err> {
                match input {
                    $($label => Ok(Self::$variant),)+
                    _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
                }
            }
        }
    };
}

/// Sub-actions supported by [`Mode::Command`] (`command` / `cmd`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CommandAction {
    Exec,
    #[default]
    Help,
    List,
}

impl_action_traits!(CommandAction {
    Exec => "exec",
    Help => "help",
    List => "list",
});

/// Sub-actions supported by [`Mode::Config`] (`config` / `cfg`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConfigAction {
    Edit,
    Help,
    #[default]
    Inspect,
    Show,
}

impl_action_traits!(ConfigAction {
    Edit => "edit",
    Help => "help",
    Inspect => "inspect",
    Show => "show",
});

/// Sub-actions supported by [`Mode::Connect`] (`connect` / `con`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConnectAction {
    #[default]
    Help,
    Shell,
}

impl_action_traits!(ConnectAction {
    Help => "help",
    Shell => "shell",
});

/// Lifecycle, status, help, and version actions supported by [`Mode::Default`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DefaultAction {
    Create,
    Down,
    #[default]
    Help,
    Reload,
    Remove,
    Restart,
    Start,
    Status,
    Stop,
    Up,
    Update,
    Version,
}

impl DefaultAction {
    /// Returns `true` if this default action requires at least one target argument.
    #[must_use]
    pub const fn requires_targets(self) -> bool {
        !matches!(self, Self::Help | Self::Status | Self::Version)
    }
}

impl_action_traits!(DefaultAction {
    Create => "create",
    Down => "down",
    Help => "help",
    Reload => "reload",
    Remove => "remove",
    Restart => "restart",
    Start => "start",
    Status => "status",
    Stop => "stop",
    Up => "up",
    Update => "update",
    Version => "version",
});

/// Sub-actions supported by [`Mode::Image`] (`image` / `img`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ImageAction {
    #[default]
    Help,
    Prune,
    Update,
}

impl_action_traits!(ImageAction {
    Help => "help",
    Prune => "prune",
    Update => "update",
});

/// Sub-actions supported by [`Mode::Init`] (`init`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InitAction {
    #[default]
    Create,
    Help,
}

impl_action_traits!(InitAction {
    Create => "create",
    Help => "help",
});

/// Sub-actions supported by [`Mode::Logs`] (`logs` / `log`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LogsAction {
    Follow,
    #[default]
    Help,
    Show,
}

impl_action_traits!(LogsAction {
    Follow => "follow",
    Help => "help",
    Show => "show",
});

/// Top-level CLI execution modes supported by Podoxide (`pods`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    Command,
    Config,
    Connect,
    #[default]
    Default,
    Image,
    Init,
    Logs,
    Recipe,
}

impl Display for Mode {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Command => "command",
            Self::Config => "config",
            Self::Connect => "connect",
            Self::Default => "default",
            Self::Image => "image",
            Self::Init => "init",
            Self::Logs => "logs",
            Self::Recipe => "recipe",
        };
        formatter.write_str(label)
    }
}

impl FromStr for Mode {
    type Err = CliError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "command" | "cmd" => Ok(Self::Command),
            "config" | "cfg" => Ok(Self::Config),
            "connect" | "con" => Ok(Self::Connect),
            "image" | "img" => Ok(Self::Image),
            "init" => Ok(Self::Init),
            "logs" | "log" => Ok(Self::Logs),
            "recipe" | "rec" => Ok(Self::Recipe),
            _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
        }
    }
}

/// Strongly typed mode-and-action pair resolved from positional CLI arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeAction {
    Command(CommandAction),
    Config(ConfigAction),
    Connect(ConnectAction),
    Default(DefaultAction),
    Image(ImageAction),
    Init(InitAction),
    Logs(LogsAction),
    Recipe(RecipeAction),
}

impl ModeAction {
    /// Returns `true` if the resolved action displays help text.
    #[inline]
    #[must_use]
    pub const fn is_help(self) -> bool {
        matches!(
            self,
            Self::Command(CommandAction::Help)
                | Self::Config(ConfigAction::Help)
                | Self::Connect(ConnectAction::Help)
                | Self::Default(DefaultAction::Help)
                | Self::Image(ImageAction::Help)
                | Self::Init(InitAction::Help)
                | Self::Logs(LogsAction::Help)
                | Self::Recipe(RecipeAction::Help)
        )
    }

    /// Derives the top-level [`Mode`] corresponding to this [`ModeAction`].
    #[inline]
    #[must_use]
    pub const fn mode(self) -> Mode {
        match self {
            Self::Command(_) => Mode::Command,
            Self::Config(_) => Mode::Config,
            Self::Connect(_) => Mode::Connect,
            Self::Default(_) => Mode::Default,
            Self::Image(_) => Mode::Image,
            Self::Init(_) => Mode::Init,
            Self::Logs(_) => Mode::Logs,
            Self::Recipe(_) => Mode::Recipe,
        }
    }
}

impl Default for ModeAction {
    fn default() -> Self {
        Self::Default(DefaultAction::Help)
    }
}

impl Display for ModeAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command(action) => write!(formatter, "{action}"),
            Self::Config(action) => write!(formatter, "{action}"),
            Self::Connect(action) => write!(formatter, "{action}"),
            Self::Default(action) => write!(formatter, "{action}"),
            Self::Image(action) => write!(formatter, "{action}"),
            Self::Init(action) => write!(formatter, "{action}"),
            Self::Logs(action) => write!(formatter, "{action}"),
            Self::Recipe(action) => write!(formatter, "{action}"),
        }
    }
}

/// Sub-actions supported by [`Mode::Recipe`] (`recipe` / `rec`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RecipeAction {
    Edit,
    Help,
    Inspect,
    #[default]
    List,
    Show,
}

impl_action_traits!(RecipeAction {
    Edit => "edit",
    Help => "help",
    Inspect => "inspect",
    List => "list",
    Show => "show",
});
