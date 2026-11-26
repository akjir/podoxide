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

//! Recipe mode (`pods recipe` / `pods rec`) action dispatcher.

pub mod edit;
pub mod help;
pub mod inspect;
pub mod list;
pub mod show;

use std::path::{Path, PathBuf};

use crate::core::context::{Context, RecipeAction};
use crate::core::log;
use crate::core::recipe::RECIPE_EXTENSION;
use crate::error::{CliError, CliErrorKind, ConfigError, ConfigErrorKind, PodError};
use crate::io::exec::command::ExecOutcome;
use crate::utilities::path::{build_full_path, is_valid_directory, sanitize_path_token};
use crate::utilities::string::is_valid_target_segment;

/// Dispatches `recipe` mode execution to the resolved sub-action (`edit`, `help`, `inspect`,
/// `list`, or `show`).
///
/// # Workflow
///
/// 1. Log that `recipe` mode is active when debug logging is enabled.
/// 2. Route [`RecipeAction`] in enum declaration order (`Edit`, `Help`, `Inspect`, `List`,
///    `Show`) to the corresponding action module ([`edit::handle`], [`help::handle`],
///    [`inspect::handle`], [`list::handle`], [`show::handle`]).
///
/// # Errors
///
/// Returns [`PodError`] if the selected action handler fails.
pub fn handle(context: &Context, action: RecipeAction) -> Result<ExecOutcome, PodError> {
    log::debug("Recipe mode is used.");
    match action {
        RecipeAction::Edit => edit::handle(context),
        RecipeAction::Help => {
            help::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
        RecipeAction::Inspect => {
            inspect::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
        RecipeAction::List => {
            list::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
        RecipeAction::Show => {
            show::handle(context)?;
            Ok(ExecOutcome::Succeeded)
        }
    }
}

/// Validates `recipes_dir` and constructs the `<recipes.path>/<recipe_name>.pod` path.
///
/// # Errors
///
/// Returns [`PodError`] with [`ConfigErrorKind::InvalidPath`] if `recipes_dir` is not a valid
/// directory path.
pub(super) fn build_recipe_path(
    recipes_dir: &Path,
    recipe_name: &str,
) -> Result<PathBuf, PodError> {
    if !is_valid_directory(recipes_dir) {
        return Err(PodError::from(ConfigError::without_line(
            ConfigErrorKind::InvalidPath,
            sanitize_path_token(recipes_dir),
        )));
    }
    Ok(build_full_path(recipes_dir, recipe_name, RECIPE_EXTENSION))
}

/// Extracts the single `<recipe>` target from `context` and verifies that it is registered in
/// `context.config.recipes.groups`.
///
/// # Errors
///
/// Returns [`PodError`] if no target parameter is present, extra parameters are provided, the
/// target is not a valid recipe segment, or `<recipe>` is not registered in `pods.conf`.
pub(super) fn extract_registered_recipe(context: &Context) -> Result<&str, PodError> {
    let Some(target) = context.parameters.first() else {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    };
    if let Some(extra) = context.parameters.get(1) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::TooManyArguments,
            extra,
        )));
    }
    if !is_valid_target_segment(target) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidTarget,
            target,
        )));
    }
    if !context.config.has_recipe(target) {
        return Err(PodError::from(ConfigError::without_line(
            ConfigErrorKind::RecipeNotFound,
            target,
        )));
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::{Path, PathBuf};

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::context::{Context, RecipeAction};
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError};
    use crate::io::exec::command::ExecOutcome;
    use super::{build_recipe_path, extract_registered_recipe, handle};

    #[test]
    fn build_recipe_path_and_extract_registered_recipe_validate_inputs() {
        let mut context = Context::new();
        context.config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        let missing_error = extract_registered_recipe(&context).unwrap_err();
        match missing_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe"), String::from("extra")];
        let extra_error = extract_registered_recipe(&context).unwrap_err();
        match extra_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::TooManyArguments),
            other => panic!("expected PodError::Cli(TooManyArguments), got {other:?}"),
        }

        for invalid_target in ["", "   ", "@all", "recipe/db", "-flag", ".", ".."] {
            context.parameters = vec![String::from(invalid_target)];
            let error = extract_registered_recipe(&context).unwrap_err();
            match error {
                PodError::Cli(cli_error) => {
                    assert_eq!(cli_error.kind, CliErrorKind::InvalidTarget);
                }
                other => panic!(
                    "expected PodError::Cli(InvalidTarget) for {invalid_target:?}, got {other:?}"
                ),
            }
        }

        context.parameters = vec![String::from("unknown")];
        let unknown_error = extract_registered_recipe(&context).unwrap_err();
        match unknown_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe")];
        assert_eq!(extract_registered_recipe(&context).unwrap(), "recipe");
        assert_eq!(
            build_recipe_path(Path::new("."), "recipe").unwrap(),
            PathBuf::from("./recipe.pod")
        );

        let non_utf8_dir = PathBuf::from(OsString::from_vec(vec![b'/', 0xFF]));
        for invalid_dir in [
            Path::new(""),
            Path::new("../recipes"),
            Path::new("-recipes"),
            non_utf8_dir.as_path(),
        ] {
            let error = build_recipe_path(invalid_dir, "recipe").unwrap_err();
            match error {
                PodError::Config(config_error) => {
                    assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
                }
                other => panic!(
                    "expected PodError::Config(InvalidPath) for {invalid_dir:?}, got {other:?}"
                ),
            }
        }
    }

    #[test]
    fn handle_routes_recipe_actions() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        for action in [
            RecipeAction::Edit,
            RecipeAction::Help,
            RecipeAction::Inspect,
            RecipeAction::List,
            RecipeAction::Show,
        ] {
            let mut context = Context::new();
            context.config = root_config.clone();
            if matches!(
                action,
                RecipeAction::Edit | RecipeAction::Inspect | RecipeAction::Show
            ) {
                context.parameters = vec![String::from("recipe")];
            }
            assert_eq!(
                handle(&context, action).unwrap(),
                ExecOutcome::Succeeded,
                "expected recipe::{action} to succeed"
            );
        }

        let mut failing_editor = Context::new();
        failing_editor.config = root_config;
        failing_editor.config.editor = String::from("false");
        failing_editor.config.simulate = false;
        failing_editor.parameters = vec![String::from("recipe")];
        assert_eq!(
            handle(&failing_editor, RecipeAction::Edit).unwrap(),
            ExecOutcome::Failed,
            "recipe edit must propagate non-zero editor exit as ExecOutcome::Failed"
        );
    }
}
