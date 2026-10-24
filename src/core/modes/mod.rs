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

//! Mode enumeration re-exports and O(1) mode dispatcher.

pub mod command;
pub mod config;
pub mod connect;
pub mod default;
pub mod image;
pub mod init;
pub mod logs;
pub mod recipe;

use crate::core::context::Context;
pub use crate::core::context::{
    CommandAction, ConfigAction, ConnectAction, DefaultAction, ImageAction, InitAction,
    LogsAction, Mode, ModeAction, RecipeAction,
};
use crate::error::PodError;

const MODE_COUNT: usize = 8;
static MODE_DISPATCH_TABLE: [ModeHandler; MODE_COUNT] = [
    command::handle,
    config::handle,
    connect::handle,
    default::handle,
    image::handle,
    init::handle,
    logs::handle,
    recipe::handle,
];

/// Function pointer signature for a top-level CLI mode handler.
type ModeHandler = fn(&Context) -> Result<(), PodError>;

/// Dispatches execution to the active mode handler in O(1) time via `MODE_DISPATCH_TABLE`.
///
/// # Workflow
///
/// 1. Look up the mode handler function in `MODE_DISPATCH_TABLE` using
///    `context.mode().as_index()`.
/// 2. Invoke the selected mode's `handle(context)` entry point (`command::handle`,
///    `config::handle`, `connect::handle`, `default::handle`, `image::handle`,
///    `init::handle`, `logs::handle`, or `recipe::handle`).
///
/// # Errors
///
/// Returns [`PodError`] if the selected mode or action handler fails.
pub fn dispatch_mode(context: &Context) -> Result<(), PodError> {
    let handler = MODE_DISPATCH_TABLE[context.mode().as_index()];
    handler(context)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::cli::parse_args;
    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{
        CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, ImageAction,
        InitAction, LogsAction, Mode, ModeAction, RecipeAction,
    };
    use crate::error::{CliErrorKind, PodError};
    use super::dispatch_mode;

    #[test]
    fn dispatch_mode_handles_all_modes_and_actions() {
        let invocations: &[&[&str]] = &[
            &["help"],
            &["create", "recipe"],
            &["down", "recipe"],
            &["reload", "recipe"],
            &["remove", "recipe"],
            &["restart", "recipe"],
            &["start", "recipe"],
            &["status"],
            &["stop", "recipe"],
            &["up", "recipe"],
            &["update", "recipe"],
            &["command", "help"],
            &["command", "list", "recipe"],
            &["command", "exec", "recipe", "migrate"],
            &["config", "help"],
            &["config", "edit"],
            &["config", "show"],
            &["connect", "help"],
            &["connect", "shell", "recipe/db"],
            &["image", "help"],
            &["image", "prune"],
            &["image", "update", "recipe"],
            &["init"],
            &["init", "help"],
            &["logs", "help"],
            &["logs", "follow", "recipe/db"],
            &["logs", "show", "recipe"],
            &["recipe", "help"],
            &["recipe", "edit", "recipe"],
            &["recipe", "list"],
            &["recipe", "show", "recipe"],
        ];

        for arguments in invocations {
            let mut context = Context::new();
            parse_args(&mut context, *arguments).unwrap();
            context.load_config_if_required().unwrap();
            assert!(
                dispatch_mode(&context).is_ok(),
                "expected dispatch_mode to succeed for {arguments:?}"
            );
        }
    }

    #[test]
    fn mode_handlers_reject_mismatched_mode_actions() {
        let mismatched_pairs = [
            (Mode::Command, ModeAction::Default(DefaultAction::Help)),
            (Mode::Config, ModeAction::Default(DefaultAction::Help)),
            (Mode::Connect, ModeAction::Default(DefaultAction::Help)),
            (Mode::Default, ModeAction::Command(CommandAction::Help)),
            (Mode::Image, ModeAction::Config(ConfigAction::Help)),
            (Mode::Init, ModeAction::Connect(ConnectAction::Help)),
            (Mode::Logs, ModeAction::Image(ImageAction::Help)),
            (Mode::Recipe, ModeAction::Init(InitAction::Help)),
        ];

        for (mode, action) in mismatched_pairs {
            let mut context = Context::new();
            context.mode = mode;
            context.action = action;

            let error = dispatch_mode(&context).unwrap_err();
            match error {
                PodError::Cli(cli_error) => {
                    assert_eq!(cli_error.kind, CliErrorKind::UnknownAction);
                }
                other => panic!("expected PodError::Cli(UnknownAction), got {other:?}"),
            }
        }

        // Verify all action enum variants map cleanly through their respective modes.
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();
        let valid_pairs = [
            (Mode::Command, ModeAction::Command(CommandAction::Exec)),
            (Mode::Command, ModeAction::Command(CommandAction::Help)),
            (Mode::Command, ModeAction::Command(CommandAction::List)),
            (Mode::Config, ModeAction::Config(ConfigAction::Edit)),
            (Mode::Config, ModeAction::Config(ConfigAction::Help)),
            (Mode::Config, ModeAction::Config(ConfigAction::Show)),
            (Mode::Connect, ModeAction::Connect(ConnectAction::Help)),
            (Mode::Connect, ModeAction::Connect(ConnectAction::Shell)),
            (Mode::Default, ModeAction::Default(DefaultAction::Create)),
            (Mode::Default, ModeAction::Default(DefaultAction::Down)),
            (Mode::Default, ModeAction::Default(DefaultAction::Help)),
            (Mode::Default, ModeAction::Default(DefaultAction::Reload)),
            (Mode::Default, ModeAction::Default(DefaultAction::Remove)),
            (Mode::Default, ModeAction::Default(DefaultAction::Restart)),
            (Mode::Default, ModeAction::Default(DefaultAction::Start)),
            (Mode::Default, ModeAction::Default(DefaultAction::Status)),
            (Mode::Default, ModeAction::Default(DefaultAction::Stop)),
            (Mode::Default, ModeAction::Default(DefaultAction::Up)),
            (Mode::Default, ModeAction::Default(DefaultAction::Update)),
            (Mode::Image, ModeAction::Image(ImageAction::Help)),
            (Mode::Image, ModeAction::Image(ImageAction::Prune)),
            (Mode::Image, ModeAction::Image(ImageAction::Update)),
            (Mode::Init, ModeAction::Init(InitAction::Create)),
            (Mode::Init, ModeAction::Init(InitAction::Help)),
            (Mode::Logs, ModeAction::Logs(LogsAction::Follow)),
            (Mode::Logs, ModeAction::Logs(LogsAction::Help)),
            (Mode::Logs, ModeAction::Logs(LogsAction::Show)),
            (Mode::Recipe, ModeAction::Recipe(RecipeAction::Edit)),
            (Mode::Recipe, ModeAction::Recipe(RecipeAction::Help)),
            (Mode::Recipe, ModeAction::Recipe(RecipeAction::List)),
            (Mode::Recipe, ModeAction::Recipe(RecipeAction::Show)),
        ];

        for (mode, action) in valid_pairs {
            let mut context = Context::new();
            context.config = root_config.clone();
            context.mode = mode;
            context.action = action;
            context.parameters = vec![String::from("recipe")];
            assert!(dispatch_mode(&context).is_ok());
        }
    }
}
