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

//! Handler for `pods recipe [show] <recipe>`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::core::recipe::Recipe;
use crate::error::PodError;
use super::{build_recipe_path, extract_registered_recipe};

/// Reads a single `.pod` recipe file and outputs its raw contents prefixed with 1-based line
/// numbers.
///
/// # Workflow
///
/// 1. Extract `<recipe>` (`context.parameters[0]`) and verify that `<recipe>` belongs to at
///    least one group in `context.config.recipes.groups`.
/// 2. Construct `<recipes.path>/<recipe>.pod` and read only the raw text lines of that single
///    file. Do not parse or validate the file into a PSL AST, and do not read any other
///    `.pod` files.
/// 3. Print each line prefixed with its 1-based line number via [`log::write_numbered_lines`].
///
/// # Errors
///
/// Returns [`PodError`] if `<recipe>` is not registered in `pods.conf` or if reading the
/// `.pod` file from disk fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    show_with(context, &mut io::stdout().lock())
}

/// Validates the target recipe, reads the raw `.pod` file, and writes numbered lines to `stdout`.
///
/// # Errors
///
/// Returns [`PodError`] if `<recipe>` is missing, invalid, or not registered in `pods.conf`,
/// `context.config.recipes.path` is invalid, or reading `<recipes.path>/<recipe>.pod` fails.
fn show_with<W: Write>(context: &Context, stdout: &mut W) -> Result<(), PodError> {
    let recipe_name = extract_registered_recipe(context)?;
    let recipe_path = build_recipe_path(&context.config.recipes.path, recipe_name)?;
    let content = Recipe::read_file(&recipe_path)?;
    log::write_numbered_lines(stdout, &content);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs as std_fs;
    use std::path::PathBuf;
    use std::process;

    use crate::core::context::Context;
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError, RecipeErrorKind};
    use crate::io::system::fs;
    use super::{handle, show_with};

    fn create_temp_test_dir(suffix: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "podoxide_recipe_show_{}_{suffix}",
            process::id()
        ));
        let _ = std_fs::remove_dir_all(&dir);
        std_fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn show_with_outputs_numbered_lines_without_parsing_psl() {
        let temp_dir = create_temp_test_dir("show_lines");
        // Write raw non-PSL content to prove show_with never parses the .pod file into an AST.
        fs::write_file(
            &temp_dir.join("recipe_002_no_pod.pod"),
            "return {\n    name = \"nopod\",\n}\n",
        )
        .unwrap();

        let mut context = Context::new();
        context.config.recipes.path = temp_dir.clone();
        context.config.recipes.groups.insert(
            String::from("all"),
            vec![
                String::from("recipe_002_no_pod"),
                String::from("recipe_000_unknown"),
            ],
        );
        context.parameters = vec![String::from("recipe_002_no_pod")];

        let mut stdout = Vec::new();
        show_with(&context, &mut stdout).unwrap();
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "  1: return {\n  2:     name = \"nopod\",\n  3: }\n"
        );

        assert!(handle(&context).is_ok());

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn show_with_rejects_missing_target_unregistered_recipe_and_missing_or_non_regular_file() {
        let temp_dir = create_temp_test_dir("errors");
        let dir_as_recipe = temp_dir.join("dir_recipe.pod");
        std_fs::create_dir_all(&dir_as_recipe).unwrap();

        let mut context = Context::new();
        context.config.recipes.path = temp_dir.clone();
        context.config.recipes.groups.insert(
            String::from("all"),
            vec![
                String::from("recipe_000_unknown"),
                String::from("dir_recipe"),
            ],
        );

        let mut stdout = Vec::new();
        let missing_target = show_with(&context, &mut stdout).unwrap_err();
        match missing_target {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("unknown")];
        let unregistered = show_with(&context, &mut stdout).unwrap_err();
        match unregistered {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe_000_unknown")];
        let missing_file = show_with(&context, &mut stdout).unwrap_err();
        match missing_file {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::NotFound);
            }
            other => panic!("expected PodError::Recipe(NotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("dir_recipe")];
        let non_regular = show_with(&context, &mut stdout).unwrap_err();
        assert!(matches!(non_regular, PodError::Io(_)));

        let _ = std_fs::remove_dir_all(&temp_dir);
    }
}
