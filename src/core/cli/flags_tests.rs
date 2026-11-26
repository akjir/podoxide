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

//! Unit tests for CLI flag scanning, value validation, and action-aware flag checks.

use std::borrow::Cow;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use crate::core::config;
use crate::core::context::{Context, DefaultAction, Flags, Mode, ModeAction};
use crate::error::CliErrorKind;
use super::{MAX_CLI_ARGS, flags, parse_args};

fn assert_invalid_config_path(invalid_config: &str) {
    let mut equals_context = Context::new();
    let equals_flag = format!("--config={invalid_config}");
    let equals_error =
        parse_args(&mut equals_context, [equals_flag.as_str(), "status"]).unwrap_err();
    assert_eq!(
        equals_error.kind,
        CliErrorKind::InvalidFlagValue,
        "expected InvalidFlagValue for {equals_flag:?}"
    );
    assert_eq!(equals_error.argument.as_deref(), Some("--config"));

    if !invalid_config.starts_with('-') {
        let mut separated_context = Context::new();
        let separated_error = parse_args(
            &mut separated_context,
            ["--config", invalid_config, "status"],
        )
        .unwrap_err();
        assert_eq!(
            separated_error.kind,
            CliErrorKind::InvalidFlagValue,
            "expected InvalidFlagValue for --config {invalid_config:?}"
        );
        assert_eq!(separated_error.argument.as_deref(), Some("--config"));
    }
}

#[test]
fn action_aware_flag_validation_rejects_invalid_combinations() {
    let rejected_cases: &[(&[&str], &str)] = &[
        (&["up", "--all", "recipe"], "--all"),
        (&["up", "--full", "recipe"], "--full"),
        (&["version", "--all"], "--all"),
        (&["status", "--all", "recipe"], "--all"),
        (&["image", "update", "--all", "recipe"], "--all"),
        (&["image", "update", "--force", "recipe"], "--force"),
        (&["image", "update", "--preview", "recipe"], "--preview"),
        (&["image", "prune", "--preview", "--force"], "--force"),
        (&["image", "help", "--all"], "--all"),
        (&["recipe", "show", "--all", "recipe"], "--all"),
        (&["recipe", "edit", "--all", "recipe"], "--all"),
        (&["recipe", "inspect", "--all", "recipe"], "--all"),
        (&["recipe", "help", "--all"], "--all"),
        (&["logs", "help", "--tail=10"], "--tail"),
        (&["logs", "help", "--timestamps"], "--timestamps"),
        (&["logs", "help", "--since=1h"], "--since"),
        (&["logs", "help", "--until=10m"], "--until"),
    ];

    for (arguments, expected_flag) in rejected_cases {
        let mut context = Context::new();
        let error = parse_args(&mut context, *arguments).unwrap_err();
        assert_eq!(
            error.kind,
            CliErrorKind::UnexpectedFlag,
            "expected UnexpectedFlag for {arguments:?}"
        );
        assert_eq!(
            error.argument.as_deref(),
            Some(*expected_flag),
            "expected offending flag {expected_flag:?} for {arguments:?}"
        );
    }
}

#[test]
fn argument_count_bounds_and_invalid_utf8() {
    let mut max_arguments = Vec::with_capacity(MAX_CLI_ARGS);
    max_arguments.push(OsString::from("up"));
    for index in 1..MAX_CLI_ARGS {
        max_arguments.push(OsString::from(format!("recipe-{index}")));
    }
    let mut valid_max = Context::new();
    parse_args(&mut valid_max, max_arguments.clone()).unwrap();
    assert_eq!(valid_max.parameters.len(), MAX_CLI_ARGS - 1);

    max_arguments.push(OsString::from("overflow-recipe"));
    let mut exceeded_context = Context::new();
    let exceeded = parse_args(&mut exceeded_context, max_arguments).unwrap_err();
    assert_eq!(exceeded.kind, CliErrorKind::LimitExceeded);

    let mut flag_boundary = Vec::with_capacity(MAX_CLI_ARGS + 1);
    flag_boundary.push(OsString::from("up"));
    for index in 1..(MAX_CLI_ARGS - 1) {
        flag_boundary.push(OsString::from(format!("recipe-{index}")));
    }
    flag_boundary.push(OsString::from("--config"));
    flag_boundary.push(OsString::from("staging"));
    assert_eq!(flag_boundary.len(), MAX_CLI_ARGS + 1);
    let mut flag_exceeded_context = Context::new();
    let flag_exceeded = parse_args(&mut flag_exceeded_context, flag_boundary).unwrap_err();
    assert_eq!(flag_exceeded.kind, CliErrorKind::LimitExceeded);

    let invalid_utf8 = OsString::from_vec(vec![b'u', b'p', 0xff, 0xfe]);
    let mut utf8_context = Context::new();
    let utf8_error = parse_args(&mut utf8_context, [invalid_utf8]).unwrap_err();
    assert_eq!(utf8_error.kind, CliErrorKind::InvalidUtf8);

    let invalid_flag_utf8 = OsString::from_vec(vec![0xff, 0xfe]);
    let mut flag_utf8_context = Context::new();
    let flag_utf8_error = parse_args(
        &mut flag_utf8_context,
        [
            OsString::from("--config"),
            invalid_flag_utf8,
            OsString::from("status"),
        ],
    )
    .unwrap_err();
    assert_eq!(flag_utf8_error.kind, CliErrorKind::InvalidUtf8);
}

