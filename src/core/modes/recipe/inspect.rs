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

//! Handler for `pods recipe inspect <recipe>`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::core::recipe::Recipe;
use crate::error::PodError;
use super::extract_registered_recipe;

/// Loads and validates a single registered `.pod` recipe file, printing a confirmation message
/// when valid or returning a diagnostic error when invalid.
///
/// # Workflow
///
/// 1. Extract `<recipe>` (`context.parameters[0]`) and verify that `<recipe>` belongs to at
///    least one group in `context.config.recipes.groups`.
/// 2. Load and validate `<recipes.path>/<recipe>.pod` via [`Recipe::inspect`].
/// 3. Write `Recipe '<recipe>' is valid.` to `stdout` when validation succeeds.
///
/// # Errors
///
/// Returns [`PodError`] if `<recipe>` is missing, invalid, or not registered in `pods.conf`,
/// or if [`Recipe::inspect`] reports a file or PSL validation error.
pub fn handle(context: &Context) -> Result<(), PodError> {
    inspect_with(context, &mut io::stdout().lock())
}

/// Validates `<recipe>` via [`Recipe::inspect`] and writes the success status to `stdout`.
fn inspect_with<W: Write>(context: &Context, stdout: &mut W) -> Result<(), PodError> {
    let recipe_name = extract_registered_recipe(context)?;
    Recipe::inspect(&context.config, recipe_name)?;
    log::write_print(stdout, &format!("Recipe '{recipe_name}' is valid."));
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
    use super::{handle, inspect_with};

    const VALID_RECIPE: &str = "\
pod nginx {
    name \"Nginx Proxy\"
    registry docker.io
    path /pods/nginx
}
container *app {
    image nginx:latest
}
";

    fn create_temp_test_dir(suffix: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "podoxide_recipe_inspect_{}_{suffix}",
            process::id()
        ));
        let _ = std_fs::remove_dir_all(&dir);
        std_fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn inspect_with_validates_recipe_and_reports_diagnostic_errors() {
        let temp_dir = create_temp_test_dir("inspect");
        fs::write_file(&temp_dir.join("nginx.pod"), VALID_RECIPE).unwrap();
        fs::write_file(
            &temp_dir.join("broken.pod"),
            "pod broken {\n    name broken\n    detach true\n}\n",
        )
        .unwrap();

        let mut context = Context::new();
        context.config.recipes.path = temp_dir.clone();
        context.config.recipes.groups.insert(
            String::from("all"),
            vec![
                String::from("nginx"),
                String::from("broken"),
                String::from("missing"),
            ],
        );

        let mut stdout = Vec::new();
        let missing_target = inspect_with(&context, &mut stdout).unwrap_err();
        match missing_target {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("unregistered")];
        let unregistered = inspect_with(&context, &mut stdout).unwrap_err();
        match unregistered {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("missing")];
        let not_found = inspect_with(&context, &mut stdout).unwrap_err();
        match not_found {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::NotFound);
            }
            other => panic!("expected PodError::Recipe(NotFound), got {other:?}"),
        }

        context.parameters = vec![String::from("broken")];
        let broken_error = inspect_with(&context, &mut stdout).unwrap_err();
        match broken_error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::UnknownDirective);
                assert!(recipe_error.to_string().contains("detach"));
            }
            other => panic!("expected PodError::Recipe(UnknownDirective), got {other:?}"),
        }

        context.parameters = vec![String::from("nginx")];
        inspect_with(&context, &mut stdout).unwrap();
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "Recipe 'nginx' is valid.\n"
        );
        assert!(handle(&context).is_ok());

        let _ = std_fs::remove_dir_all(&temp_dir);
    }
}
