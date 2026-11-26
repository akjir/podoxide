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

//! Unit tests for [`super::Context`], [`super::Flags`], and mode/action enumerations.

use std::fmt::Debug;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::core::cli;
use crate::core::config::{Config, PodsConfig, RecipesConfig};
use crate::error::{CliError, CliErrorKind};
use crate::io::system::fs::TestTempDir;
use super::{
    CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, Flags, ImageAction,
    InitAction, LogsAction, Mode, ModeAction, RecipeAction,
};

fn assert_action_simulate_transitions(
    action: ModeAction,
    simulate_false_path: &Path,
    simulate_true_path: &Path,
) {
    let mut false_context = Context::new();
    false_context.action = action;
    false_context.config.name = String::from("sim_false");
    false_context.config.path = simulate_false_path.to_path_buf();
    assert!(false_context.config.simulate);
    assert!(false_context.requires_config());
    false_context.load_config_if_required().unwrap();
    assert!(
        !false_context.config.simulate,
        "expected simulate=false from config for action {action:?}"
    );

    let mut override_context = Context::new();
    override_context.action = action;
    override_context.config.name = String::from("sim_false");
    override_context.config.path = simulate_false_path.to_path_buf();
    override_context.flags.set_simulate(true);
    override_context.load_config_if_required().unwrap();
    assert!(
        override_context.config.simulate,
        "expected --simulate flag to override simulate=false for action {action:?}"
    );

    let mut true_context = Context::new();
    true_context.action = action;
    true_context.config.name = String::from("sim_true");
    true_context.config.path = simulate_true_path.to_path_buf();
    true_context.config.simulate = false;
    true_context.load_config_if_required().unwrap();
    assert!(
        true_context.config.simulate,
        "expected simulate=true from config for action {action:?}"
    );
}

fn assert_unknown_action<T: FromStr<Err = CliError> + Debug>() {
    assert_eq!(
        T::from_str("unknown").unwrap_err().kind,
        CliErrorKind::UnknownAction
    );
}

fn flag_states(flags: &Flags) -> [bool; 7] {
    [
        flags.all(),
        flags.debug(),
        flags.force(),
        flags.full(),
        flags.preview(),
        flags.simulate(),
        flags.timestamps(),
    ]
}

#[test]
fn context_defaults_and_cli_population() {
    let default_context = Context::new();
    assert_eq!(default_context.mode(), Mode::Default);
    assert_eq!(
        default_context.action,
        ModeAction::Default(DefaultAction::Help)
    );
    assert_eq!(
        default_context.action(),
        ModeAction::Default(DefaultAction::Help)
    );
    assert_eq!(default_context.config, Config::new());
    assert_eq!(default_context.config.editor, "");
    assert_eq!(default_context.config.name, "pods");
    assert_eq!(default_context.config.path, PathBuf::from("./pods.conf"));
    assert_eq!(default_context.config.pods, PodsConfig::new());
    assert_eq!(default_context.config.recipes, RecipesConfig::new());
    assert!(default_context.config.simulate);
    assert_eq!(default_context.flags, Flags::new());
    assert!(!default_context.flags.simulate());
    assert!(default_context.parameters.is_empty());
    assert!(!default_context.requires_config());

    let mut context = Context::new();
    cli::parse_args(
        &mut context,
        ["--config=staging", "--debug", "up", "example-app"],
    )
    .unwrap();
    assert!(context.flags.debug());
    assert!(!context.flags.simulate());
    assert!(context.config.simulate);
    assert_eq!(context.config.name, "staging");
    assert_eq!(context.config.path, PathBuf::from("./staging.conf"));
    assert_eq!(context.parameters, vec!["example-app"]);
    assert!(context.requires_config());

    context.config.simulate = false;
    assert!(context.load_config_if_required().is_err());

    let mut simulate_context = Context::new();
    cli::parse_args(&mut simulate_context, ["--simulate", "up", "example-app"]).unwrap();
    assert!(simulate_context.flags.simulate());
    assert!(simulate_context.config.simulate);
    simulate_context.config.simulate = false;
    simulate_context.load_config_if_required().unwrap();
    assert_eq!(simulate_context.config.editor, "vim");
    assert_eq!(simulate_context.config.name, "pods");
    assert_eq!(simulate_context.config.path, PathBuf::from("./pods.conf"));
    assert_eq!(simulate_context.config.pods.path, PathBuf::from("./pods"));
    assert!(simulate_context.config.simulate);

    let mut init_context = Context::new();
    cli::parse_args(&mut init_context, ["init"]).unwrap();
    assert!(!init_context.requires_config());
    init_context.config.simulate = false;
    init_context.load_config_if_required().unwrap();
    assert!(!init_context.config.simulate);
    assert!(!init_context.flags.simulate());

    let mut prune_context = Context::new();
    cli::parse_args(&mut prune_context, ["image", "prune"]).unwrap();
    assert!(prune_context.requires_config());
    prune_context.config.simulate = false;
    prune_context.load_config_if_required().unwrap();
    assert!(prune_context.config.simulate);
}

