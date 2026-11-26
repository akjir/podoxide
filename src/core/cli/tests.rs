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

//! Unit tests for CLI mode/action positional parsing, aliases, and target validation.

use crate::core::context::{
    CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, Flags, ImageAction,
    InitAction, LogsAction, Mode, ModeAction, RecipeAction,
};
use crate::error::CliErrorKind;
use super::{parse_args, positionals};

fn assert_equivalent_parse(
    alias_args: &[&str],
    canonical_args: &[&str],
    expected_action: ModeAction,
    expected_parameters: &[&str],
) {
    let alias_context = assert_parse(alias_args, expected_action, expected_parameters);
    let canonical_context = assert_parse(canonical_args, expected_action, expected_parameters);
    assert_eq!(
        alias_context, canonical_context,
        "expected {alias_args:?} to match {canonical_args:?}"
    );
}

fn assert_parse(
    arguments: &[&str],
    expected_action: ModeAction,
    expected_parameters: &[&str],
) -> Context {
    let mut context = Context::new();
    parse_args(&mut context, arguments).unwrap();
    assert_eq!(
        context.action, expected_action,
        "unexpected action for {arguments:?}"
    );
    assert_eq!(
        context.mode(),
        expected_action.mode(),
        "unexpected mode for {arguments:?}"
    );
    assert_eq!(
        context.parameters, expected_parameters,
        "unexpected parameters for {arguments:?}"
    );
    context
}

#[test]
fn default_lifecycle_actions_and_targets() {
    let lifecycle_actions = [
        ("create", DefaultAction::Create),
        ("down", DefaultAction::Down),
        ("reload", DefaultAction::Reload),
        ("remove", DefaultAction::Remove),
        ("restart", DefaultAction::Restart),
        ("start", DefaultAction::Start),
        ("stop", DefaultAction::Stop),
        ("up", DefaultAction::Up),
        ("update", DefaultAction::Update),
    ];

    for (keyword, expected_action) in lifecycle_actions {
        let mut context = Context::new();
        parse_args(&mut context, [keyword, "example-app", "@backend"]).unwrap();
        assert_eq!(context.mode(), Mode::Default);
        assert_eq!(context.action, ModeAction::Default(expected_action));
        assert_eq!(context.parameters, vec!["example-app", "@backend"]);
        assert!(context.requires_config());

        let mut container_context = Context::new();
        parse_args(
            &mut container_context,
            [
                keyword,
                "buce/buce-app",
                "buce/*app",
                "buce/app",
                "buce/1",
                "other_pod/db",
            ],
        )
        .unwrap();
        assert_eq!(container_context.mode(), Mode::Default);
        assert_eq!(
            container_context.action,
            ModeAction::Default(expected_action)
        );
        assert_eq!(
            container_context.parameters,
            vec![
                "buce/buce-app",
                "buce/*app",
                "buce/app",
                "buce/1",
                "other_pod/db",
            ]
        );

        for mixed_targets in [
            [keyword, "@all", "other_pod", "buce/app"].as_slice(),
            [keyword, "other_pod", "buce/app"].as_slice(),
            [keyword, "buce/app", "other_pod"].as_slice(),
            [keyword, "buce/app", "@all"].as_slice(),
        ] {
            let mut mixed_context = Context::new();
            let mixed_error = parse_args(&mut mixed_context, mixed_targets).unwrap_err();
            assert_eq!(
                mixed_error.kind,
                CliErrorKind::MixedTargets,
                "expected MixedTargets for {mixed_targets:?}"
            );
        }

        let mut missing_context = Context::new();
        let missing_target = parse_args(&mut missing_context, [keyword]).unwrap_err();
        assert_eq!(missing_target.kind, CliErrorKind::MissingTarget);
    }

    let mut status_default = Context::new();
    parse_args(&mut status_default, ["status", "--all", "--full"]).unwrap();
    assert_eq!(status_default.mode(), Mode::Default);
    assert_eq!(
        status_default.action,
        ModeAction::Default(DefaultAction::Status)
    );
    assert!(status_default.flags.all());
    assert!(status_default.flags.full());
    assert!(status_default.requires_config());

    let mut status_containers = Context::new();
    parse_args(
        &mut status_containers,
        ["status", "--full", "buce/app", "buce/1"],
    )
    .unwrap();
    assert!(status_containers.flags.full());
    assert_eq!(status_containers.parameters, vec!["buce/app", "buce/1"]);

    let mut status_mixed = Context::new();
    let status_mixed_err = parse_args(&mut status_mixed, ["status", "buce", "buce/1"]).unwrap_err();
    assert_eq!(status_mixed_err.kind, CliErrorKind::MixedTargets);

    let mut empty_invocation = Context::new();
    parse_args(&mut empty_invocation, Vec::<String>::new()).unwrap();
    assert_eq!(empty_invocation, Context::new());
    assert_eq!(empty_invocation.flags, Flags::new());
    assert_eq!(empty_invocation.mode(), Mode::Default);
    assert_eq!(
        empty_invocation.action,
        ModeAction::Default(DefaultAction::Help)
    );
    assert!(!empty_invocation.requires_config());
}