#[test]
fn config_path_edge_cases_and_resolution() {
    assert_eq!(config::resolve_config_name(None), "pods");
    assert_eq!(
        config::resolve_config_path(None),
        PathBuf::from("./pods.conf")
    );
    assert_eq!(
        config::resolve_config_name(Some(Path::new("staging"))),
        "staging"
    );
    assert_eq!(
        config::resolve_config_path(Some(Path::new("staging"))),
        PathBuf::from("./staging.conf")
    );
    assert_eq!(
        config::resolve_config_name(Some(Path::new(".hidden"))),
        ".hidden"
    );
    assert_eq!(
        config::resolve_config_path(Some(Path::new(".hidden"))),
        PathBuf::from("./.hidden.conf")
    );
    assert_eq!(
        config::resolve_config_name(Some(Path::new("./.hidden.conf"))),
        ".hidden"
    );
    assert_eq!(
        config::resolve_config_path(Some(Path::new("./.hidden.conf"))),
        PathBuf::from("./.hidden.conf")
    );
    assert_eq!(
        config::resolve_config_name(Some(Path::new("../my.multi.ext"))),
        "my.multi.ext"
    );
    assert_eq!(
        config::resolve_config_path(Some(Path::new("../my.multi.ext"))),
        PathBuf::from("../my.multi.ext.conf")
    );
    assert_eq!(
        config::resolve_config_name(Some(Path::new("/etc/pods/my.multi.ext.conf"))),
        "my.multi.ext"
    );
    assert_eq!(
        config::resolve_config_path(Some(Path::new("/etc/pods/my.multi.ext.conf"))),
        PathBuf::from("/etc/pods/my.multi.ext.conf")
    );

    let mut hidden_context = Context::new();
    parse_args(&mut hidden_context, ["--config=.hidden", "status"]).unwrap();
    assert_eq!(hidden_context.config.name, ".hidden");
    assert_eq!(hidden_context.config.path, PathBuf::from("./.hidden.conf"));

    for invalid_path in [".", "..", "/", "./", "../", "configs/", "   "] {
        let mut context = Context::new();
        let error = parse_args(&mut context, ["--config", invalid_path, "status"]).unwrap_err();
        assert_eq!(
            error.kind,
            CliErrorKind::InvalidFlagValue,
            "expected InvalidFlagValue for --config {invalid_path:?}"
        );
    }
}