#[test]
fn display_from_str_and_mode_derivation() {
    assert_unknown_action::<CommandAction>();
    assert_unknown_action::<ConfigAction>();
    assert_unknown_action::<ConnectAction>();
    assert_unknown_action::<DefaultAction>();
    assert_unknown_action::<ImageAction>();
    assert_eq!(InitAction::from_str("create").unwrap(), InitAction::Create);
    assert_eq!(InitAction::from_str("help").unwrap(), InitAction::Help);
    assert_unknown_action::<InitAction>();
    assert_unknown_action::<LogsAction>();
    assert_unknown_action::<Mode>();
    assert_unknown_action::<RecipeAction>();

    assert_eq!(Mode::Command.to_string(), "command");
    assert_eq!(Mode::Config.to_string(), "config");
    assert_eq!(Mode::Connect.to_string(), "connect");
    assert_eq!(Mode::Default.to_string(), "default");
    assert_eq!(Mode::Image.to_string(), "image");
    assert_eq!(Mode::Init.to_string(), "init");
    assert_eq!(Mode::Logs.to_string(), "logs");
    assert_eq!(Mode::Recipe.to_string(), "recipe");

    let action_modes = [
        (
            ModeAction::Command(CommandAction::Exec),
            Mode::Command,
            "exec",
        ),
        (
            ModeAction::Command(CommandAction::Help),
            Mode::Command,
            "help",
        ),
        (
            ModeAction::Command(CommandAction::List),
            Mode::Command,
            "list",
        ),
        (ModeAction::Config(ConfigAction::Edit), Mode::Config, "edit"),
        (ModeAction::Config(ConfigAction::Help), Mode::Config, "help"),
        (
            ModeAction::Config(ConfigAction::Inspect),
            Mode::Config,
            "inspect",
        ),
        (ModeAction::Config(ConfigAction::Show), Mode::Config, "show"),
        (
            ModeAction::Connect(ConnectAction::Help),
            Mode::Connect,
            "help",
        ),
        (
            ModeAction::Connect(ConnectAction::Shell),
            Mode::Connect,
            "shell",
        ),
        (
            ModeAction::Default(DefaultAction::Create),
            Mode::Default,
            "create",
        ),
        (
            ModeAction::Default(DefaultAction::Down),
            Mode::Default,
            "down",
        ),
        (
            ModeAction::Default(DefaultAction::Help),
            Mode::Default,
            "help",
        ),
        (
            ModeAction::Default(DefaultAction::Reload),
            Mode::Default,
            "reload",
        ),
        (
            ModeAction::Default(DefaultAction::Remove),
            Mode::Default,
            "remove",
        ),
        (
            ModeAction::Default(DefaultAction::Restart),
            Mode::Default,
            "restart",
        ),
        (
            ModeAction::Default(DefaultAction::Start),
            Mode::Default,
            "start",
        ),
        (
            ModeAction::Default(DefaultAction::Status),
            Mode::Default,
            "status",
        ),
        (
            ModeAction::Default(DefaultAction::Stop),
            Mode::Default,
            "stop",
        ),
        (ModeAction::Default(DefaultAction::Up), Mode::Default, "up"),
        (
            ModeAction::Default(DefaultAction::Update),
            Mode::Default,
            "update",
        ),
        (
            ModeAction::Default(DefaultAction::Version),
            Mode::Default,
            "version",
        ),
        (ModeAction::Image(ImageAction::Help), Mode::Image, "help"),
        (ModeAction::Image(ImageAction::Prune), Mode::Image, "prune"),
        (
            ModeAction::Image(ImageAction::Update),
            Mode::Image,
            "update",
        ),
        (ModeAction::Init(InitAction::Create), Mode::Init, "create"),
        (ModeAction::Init(InitAction::Help), Mode::Init, "help"),
        (ModeAction::Logs(LogsAction::Follow), Mode::Logs, "follow"),
        (ModeAction::Logs(LogsAction::Help), Mode::Logs, "help"),
        (ModeAction::Logs(LogsAction::Show), Mode::Logs, "show"),
        (ModeAction::Recipe(RecipeAction::Edit), Mode::Recipe, "edit"),
        (ModeAction::Recipe(RecipeAction::Help), Mode::Recipe, "help"),
        (
            ModeAction::Recipe(RecipeAction::Inspect),
            Mode::Recipe,
            "inspect",
        ),
        (ModeAction::Recipe(RecipeAction::List), Mode::Recipe, "list"),
        (ModeAction::Recipe(RecipeAction::Show), Mode::Recipe, "show"),
    ];

    for (action, expected_mode, expected_label) in action_modes {
        let mut context = Context::new();
        context.action = action;
        assert_eq!(
            action.mode(),
            expected_mode,
            "unexpected derived mode for {action:?}"
        );
        assert_eq!(
            context.mode(),
            expected_mode,
            "unexpected context.mode() for {action:?}"
        );
        assert_eq!(
            action.to_string(),
            expected_label,
            "unexpected Display label for {action:?}"
        );
    }
}

