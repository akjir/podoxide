//! Configuration model (`pods.conf`), loading, and target group resolution stubs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::error::PodError;

/// Default configuration name when `--config` is not specified.
pub const DEFAULT_CONFIG_NAME: &str = "pods";
/// Default configuration file path when `--config` is not specified.
pub const DEFAULT_CONFIG_PATH: &str = "./pods.conf";
/// Default recipe directory path when not specified in `pods.conf`.
pub const DEFAULT_RECIPES_PATH: &str = ".";

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

    /// Loads and validates a configuration file from `path`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] when reading or parsing the configuration fails.
    pub fn load(path: &Path) -> Result<Self, PodError> {
        load(path)
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

/// Loads and validates a configuration file from `path` (placeholder stub).
///
/// # Errors
///
/// Returns [`PodError`] when reading or parsing the configuration fails.
pub fn load(_path: &Path) -> Result<Config, PodError> {
    Ok(Config::default())
}