#[test]
fn flags_equals_separated_and_rejections() {
    let mut logs_equals = Context::new();
    parse_args(
        &mut logs_equals,
        [
            "--config=staging",
            "--debug",
            "logs",
            "--tail=100",
            "--since=1h",
            "--until=10m",
            "--timestamps",
            "show",
            "example-app/1",
            "--simulate",
        ],
    )
    .unwrap();
    let mut logs_separated = Context::new();
    parse_args(
        &mut logs_separated,
        [
            "--config",
            "staging",
            "--debug",
            "log",
            "--tail",
            "100",
            "--since",
            "1h",
            "--until",
            "10m",
            "--timestamps",
            "show",
            "example-app/1",
            "--simulate",
        ],
    )
    .unwrap();
    assert_eq!(logs_equals, logs_separated);
    assert_eq!(logs_equals.flags.config, Some(PathBuf::from("staging")));
    assert_eq!(logs_equals.config.name, "staging");
    assert_eq!(logs_equals.config.path, PathBuf::from("./staging.conf"));
    assert!(logs_equals.flags.debug());
    assert!(logs_equals.flags.simulate());
    assert!(logs_equals.config.simulate);
    assert_eq!(logs_equals.flags.tail.as_deref(), Some("100"));
    assert_eq!(logs_equals.flags.since.as_deref(), Some("1h"));
    assert_eq!(logs_equals.flags.until.as_deref(), Some("10m"));
    assert!(logs_equals.flags.timestamps());
    assert_eq!(logs_equals.parameters, vec!["example-app/1"]);

    let mut version_action = Context::new();
    parse_args(&mut version_action, ["version"]).unwrap();
    assert_eq!(version_action.mode(), Mode::Default);
    assert_eq!(
        version_action.action,
        ModeAction::Default(DefaultAction::Version)
    );
    assert!(!version_action.requires_config());

    for empty_arg in [
        "--config=",
        "--config=\"\"",
        "--config=''",
        "--tail=",
        "--tail=\"\"",
    ] {
        let mut context = Context::new();
        let error = parse_args(&mut context, ["logs", empty_arg, "example-app"]).unwrap_err();
        assert_eq!(error.kind, CliErrorKind::EmptyFlagValue);
    }

    let mut context = Context::new();
    let empty_separated =
        parse_args(&mut context, ["--config", "", "up", "example-app"]).unwrap_err();
    assert_eq!(empty_separated.kind, CliErrorKind::EmptyFlagValue);

    for missing_flag in ["--config", "--tail", "--since", "--until"] {
        let mut missing_context = Context::new();
        let error =
            parse_args(&mut missing_context, ["logs", "example-app", missing_flag]).unwrap_err();
        assert_eq!(error.kind, CliErrorKind::MissingFlagValue);
    }

    for unknown_flag in [
        "--unknown",
        "--foo=bar",
        "--help",
        "-c",
        "-c=staging",
        "-h",
        "--version",
        "-v",
        "-x",
        "-",
    ] {
        let mut unknown_context = Context::new();
        let error = parse_args(&mut unknown_context, [unknown_flag]).unwrap_err();
        assert_eq!(error.kind, CliErrorKind::UnknownFlag);
    }

    for (mode_arg, bad_flag) in [
        ("init", "--config=staging"),
        ("logs", "--all"),
        ("recipe", "--full"),
        ("status", "--force"),
        ("status", "--preview"),
        ("status", "--since=1h"),
        ("status", "--tail=10"),
        ("status", "--timestamps"),
        ("status", "--until=10m"),
    ] {
        let mut bad_context = Context::new();
        let error = parse_args(&mut bad_context, [mode_arg, bad_flag]).unwrap_err();
        assert_eq!(error.kind, CliErrorKind::UnexpectedFlag);
    }

    let fallback_error =
        flags::apply_flag_value(&mut Flags::new(), "--unknown", Cow::Borrowed("value"))
            .unwrap_err();
    assert_eq!(fallback_error.kind, CliErrorKind::UnknownFlag);
}

#[test]
fn log_flag_values_enforce_tail_grammar_and_space_timestamps() {
    let mut invalid_tail_context = Context::new();
    let invalid_tail =
        parse_args(&mut invalid_tail_context, ["logs", "--tail=abc", "recipe"]).unwrap_err();
    assert_eq!(invalid_tail.kind, CliErrorKind::InvalidFlagValue);
    assert_eq!(invalid_tail.argument.as_deref(), Some("--tail"));

    for valid_tail in ["0", "25", "all", "ALL", "All"] {
        let mut context = Context::new();
        parse_args(
            &mut context,
            ["logs", &format!("--tail={valid_tail}"), "recipe"],
        )
        .unwrap();
        assert_eq!(context.flags.tail.as_deref(), Some(valid_tail));
    }

    let mut timestamp_context = Context::new();
    parse_args(
        &mut timestamp_context,
        [
            "logs",
            "--since=2026-01-01 12:00:00",
            "--until=2026-01-01 13:00:00",
            "recipe",
        ],
    )
    .unwrap();
    assert_eq!(
        timestamp_context.flags.since.as_deref(),
        Some("2026-01-01 12:00:00")
    );
    assert_eq!(
        timestamp_context.flags.until.as_deref(),
        Some("2026-01-01 13:00:00")
    );

    for leading_dash in ["--since=-1h", "--since= -1h", "--until=-10m", "--until= -f"] {
        let mut dash_context = Context::new();
        let error = parse_args(&mut dash_context, ["logs", leading_dash, "recipe"]).unwrap_err();
        assert_eq!(
            error.kind,
            CliErrorKind::InvalidFlagValue,
            "expected InvalidFlagValue for {leading_dash:?}"
        );
    }
}