#[test]
fn flags_bitmask_toggles_all_seven_flags_independently() {
    let setters: [fn(&mut Flags, bool); 7] = [
        Flags::set_all,
        Flags::set_debug,
        Flags::set_force,
        Flags::set_full,
        Flags::set_preview,
        Flags::set_simulate,
        Flags::set_timestamps,
    ];

    let default_flags = Flags::new();
    assert_eq!(flag_states(&default_flags), [false; 7]);

    for (index, setter) in setters.iter().enumerate() {
        let mut flags = Flags::new();
        setter(&mut flags, true);
        let mut expected = [false; 7];
        expected[index] = true;
        assert_eq!(
            flag_states(&flags),
            expected,
            "setting flag index {index} must not affect other bits"
        );

        setter(&mut flags, false);
        assert_eq!(
            flag_states(&flags),
            [false; 7],
            "clearing flag index {index} must restore empty mask"
        );
    }

    let mut all_set = Flags::new();
    for setter in setters {
        setter(&mut all_set, true);
    }
    assert_eq!(flag_states(&all_set), [true; 7]);

    for (index, setter) in setters.iter().enumerate() {
        let mut flags = all_set.clone();
        setter(&mut flags, false);
        let mut expected = [true; 7];
        expected[index] = false;
        assert_eq!(
            flag_states(&flags),
            expected,
            "clearing flag index {index} must preserve the remaining six bits"
        );
    }
}