#[test]
fn modes_and_three_letter_aliases_and_reject_ini() {
    assert_equivalent_parse(
        &["cmd", "exec", "example-app", "migrate"],
        &["command", "exec", "example-app", "migrate"],
        ModeAction::Command(CommandAction::Exec),
        &["example-app", "migrate"],
    );
    assert_equivalent_parse(
        &["command"],
        &["command", "help"],
        ModeAction::Command(CommandAction::Help),
        &[],
    );
    assert_equivalent_parse(
        &["cmd", "example-app"],
        &["command", "list", "example-app"],
        ModeAction::Command(CommandAction::List),
        &["example-app"],
    );
    assert_equivalent_parse(
        &["cfg"],
        &["config", "inspect"],
        ModeAction::Config(ConfigAction::Inspect),
        &[],
    );
    assert_equivalent_parse(
        &["con", "example-app/db"],
        &["connect", "shell", "example-app/db"],
        ModeAction::Connect(ConnectAction::Shell),
        &["example-app/db"],
    );
    assert_equivalent_parse(
        &["connect"],
        &["connect", "help"],
        ModeAction::Connect(ConnectAction::Help),
        &[],
    );
    assert_equivalent_parse(
        &["img", "prune", "--all", "--force"],
        &["image", "prune", "--all", "--force"],
        ModeAction::Image(ImageAction::Prune),
        &[],
    );
    assert_equivalent_parse(
        &["img", "prune", "--all", "--preview"],
        &["image", "prune", "--all", "--preview"],
        ModeAction::Image(ImageAction::Prune),
        &[],
    );
    assert_equivalent_parse(
        &["image"],
        &["image", "help"],
        ModeAction::Image(ImageAction::Help),
        &[],
    );
    assert_equivalent_parse(
        &["init"],
        &["init", "create"],
        ModeAction::Init(InitAction::Create),
        &[],
    );
    assert_equivalent_parse(
        &["log", "follow", "example-app/db"],
        &["logs", "follow", "example-app/db"],
        ModeAction::Logs(LogsAction::Follow),
        &["example-app/db"],
    );
    assert_equivalent_parse(
        &["logs"],
        &["logs", "help"],
        ModeAction::Logs(LogsAction::Help),
        &[],
    );
    assert_equivalent_parse(
        &["rec", "--all"],
        &["recipe", "list", "--all"],
        ModeAction::Recipe(RecipeAction::List),
        &[],
    );
    assert_equivalent_parse(
        &["rec", "example-app"],
        &["recipe", "show", "example-app"],
        ModeAction::Recipe(RecipeAction::Show),
        &["example-app"],
    );

    assert_parse(
        &["command", "example-app", "1"],
        ModeAction::Command(CommandAction::Exec),
        &["example-app", "1"],
    );
    assert!(
        !assert_parse(
            &["config", "show"],
            ModeAction::Config(ConfigAction::Show),
            &[]
        )
        .requires_config()
    );
    assert_parse(
        &["cfg", "edit"],
        ModeAction::Config(ConfigAction::Edit),
        &[],
    );
    assert_parse(
        &["config", "help"],
        ModeAction::Config(ConfigAction::Help),
        &[],
    );
    assert_parse(
        &["img", "update", "@backend"],
        ModeAction::Image(ImageAction::Update),
        &["@backend"],
    );
    assert_parse(
        &["image", "update", "buce/app", "buce/1"],
        ModeAction::Image(ImageAction::Update),
        &["buce/app", "buce/1"],
    );
    assert_parse(&["init", "help"], ModeAction::Init(InitAction::Help), &[]);
    assert_parse(
        &["log", "example-app"],
        ModeAction::Logs(LogsAction::Show),
        &["example-app"],
    );
    assert_parse(
        &["recipe", "help"],
        ModeAction::Recipe(RecipeAction::Help),
        &[],
    );
    assert_parse(
        &["recipe", "edit", "example-app"],
        ModeAction::Recipe(RecipeAction::Edit),
        &["example-app"],
    );
    assert_parse(
        &["recipe", "inspect", "example-app"],
        ModeAction::Recipe(RecipeAction::Inspect),
        &["example-app"],
    );

    let mut image_update_mixed = Context::new();
    let image_mixed_err = parse_args(
        &mut image_update_mixed,
        ["image", "update", "@backend", "buce/app"],
    )
    .unwrap_err();
    assert_eq!(image_mixed_err.kind, CliErrorKind::MixedTargets);

    let mut ini_context = Context::new();
    let ini_error = parse_args(&mut ini_context, ["ini"]).unwrap_err();
    assert_eq!(ini_error.kind, CliErrorKind::UnknownAction);
    assert_eq!(ini_error.argument.as_deref(), Some("ini"));
}

