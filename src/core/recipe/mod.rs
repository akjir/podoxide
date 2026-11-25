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

//! Recipe model (`.pod`), PSL recipe loading, container name resolution, and semantic validation.

mod directives;
mod parser;
mod resolve;
pub(crate) mod validation;

use std::path::Path;
use std::str::FromStr;

use crate::core::config::Config;
use crate::core::container::Container;
use crate::core::pod::Pod;
use crate::error::{PodError, RecipeError};
use crate::io::system::fs;
use crate::utilities::path;

/// File extension for PodScript Language recipe files.
pub const RECIPE_EXTENSION: &str = ".pod";

/// Declarative pod and container recipe loaded from a `.pod` file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Recipe {
    pub containers: Vec<Container>,
    pub has_pod: bool,
    pub pod: Pod,
}

impl Recipe {
    /// Creates a new [`Recipe`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates whether `<recipes_dir>/<recipe_name>.pod` is a valid path that exists on disk.
    #[must_use]
    pub fn file_exists(recipes_dir: &Path, recipe_name: &str) -> bool {
        if !parser::is_valid_load_target(recipes_dir, recipe_name) {
            return false;
        }
        fs::file_exists(&path::build_full_path(
            recipes_dir,
            recipe_name,
            RECIPE_EXTENSION,
        ))
    }

    /// Loads, parses, and semantically validates `<recipes.path>/<recipe_name>.pod` against `config`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] when the recipe path is invalid, the file is missing, PSL parsing fails,
    /// or required pod and container fields are missing or invalid.
    pub fn inspect(config: &Config, recipe_name: &str) -> Result<Self, PodError> {
        let (mut recipe, content, display_path) = parser::load(&config.recipes.path, recipe_name)?;
        validation::validate_recipe_inner(&mut recipe, config, recipe_name)
            .map_err(|error| PodError::from(error.with_diagnostic(&content, &display_path)))?;
        Ok(recipe)
    }

    /// Reads the raw `.pod` file at `path`, mapping a missing file to [`RecipeErrorKind::NotFound`].
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] with [`RecipeErrorKind::InvalidPath`] if `path` is not a valid `.pod`
    /// path, [`RecipeErrorKind::NotFound`] if `path` does not exist, or [`PodError::Io`] if reading
    /// the file fails.
    pub fn read_file(path: &Path) -> Result<String, PodError> {
        parser::validate_load_path(path)?;
        parser::read_validated_file(path)
    }

    /// Resolves a container reference (`1..=N`, `*suffix`, or name) to its matching [`Container`] in this recipe.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] with [`RecipeErrorKind::ContainerNotFound`] when no container matches.
    pub fn resolve_container(&self, container_spec: &str) -> Result<&Container, RecipeError> {
        resolve::resolve_container_entry(self, container_spec).map(|(container, _)| container)
    }

    /// Resolves a container reference (`1..=N`, `*suffix`, or name) against this recipe's containers.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] with [`RecipeErrorKind::ContainerNotFound`] when no container matches.
    pub fn resolve_container_name(&self, container_spec: &str) -> Result<String, RecipeError> {
        resolve::resolve_container_entry(self, container_spec).map(|(_, name)| name.into_owned())
    }
}

impl FromStr for Recipe {
    type Err = PodError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        parser::parse_recipe_with_path(input, "<input>")
    }
}

#[cfg(test)]
mod directives_tests;
#[cfg(test)]
mod parser_tests;
#[cfg(test)]
mod resolve_tests;
#[cfg(test)]
mod validation_tests;