#[test]
fn requires_config_podman_elevation_and_simulate_sync_across_all_actions() {
    let config_bypass_actions = [
        ModeAction::Command(CommandAction::Help),
        ModeAction::Config(ConfigAction::Help),
        ModeAction::Config(ConfigAction::Show),
        ModeAction::Connect(ConnectAction::Help),
        ModeAction::Default(DefaultAction::Help),
        ModeAction::Default(DefaultAction::Version),
        ModeAction::Image(ImageAction::Help),
        ModeAction::Init(InitAction::Create),
        ModeAction::Init(InitAction::Help),
        ModeAction::Logs(LogsAction::Help),
        ModeAction::Recipe(RecipeAction::Help),
    ];

    for action in config_bypass_actions {
        let mut context = Context::new();
        context.action = action;
        assert!(
            !context.requires_config(),
            "expected {action:?} to bypass config loading"
        );
        let expects_config_file = action == ModeAction::Config(ConfigAction::Show);
        assert_eq!(
            context.uses_config_file(),
            expects_config_file,
            "unexpected uses_config_file() for {action:?}"
        );
        assert!(
            !context.requires_podman(),
            "expected {action:?} to bypass Podman check"
        );
        let expects_elevation = action == ModeAction::Init(InitAction::Create);
        assert_eq!(
            context.requires_elevation_confirmation(),
            expects_elevation,
            "unexpected requires_elevation_confirmation() for {action:?}"
        );
    }

    let config_actions = [
        ModeAction::Command(CommandAction::Exec),
        ModeAction::Command(CommandAction::List),
        ModeAction::Config(ConfigAction::Edit),
        ModeAction::Config(ConfigAction::Inspect),
        ModeAction::Connect(ConnectAction::Shell),
        ModeAction::Default(DefaultAction::Create),
        ModeAction::Default(DefaultAction::Down),
        ModeAction::Default(DefaultAction::Reload),
        ModeAction::Default(DefaultAction::Remove),
        ModeAction::Default(DefaultAction::Restart),
        ModeAction::Default(DefaultAction::Start),
        ModeAction::Default(DefaultAction::Status),
        ModeAction::Default(DefaultAction::Stop),
        ModeAction::Default(DefaultAction::Up),
        ModeAction::Default(DefaultAction::Update),
        ModeAction::Image(ImageAction::Prune),
        ModeAction::Image(ImageAction::Update),
        ModeAction::Logs(LogsAction::Follow),
        ModeAction::Logs(LogsAction::Show),
        ModeAction::Recipe(RecipeAction::Edit),
        ModeAction::Recipe(RecipeAction::Inspect),
        ModeAction::Recipe(RecipeAction::List),
        ModeAction::Recipe(RecipeAction::Show),
    ];

    let temp_dir = TestTempDir::new("ctx_sim_test");
    let simulate_false_path = temp_dir.join("sim_false.conf");
    let simulate_true_path = temp_dir.join("sim_true.conf");
    fs::write(
        &simulate_false_path,
        "general {\n    editor vim\n    simulate false\n}\nrecipes {\n    groups {\n        all recipe\n    }\n}\n",
    )
    .unwrap();
    fs::write(
        &simulate_true_path,
        "general {\n    editor vim\n    simulate true\n}\nrecipes {\n    groups {\n        all recipe\n    }\n}\n",
    )
    .unwrap();

    for action in config_actions {
        let mut context = Context::new();
        context.action = action;
        assert!(
            context.uses_config_file(),
            "expected {action:?} to use the configured pods.conf file"
        );
        let expects_podman = matches!(
            action,
            ModeAction::Command(CommandAction::Exec)
                | ModeAction::Connect(ConnectAction::Shell)
                | ModeAction::Default(_)
                | ModeAction::Image(_)
                | ModeAction::Logs(_)
        );
        assert_eq!(
            context.requires_podman(),
            expects_podman,
            "unexpected requires_podman() for {action:?}"
        );
        let expects_elevation = expects_podman
            || matches!(
                action,
                ModeAction::Config(ConfigAction::Edit) | ModeAction::Recipe(RecipeAction::Edit)
            );
        assert_eq!(
            context.requires_elevation_confirmation(),
            expects_elevation,
            "unexpected requires_elevation_confirmation() for {action:?}"
        );
        assert_action_simulate_transitions(action, &simulate_false_path, &simulate_true_path);
    }
}
