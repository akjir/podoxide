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
pub(crate) mod targets;
pub(crate) mod update_runner;

use crate::core::context::Context;
pub use crate::core::context::{
    CommandAction, ConfigAction, ConnectAction, DefaultAction, ImageAction, InitAction, LogsAction,
    Mode, ModeAction, RecipeAction,
};
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;

/// Dispatches execution to the active mode handler in O(1) time based on `context.action()`.
///
/// # Workflow
///
/// 1. Match `context.action()` directly across all eight [`ModeAction`] variants in enum
///    declaration order.
/// 2. Invoke the selected mode's `handle(context, action)` entry point (`command::handle`,
///    `config::handle`, `connect::handle`, `default::handle`, `image::handle`,
///    `init::handle`, `logs::handle`, or `recipe::handle`) with its typed sub-action enum.
///
/// # Errors
///
/// Returns [`PodError`] if the selected mode or action handler fails.
pub fn dispatch_mode(context: &Context) -> Result<ExecOutcome, PodError> {
    match context.action() {
        ModeAction::Command(action) => command::handle(context, action),
        ModeAction::Config(action) => config::handle(context, action),
        ModeAction::Connect(action) => connect::handle(context, action),
        ModeAction::Default(action) => default::handle(context, action),
        ModeAction::Image(action) => image::handle(context, action),
        ModeAction::Init(action) => init::handle(context, action),
        ModeAction::Logs(action) => logs::handle(context, action),
        ModeAction::Recipe(action) => recipe::handle(context, action),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::core::cli;
    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{
        CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, ImageAction,
        InitAction, LogsAction, ModeAction, RecipeAction,
    };
    use crate::error::{ConfigErrorKind, PodError};
    use crate::io::exec::command::ExecOutcome;
    use super::dispatch_mode;

    #[test]
    fn dispatch_mode_handles_all_modes_and_actions() {
        let invocations: &[&[&str]] = &[
            &["help"],
            &["version"],
            &["create", "recipe"],
            &["down", "recipe"],
            &["reload", "recipe"],
            &["remove", "recipe"],
            &["restart", "recipe"],
            &["start", "recipe"],
            &["stop", "recipe"],
            &["up", "recipe"],
            &["update", "recipe"],
            &["command", "help"],
            &["command", "list", "recipe"],
            &["command", "exec", "recipe", "migrate"],
            &["cmd", "recipe"],
            &["cmd", "recipe", "1"],
            &["config", "help"],
            &["config", "edit"],
            &["config", "inspect"],
            &["config", "show"],
            &["connect", "help"],
            &["connect", "shell", "recipe/db"],
            &["image", "help"],
            &["image", "prune"],
            &["image", "update", "recipe"],
            &["init", "--simulate"],
            &["init", "help"],
            &["logs", "help"],
            &["logs", "follow", "recipe/db"],
            &["logs", "show", "recipe"],
            &["recipe", "help"],
            &["recipe", "edit", "recipe"],
            &["recipe", "inspect", "recipe"],
            &["recipe", "list"],
            &["recipe", "show", "recipe"],
        ];

        for arguments in invocations {
            let mut context = Context::new();
            cli::parse_args(&mut context, *arguments).unwrap();
            context.load_config_if_required().unwrap();
            assert_eq!(
                dispatch_mode(&context).unwrap(),
                ExecOutcome::Succeeded,
                "expected dispatch_mode to succeed for {arguments:?}"
            );
        }

        // Route `status` with an unregistered target to verify dispatch without invoking host podman.
        let mut status_context = Context::new();
        cli::parse_args(
            &mut status_context,
            ["status", "unregistered_status_target"],
        )
        .unwrap();
        status_context.load_config_if_required().unwrap();
        let status_error = dispatch_mode(&status_context).unwrap_err();
        assert!(matches!(
            status_error,
            PodError::Config(ref error) if error.kind == ConfigErrorKind::RecipeNotFound
        ));
    }

    #[test]
    fn dispatch_mode_routes_all_typed_mode_actions() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();
        let valid_actions = [
            ModeAction::Command(CommandAction::Exec),
            ModeAction::Command(CommandAction::Help),
            ModeAction::Command(CommandAction::List),
            ModeAction::Config(ConfigAction::Edit),
            ModeAction::Config(ConfigAction::Help),
            ModeAction::Config(ConfigAction::Inspect),
            ModeAction::Config(ConfigAction::Show),
            ModeAction::Connect(ConnectAction::Help),
            ModeAction::Connect(ConnectAction::Shell),
            ModeAction::Default(DefaultAction::Create),
            ModeAction::Default(DefaultAction::Down),
            ModeAction::Default(DefaultAction::Help),
            ModeAction::Default(DefaultAction::Reload),
            ModeAction::Default(DefaultAction::Remove),
            ModeAction::Default(DefaultAction::Restart),
            ModeAction::Default(DefaultAction::Start),
            ModeAction::Default(DefaultAction::Status),
            ModeAction::Default(DefaultAction::Stop),
            ModeAction::Default(DefaultAction::Up),
            ModeAction::Default(DefaultAction::Update),
            ModeAction::Default(DefaultAction::Version),
            ModeAction::Image(ImageAction::Help),
            ModeAction::Image(ImageAction::Prune),
            ModeAction::Image(ImageAction::Update),
            ModeAction::Init(InitAction::Create),
            ModeAction::Init(InitAction::Help),
            ModeAction::Logs(LogsAction::Follow),
            ModeAction::Logs(LogsAction::Help),
            ModeAction::Logs(LogsAction::Show),
            ModeAction::Recipe(RecipeAction::Edit),
            ModeAction::Recipe(RecipeAction::Help),
            ModeAction::Recipe(RecipeAction::Inspect),
            ModeAction::Recipe(RecipeAction::List),
            ModeAction::Recipe(RecipeAction::Show),
        ];

        for action in valid_actions {
            let mut context = Context::new();
            context.config = root_config.clone();
            context.action = action;
            if action == ModeAction::Init(InitAction::Create) {
                context.flags.set_simulate(true);
            }
            if action == ModeAction::Default(DefaultAction::Status) {
                context.parameters = vec![String::from("unregistered_status_target")];
                let error = dispatch_mode(&context).unwrap_err();
                assert!(matches!(
                    error,
                    PodError::Config(ref config_error)
                        if config_error.kind == ConfigErrorKind::RecipeNotFound
                ));
                continue;
            }
            context.parameters = if action == ModeAction::Command(CommandAction::Exec) {
                vec![String::from("recipe"), String::from("migrate")]
            } else if action == ModeAction::Connect(ConnectAction::Shell) {
                vec![String::from("recipe/db")]
            } else {
                vec![String::from("recipe")]
            };
            assert_eq!(
                dispatch_mode(&context).unwrap(),
                ExecOutcome::Succeeded,
                "expected dispatch_mode to succeed for {action:?}"
            );
        }
    }

    struct HelpScreenSpec {
        alias: Option<&'static str>,
        mode_title: Option<&'static str>,
        name: &'static str,
        render: fn(&mut Vec<u8>),
    }

    #[test]
    fn all_eight_help_screens_follow_consistent_layout() {
        let screens = [
            HelpScreenSpec {
                alias: None,
                mode_title: None,
                name: "default",
                render: super::default::help::write_help,
            },
            HelpScreenSpec {
                alias: Some("cmd"),
                mode_title: Some("Command Mode"),
                name: "command",
                render: super::command::help::write_help,
            },
            HelpScreenSpec {
                alias: Some("cfg"),
                mode_title: Some("Config Mode"),
                name: "config",
                render: super::config::help::write_help,
            },
            HelpScreenSpec {
                alias: Some("con"),
                mode_title: Some("Connect Mode"),
                name: "connect",
                render: super::connect::help::write_help,
            },
            HelpScreenSpec {
                alias: Some("img"),
                mode_title: Some("Image Mode"),
                name: "image",
                render: super::image::help::write_help,
            },
            HelpScreenSpec {
                alias: None,
                mode_title: Some("Init Mode"),
                name: "init",
                render: super::init::help::write_help,
            },
            HelpScreenSpec {
                alias: Some("log"),
                mode_title: Some("Logs Mode"),
                name: "logs",
                render: super::logs::help::write_help,
            },
            HelpScreenSpec {
                alias: Some("rec"),
                mode_title: Some("Recipe Mode"),
                name: "recipe",
                render: super::recipe::help::write_help,
            },
        ];

        for screen in screens {
            let mut buffer = Vec::new();
            (screen.render)(&mut buffer);
            let output = String::from_utf8(buffer).unwrap();
            let name = screen.name;

            if let Some(title) = screen.mode_title {
                assert!(
                    output.contains(&format!(" - {title}\n")),
                    "{name} help missing mode title {title:?}"
                );
            } else {
                assert!(
                    !output.lines().next().unwrap_or_default().contains(" - "),
                    "default help header must not contain mode suffix"
                );
            }
            if let Some(short) = screen.alias {
                assert!(
                    output.contains(&format!("Alias: {short}\n")),
                    "{name} help missing Alias: {short}"
                );
            } else {
                assert!(
                    !output.contains("Alias:"),
                    "{name} help must not declare Alias:"
                );
            }
            assert!(
                output.contains("Usage: pods "),
                "{name} help missing Usage: line"
            );
            assert!(
                output.contains("  help               display this help and exit\n"),
                "{name} help missing standardized help wording"
            );
            let actions_pos = output.find("ACTIONS:\n").expect("ACTIONS section");
            let options_pos = output.find("OPTIONS:\n").expect("OPTIONS section");
            assert!(
                actions_pos < options_pos,
                "{name} help must place ACTIONS: before OPTIONS:"
            );
            assert!(
                !output.lines().any(|line| line.ends_with(' ')),
                "{name} help must not contain trailing spaces"
            );
        }
    }
}
