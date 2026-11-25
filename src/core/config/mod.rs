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

//! Configuration model (`pods.conf`), PSL configuration loading, path resolution, and target expansion.

mod parser;
mod path;
mod targets;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::error::PodError;
use crate::utilities::path as path_util;
pub use self::path::{extract_config_stem, resolve_config_name, resolve_config_path, validate_editor};

/// Default configuration name when `--config` is not specified.
pub const DEFAULT_CONFIG_NAME: &str = "pods";
/// Default configuration file path when `--config` is not specified.
pub const DEFAULT_CONFIG_PATH: &str = "./pods.conf";
/// Default recipe directory path when not specified in `pods.conf`.
pub const DEFAULT_RECIPES_PATH: &str = ".";
/// Maximum nesting depth allowed when expanding nested `@<group>` references.
pub const MAX_GROUP_EXPANSION_DEPTH: usize = 64;

/// Runtime configuration settings loaded from `pods.conf`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub editor: String,
    pub name: String,
    pub path: PathBuf,
    pub pods: PodsConfig,
    pub recipes: RecipesConfig,
    pub simulate: bool,
}

impl Config {
    /// Creates a new [`Config`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Expands `@<group>` and `<recipe>` targets against `self.recipes.groups`, preserving first-appearance order and deduplicating repeated recipes.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if an `@<group>` target does not exist in `self.recipes.groups`, a group contains a cycle, exceeds [`MAX_GROUP_EXPANSION_DEPTH`], or contains an invalid target, or a `<recipe>` target is not referenced in any configured group.
    pub fn expand_targets(&self, targets: &[String]) -> Result<Vec<String>, PodError> {
        targets::expand_targets(self, targets)
    }

    /// Collects the sorted set of unique recipe names referenced across all groups in `self.recipes.groups`.
    #[must_use]
    pub fn get_recipes(&self) -> BTreeSet<&str> {
        targets::get_recipes(self)
    }

    /// Evaluates whether `recipe_name` is referenced in any group of `self.recipes.groups`.
    #[must_use]
    pub fn has_recipe(&self, recipe_name: &str) -> bool {
        targets::has_recipe(self, recipe_name)
    }

    /// Loads and validates a configuration file from `path`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] when reading, parsing, or validating the configuration fails.
    pub fn load(path: &Path) -> Result<Self, PodError> {
        let config_name = path::validate_load_path(path)?;
        let content = path::read_validated_file(path)?;
        let display_path = path_util::sanitize_path_token(path);
        let mut config = parser::parse_config_with_path(&content, &display_path)?;
        config.name = config_name.to_owned();
        config.path = path.to_path_buf();
        Ok(config)
    }

    /// Reads the raw `.conf` file at `path`, mapping a missing file to [`ConfigErrorKind::NotFound`](crate::error::ConfigErrorKind::NotFound).
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] with [`ConfigErrorKind::InvalidPath`](crate::error::ConfigErrorKind::InvalidPath) if `path` is not a valid `.conf`
    /// path, [`ConfigErrorKind::NotFound`](crate::error::ConfigErrorKind::NotFound) if `path` does not exist, or [`PodError::Io`] if reading
    /// the file fails.
    pub fn read_file(path: &Path) -> Result<String, PodError> {
        path::validate_load_path(path)?;
        path::read_validated_file(path)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            editor: String::new(),
            name: String::from(DEFAULT_CONFIG_NAME),
            path: PathBuf::from(DEFAULT_CONFIG_PATH),
            pods: PodsConfig::default(),
            recipes: RecipesConfig::default(),
            simulate: true,
        }
    }
}

impl FromStr for Config {
    type Err = PodError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        parser::parse_config_with_path(input, "<input>")
    }
}

/// Pod directory configuration settings (`pods` block in `pods.conf`).
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodsConfig {
    pub path: PathBuf,
}

impl PodsConfig {
    /// Creates a new [`PodsConfig`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Recipe directory and group configuration settings (`recipes` block in `pods.conf`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecipesConfig {
    pub groups: BTreeMap<String, Vec<String>>,
    pub path: PathBuf,
}

impl RecipesConfig {
    /// Creates a new [`RecipesConfig`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for RecipesConfig {
    fn default() -> Self {
        Self {
            groups: BTreeMap::new(),
            path: PathBuf::from(DEFAULT_RECIPES_PATH),
        }
    }
}

#[cfg(test)]
mod parser_tests;
#[cfg(test)]
mod path_tests;
#[cfg(test)]
mod tests;
