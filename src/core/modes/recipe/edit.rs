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

//! Handler for `pods recipe edit <recipe>`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::error::PodError;
use crate::io::exec::command;
use crate::utilities::path::validate_editor;
use super::{build_recipe_path, extract_registered_recipe};

/// Opens the specified `.pod` recipe file in the external editor configured in `pods.conf`.
///
/// # Workflow
///
/// 1. Extract `<recipe>` (`context.parameters[0]`) and verify that `<recipe>` belongs to at
///    least one group in `context.config.recipes.groups`.
/// 2. Verify that `context.config.editor` is non-empty; return an error if no editor is
///    configured.
/// 3. Construct the target path `<recipes.path>/<recipe>.pod` without reading or parsing the
///    `.pod` file contents.
/// 4. Execute `<editor> <full_path>` interactively via `src/io/exec/command.rs` (or print
///    the command without executing it when `context.config.simulate` is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if `<recipe>` is not registered in `pods.conf`, `general.editor` is
/// empty, or spawning the editor process fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    edit_with(context, &mut stdout, &mut stderr)
}

/// Validates the target recipe, configured editor, and recipes directory, then executes or
/// simulates `<editor> <recipes.path>/<recipe>.pod`.
///
/// # Errors
///
/// Returns [`PodError`] if `<recipe>` is missing, invalid, or not registered in `pods.conf`,
/// `context.config.editor` is missing or invalid, `context.config.recipes.path` is invalid, or
/// the editor command fails.
fn edit_with<W: Write, E: Write>(
    context: &Context,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    let recipe_name = extract_registered_recipe(context)?;
    let editor = validate_editor(&context.config.editor)?;
    let recipe_path = build_recipe_path(&context.config.recipes.path, recipe_name)?;
    let recipe_argument = recipe_path.to_string_lossy();
    command::exec_editor_with(
        editor,
        recipe_argument.as_ref(),
        context.config.simulate,
        stdout,
        stderr,
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::core::context::Context;
    use crate::core::log;
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError};
    use super::{edit_with, handle};

    fn build_edit_context() -> Context {
        let mut context = Context::new();
        context.config.editor = String::from("vim");
        context.config.recipes.path = PathBuf::from("./tests/pods/recipes");
        context.config.recipes.groups.insert(
            String::from("all"),
            vec![String::from("recipe_002_no_pod")],
        );
        context.config.simulate = true;
        context.parameters = vec![String::from("recipe_002_no_pod")];
        context
    }

    #[test]
    fn edit_with_executes_and_logs_debug_when_not_simulated() {
        let mut context = build_edit_context();
        context.config.editor = String::from("false");
        context.config.simulate = false;

        log::set_debug_enabled(true);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        edit_with(&context, &mut stdout, &mut stderr).unwrap();
        log::set_debug_enabled(false);

        assert!(stdout.is_empty());
        assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "DEBUG: Execute: false ./tests/pods/recipes/recipe_002_no_pod.pod\nWARNING: Command exited with code '1'!\n"
        );

        context.config.editor = String::from("true");
        let mut ok_stdout = Vec::new();
        let mut ok_stderr = Vec::new();
        edit_with(&context, &mut ok_stdout, &mut ok_stderr).unwrap();
        assert!(ok_stdout.is_empty());
        assert!(ok_stderr.is_empty());
    }

    #[test]
    fn edit_with_rejects_missing_target_unregistered_recipe_and_invalid_editor_or_path() {
        let mut context = build_edit_context();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        context.parameters.clear();
        let missing_target = edit_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match missing_target {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("unknown")];
        let unknown_recipe = edit_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match unknown_recipe {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe_002_no_pod")];
        context.config.editor = String::new();
        let missing_editor = edit_with(&context, &mut stdout, &mut stderr).unwrap_err();
        match missing_editor {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::MissingEditor);
            }
            other => panic!("expected PodError::Config(MissingEditor), got {other:?}"),
        }

        for invalid_editor in ["-vim", "../vim", "vim -u NONE", "vim\n"] {
            context.config.editor = String::from(invalid_editor);
            let error = edit_with(&context, &mut stdout, &mut stderr).unwrap_err();
            match error {
                PodError::Config(config_error) => {
                    assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
                }
                other => panic!(
                    "expected PodError::Config(InvalidPath) for {invalid_editor:?}, got {other:?}"
                ),
            }
        }

        context.config.editor = String::from("vim");
        for invalid_recipes_path in ["", "../recipes", "-recipes"] {
            context.config.recipes.path = PathBuf::from(invalid_recipes_path);
            let error = edit_with(&context, &mut stdout, &mut stderr).unwrap_err();
            match error {
                PodError::Config(config_error) => {
                    assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
                }
                other => panic!(
                    "expected PodError::Config(InvalidPath) for {invalid_recipes_path:?}, got {other:?}"
                ),
            }
        }
    }

    #[test]
    fn edit_with_simulates_editor_command_and_escapes_paths() {
        let mut context = build_edit_context();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        edit_with(&context, &mut stdout, &mut stderr).unwrap();
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "vim ./tests/pods/recipes/recipe_002_no_pod.pod\n"
        );
        assert!(stderr.is_empty());

        context.config.recipes.path = PathBuf::from("./my recipes/O'Hara");
        let mut quoted_stdout = Vec::new();
        edit_with(&context, &mut quoted_stdout, &mut stderr).unwrap();
        assert_eq!(
            String::from_utf8(quoted_stdout).unwrap(),
            "vim './my recipes/O'\\''Hara/recipe_002_no_pod.pod'\n"
        );

        context.config.recipes.path = PathBuf::from(".");
        assert!(handle(&context).is_ok());
    }
}