#[test]
fn parse_equals_flag_reports_consistent_flag_argument_subject() {
    let cases: &[(&[&str], &str)] = &[
        (&["--debug=true", "status"], "--debug"),
        (&["--all=1", "status"], "--all"),
        (&["--config=../bad", "status"], "--config"),
        (&["logs", "--tail=abc", "recipe"], "--tail"),
        (&["logs", "--since=-f", "recipe"], "--since"),
        (&["logs", "--until=-f", "recipe"], "--until"),
    ];
    for (arguments, expected_flag) in cases {
        let mut context = Context::new();
        let error = parse_args(&mut context, *arguments).unwrap_err();
        assert_eq!(
            error.kind,
            CliErrorKind::InvalidFlagValue,
            "expected InvalidFlagValue for {arguments:?}"
        );
        assert_eq!(
            error.argument.as_deref(),
            Some(*expected_flag),
            "expected consistent flag subject {expected_flag:?} for {arguments:?}"
        );
    }
}

#[test]
fn red_team_config_path_and_flag_smuggling_exploits() {
    for smuggled_flag in [
        ["--config", "--simulate", "down", "example-app"].as_slice(),
        ["logs", "--tail", "--simulate", "example-app"].as_slice(),
        ["logs", "--since", "--all", "example-app"].as_slice(),
        ["logs", "--until", "--timestamps", "example-app"].as_slice(),
    ] {
        let mut context = Context::new();
        let error = parse_args(&mut context, smuggled_flag).unwrap_err();
        assert_eq!(error.kind, CliErrorKind::MissingFlagValue);
    }

    for invalid_log_flag in [
        "--tail=--follow",
        "--tail=-10",
        "--tail=   ",
        "--tail=10\0",
        "--tail=10\n",
        "--tail=abc",
        "--since=--all",
        "--since=-f",
        "--since=\t",
        "--since=1h\0",
        "--until=--help",
        "--until=10m\r",
    ] {
        let mut context = Context::new();
        let error =
            parse_args(&mut context, ["logs", invalid_log_flag, "example-app"]).unwrap_err();
        assert_eq!(
            error.kind,
            CliErrorKind::InvalidFlagValue,
            "expected InvalidFlagValue for {invalid_log_flag:?}"
        );
    }

    for invalid_config in [
        "./.",
        "./..",
        "../..",
        "../pods.conf",
        "configs/../staging",
        "../../etc/passwd",
        "/.",
        "/..",
        "configs/.",
        "configs/..",
        ".conf",
        "./.conf",
        "/etc/.conf",
        "..conf",
        "...conf",
        "-rf",
        "--simulate",
        "configs/-rf",
        " staging",
        "staging ",
        " /etc/pods",
        "staging\0",
        "staging\n",
        "staging\t",
    ] {
        assert_invalid_config_path(invalid_config);
    }

    let invalid_utf8_os = OsString::from_vec(vec![0xff, 0xfe]);
    let invalid_utf8_path = Path::new(&invalid_utf8_os);
    assert_eq!(config::resolve_config_name(Some(invalid_utf8_path)), "pods");
    assert_eq!(
        config::resolve_config_path(Some(invalid_utf8_path)),
        PathBuf::from("./pods.conf")
    );
    assert_eq!(config::resolve_config_name(Some(Path::new("./.."))), "pods");
    assert_eq!(
        config::resolve_config_path(Some(Path::new("./.."))),
        PathBuf::from("./pods.conf")
    );
    assert_eq!(
        config::resolve_config_path(Some(Path::new(".conf"))),
        PathBuf::from("./pods.conf")
    );
}