#[test]
fn red_team_target_injection_and_bypass_exploits() {
    let mut context = Context::new();
    let group_in_connect = parse_args(&mut context, ["connect", "@backend"]).unwrap_err();
    assert_eq!(group_in_connect.kind, CliErrorKind::InvalidTarget);

    let group_with_container = parse_args(&mut context, ["up", "@backend/db"]).unwrap_err();
    assert_eq!(group_with_container.kind, CliErrorKind::InvalidTarget);

    let slash_in_recipe = parse_args(&mut context, ["recipe", "show", "app/db"]).unwrap_err();
    assert_eq!(slash_in_recipe.kind, CliErrorKind::InvalidTarget);

    let too_many_config = parse_args(&mut context, ["config", "show", "extra"]).unwrap_err();
    assert_eq!(too_many_config.kind, CliErrorKind::TooManyArguments);

    for invalid_target in [
        ".",
        "..",
        "@.",
        "@..",
        "@--all",
        "@-rf",
        "@@backend",
        "app\0evil",
        "app\nevil",
        "app name",
        " app",
    ] {
        let mut target_context = Context::new();
        let error = parse_args(&mut target_context, ["up", invalid_target]).unwrap_err();
        assert_eq!(
            error.kind,
            CliErrorKind::InvalidTarget,
            "expected InvalidTarget for default target {invalid_target:?}"
        );
    }

    for invalid_container in [
        ".",
        "..",
        "./db",
        "../db",
        "../..",
        "app/.",
        "app/..",
        "app/--privileged",
        "app/-f",
        "app/@db",
        "app/db\0",
        "app/db\n",
        "app/db name",
    ] {
        let mut connect_context = Context::new();
        let connect_error =
            parse_args(&mut connect_context, ["connect", invalid_container]).unwrap_err();
        assert_eq!(
            connect_error.kind,
            CliErrorKind::InvalidTarget,
            "expected InvalidTarget for connect target {invalid_container:?}"
        );

        let mut log_context = Context::new();
        let log_error = parse_args(&mut log_context, ["logs", invalid_container]).unwrap_err();
        assert_eq!(
            log_error.kind,
            CliErrorKind::InvalidTarget,
            "expected InvalidTarget for logs target {invalid_container:?}"
        );
    }

    for invalid_recipe in [".", "..", "app\0", "app\n", "app name"] {
        let mut recipe_context = Context::new();
        let recipe_error =
            parse_args(&mut recipe_context, ["recipe", "edit", invalid_recipe]).unwrap_err();
        assert_eq!(recipe_error.kind, CliErrorKind::InvalidTarget);
    }

    for invalid_command in [".", "..", "cmd/1", "migrate\0", "migrate\n", "migrate now"] {
        let mut command_context = Context::new();
        let command_error = parse_args(
            &mut command_context,
            ["command", "exec", "example-app", invalid_command],
        )
        .unwrap_err();
        assert_eq!(command_error.kind, CliErrorKind::InvalidTarget);
    }

    let mut help_extra = Context::new();
    let extra_error = parse_args(&mut help_extra, ["help", "../../etc/passwd"]).unwrap_err();
    assert_eq!(extra_error.kind, CliErrorKind::TooManyArguments);

    let mut version_extra = Context::new();
    let version_extra_error = parse_args(&mut version_extra, ["version", "extra"]).unwrap_err();
    assert_eq!(version_extra_error.kind, CliErrorKind::TooManyArguments);

    let mut status_help_flag = Context::new();
    let status_flag_error = parse_args(&mut status_help_flag, ["status", "--help"]).unwrap_err();
    assert_eq!(status_flag_error.kind, CliErrorKind::UnknownFlag);

    for missing_target_args in [
        ["command", "exec"].as_slice(),
        ["command", "exec", "example-app"].as_slice(),
        ["command", "list"].as_slice(),
        ["connect", "shell"].as_slice(),
        ["image", "update"].as_slice(),
        ["logs", "show"].as_slice(),
        ["logs", "follow"].as_slice(),
        ["recipe", "show"].as_slice(),
        ["recipe", "edit"].as_slice(),
        ["recipe", "inspect"].as_slice(),
    ] {
        let mut missing_context = Context::new();
        let error = parse_args(&mut missing_context, missing_target_args).unwrap_err();
        assert_eq!(error.kind, CliErrorKind::MissingTarget);
    }

    let mut bad_config = Context::new();
    let config_error = parse_args(&mut bad_config, ["config", "bogus"]).unwrap_err();
    assert_eq!(config_error.kind, CliErrorKind::UnknownAction);

    let mut bad_init = Context::new();
    let init_error = parse_args(&mut bad_init, ["init", "bogus"]).unwrap_err();
    assert_eq!(init_error.kind, CliErrorKind::UnknownAction);

    let (default_mode_action, default_parameters) =
        positionals::parse_mode_action(Mode::Default, vec![String::from("status")]).unwrap();
    assert_eq!(
        default_mode_action,
        ModeAction::Default(DefaultAction::Status)
    );
    assert!(default_parameters.is_empty());

    let implicit_empty = positionals::parse_command_implicit(Vec::new()).unwrap_err();
    assert_eq!(implicit_empty.kind, CliErrorKind::MissingTarget);
}
