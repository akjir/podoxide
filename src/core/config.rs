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

//! Configuration model (`pods.conf`), PSL configuration loading, and semantic validation.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::core::log;
use crate::error::{ConfigError, ConfigErrorKind, PodError};
use crate::io::psl::{PslBlock, PslDirective, PslDocument, PslStatement, parse_psl};
use crate::io::system::fs;
use crate::utilities::collection::{
    MAX_COLLECTION_PREALLOC, bounded_capacity, dedup_preserve_order,
};
use crate::utilities::path::{
    extract_config_stem, has_parent_dir, is_valid_directory_path, is_valid_editor_value,
    sanitize_path_token,
};
use crate::utilities::string::{
    is_valid_group_target, is_valid_identifier, is_valid_recipe_identifier,
};

/// Default configuration name when `--config` is not specified.
pub const DEFAULT_CONFIG_NAME: &str = "pods";
/// Default configuration file path when `--config` is not specified.
pub const DEFAULT_CONFIG_PATH: &str = "./pods.conf";
/// Default recipe directory path when not specified in `pods.conf`.
pub const DEFAULT_RECIPES_PATH: &str = ".";
const SEEN_EDITOR: u8 = 1 << 0;
const SEEN_GENERAL: u8 = 1 << 0;
const SEEN_GROUPS: u8 = 1 << 0;
const SEEN_PODS: u8 = 1 << 1;
const SEEN_PODS_PATH: u8 = 1 << 0;
const SEEN_RECIPES: u8 = 1 << 2;
const SEEN_RECIPES_PATH: u8 = 1 << 1;
const SEEN_SIMULATE: u8 = 1 << 1;

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
    /// Returns [`PodError`] if an `@<group>` target does not exist in `self.recipes.groups`, a group contains a cycle or invalid target, or a `<recipe>` target is not referenced in any configured group.
    pub fn expand_targets(&self, targets: &[String]) -> Result<Vec<String>, PodError> {
        expand_targets(self, targets)
    }

    /// Collects the sorted set of unique recipe names referenced across all groups in `self.recipes.groups`.
    #[must_use]
    pub fn get_recipes(&self) -> BTreeSet<&str> {
        get_recipes(self)
    }

    /// Evaluates whether `recipe_name` is referenced in any group of `self.recipes.groups`.
    #[must_use]
    pub fn has_recipe(&self, recipe_name: &str) -> bool {
        has_recipe(self, recipe_name)
    }

    /// Loads and validates a configuration file from `path`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] when reading, parsing, or validating the configuration fails.
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

impl FromStr for Config {
    type Err = PodError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        parse_config_with_path(input, "<input>")
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

/// Expands `@<group>` and `<recipe>` targets against `config.recipes.groups`, preserving first-appearance order and deduplicating repeated recipes.
///
/// # Errors
///
/// Returns [`PodError`] if an `@<group>` target does not exist in `config.recipes.groups`, a group contains a cycle or invalid target, or a `<recipe>` target is not referenced in any configured group.
pub fn expand_targets(config: &Config, targets: &[String]) -> Result<Vec<String>, PodError> {
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    if log::is_debug_enabled() {
        log::debug(&format!("Targets   - {}", targets.join(" ")));
    }
    let capacity = bounded_capacity(targets.len(), MAX_COLLECTION_PREALLOC);
    let mut collected = Vec::with_capacity(capacity);
    let mut expanded_groups = BTreeSet::new();
    let mut visiting = BTreeSet::new();

    for target in targets {
        expand_single_target(
            config,
            target.as_str(),
            &mut visiting,
            &mut expanded_groups,
            &mut collected,
        )?;
    }

    let deduplicated = dedup_preserve_order(&collected);
    if log::is_debug_enabled() && !deduplicated.is_empty() {
        log::debug(&format!("Untangled - {}", deduplicated.join(" ")));
    }
    Ok(deduplicated)
}

/// Collects the sorted set of unique recipe names referenced across all groups in `config.recipes.groups`.
#[must_use]
pub fn get_recipes(config: &Config) -> BTreeSet<&str> {
    let mut recipes = BTreeSet::new();
    for group_targets in config.recipes.groups.values() {
        collect_group_recipes(group_targets, &mut recipes);
    }
    recipes
}

/// Evaluates whether `recipe_name` is referenced in any group of `config.recipes.groups`.
#[must_use]
pub fn has_recipe(config: &Config, recipe_name: &str) -> bool {
    if recipe_name.starts_with('@') || !is_valid_recipe_identifier(recipe_name) {
        return false;
    }
    config
        .recipes
        .groups
        .values()
        .any(|entries| group_contains_recipe(entries, recipe_name))
}

/// Loads and validates a configuration file from `path`.
///
/// # Errors
///
/// Returns [`PodError`] when reading, parsing, or validating the configuration fails.
pub fn load(path: &Path) -> Result<Config, PodError> {
    let config_name = validate_load_path(path)?;
    let content = read_config_file(path)?;
    let display_path = sanitize_path_token(path);
    let mut config = parse_config_with_path(&content, &display_path)?;
    config.name = config_name.to_owned();
    config.path = path.to_path_buf();
    Ok(config)
}

/// Validates and applies a single directive inside the `general` block (`editor` or `simulate`).
fn apply_general_statement(
    config: &mut Config,
    statement: &PslStatement<'_>,
    seen: &mut u8,
) -> Result<(), ConfigError> {
    let directive = match statement {
        PslStatement::Block(block) => {
            return Err(ConfigError::with_token(
                ConfigErrorKind::UnexpectedBlock,
                block.line,
                block.name,
            ));
        }
        PslStatement::Directive(directive) => directive,
    };

    match directive.name {
        "editor" => {
            mark_seen(
                seen,
                SEEN_EDITOR,
                ConfigErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            config.editor = parse_editor_directive(directive)?;
            Ok(())
        }
        "simulate" => {
            mark_seen(
                seen,
                SEEN_SIMULATE,
                ConfigErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            config.simulate = parse_bool_directive(directive)?;
            Ok(())
        }
        _ => Err(ConfigError::with_token(
            ConfigErrorKind::UnknownDirective,
            directive.line,
            directive.name,
        )),
    }
}

/// Validates and inserts a single recipe group directive inside the `recipes.groups` block.
fn apply_group_statement<'a>(
    groups: &mut BTreeMap<String, Vec<String>>,
    group_lines: &mut BTreeMap<&'a str, usize>,
    statement: &'a PslStatement<'a>,
) -> Result<(), ConfigError> {
    let directive = match statement {
        PslStatement::Block(block) => {
            return Err(ConfigError::with_token(
                ConfigErrorKind::UnexpectedBlock,
                block.line,
                block.name,
            ));
        }
        PslStatement::Directive(directive) => directive,
    };

    if groups.contains_key(directive.name) {
        return Err(ConfigError::with_token(
            ConfigErrorKind::DuplicateDirective,
            directive.line,
            directive.name,
        ));
    }
    let targets = collect_group_targets(directive)?;
    groups.insert(directive.name.to_owned(), targets);
    group_lines.insert(directive.name, directive.line);
    Ok(())
}

/// Validates and applies a single directive inside the `pods` block (`path`).
fn apply_pods_statement(
    pods: &mut PodsConfig,
    statement: &PslStatement<'_>,
    seen: &mut u8,
) -> Result<(), ConfigError> {
    let directive = match statement {
        PslStatement::Block(block) => {
            return Err(ConfigError::with_token(
                ConfigErrorKind::UnexpectedBlock,
                block.line,
                block.name,
            ));
        }
        PslStatement::Directive(directive) => directive,
    };

    match directive.name {
        "path" => {
            mark_seen(
                seen,
                SEEN_PODS_PATH,
                ConfigErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            pods.path = parse_path_directive(directive)?;
            Ok(())
        }
        _ => Err(ConfigError::with_token(
            ConfigErrorKind::UnknownDirective,
            directive.line,
            directive.name,
        )),
    }
}

/// Validates and applies a statement inside the `recipes` block (`path` directive or `groups` sub-block).
fn apply_recipes_statement(
    recipes: &mut RecipesConfig,
    statement: &PslStatement<'_>,
    seen: &mut u8,
) -> Result<(), ConfigError> {
    match statement {
        PslStatement::Block(block) => {
            if block.name != "groups" {
                return Err(ConfigError::with_token(
                    ConfigErrorKind::UnexpectedBlock,
                    block.line,
                    block.name,
                ));
            }
            mark_seen(
                seen,
                SEEN_GROUPS,
                ConfigErrorKind::DuplicateBlock,
                block.line,
                block.name,
            )?;
            parse_groups_block(recipes, block)
        }
        PslStatement::Directive(directive) => {
            if directive.name != "path" {
                return Err(ConfigError::with_token(
                    ConfigErrorKind::UnknownDirective,
                    directive.line,
                    directive.name,
                ));
            }
            mark_seen(
                seen,
                SEEN_RECIPES_PATH,
                ConfigErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            recipes.path = parse_path_directive(directive)?;
            Ok(())
        }
    }
}

/// Validates and dispatches a top-level block (`general`, `pods`, or `recipes`) in `pods.conf`.
fn apply_root_block(
    config: &mut Config,
    block: &PslBlock<'_>,
    seen: &mut u8,
) -> Result<(), ConfigError> {
    ensure_no_block_arguments(block)?;
    match block.name {
        "general" => {
            mark_seen(
                seen,
                SEEN_GENERAL,
                ConfigErrorKind::DuplicateBlock,
                block.line,
                block.name,
            )?;
            parse_general_block(config, block)
        }
        "pods" => {
            mark_seen(
                seen,
                SEEN_PODS,
                ConfigErrorKind::DuplicateBlock,
                block.line,
                block.name,
            )?;
            parse_pods_block(&mut config.pods, block)
        }
        "recipes" => {
            mark_seen(
                seen,
                SEEN_RECIPES,
                ConfigErrorKind::DuplicateBlock,
                block.line,
                block.name,
            )?;
            parse_recipes_block(&mut config.recipes, block)
        }
        _ => Err(ConfigError::with_token(
            ConfigErrorKind::UnexpectedBlock,
            block.line,
            block.name,
        )),
    }
}

/// Builds and semantically validates a [`Config`] from a parsed [`PslDocument`].
fn build_config(document: &PslDocument<'_>) -> Result<Config, ConfigError> {
    let mut config = Config::new();
    let mut seen_blocks = 0_u8;

    for block in &document.blocks {
        apply_root_block(&mut config, block, &mut seen_blocks)?;
    }

    if seen_blocks & SEEN_RECIPES == 0 {
        return Err(ConfigError::new(ConfigErrorKind::MissingRecipesBlock, None));
    }

    Ok(config)
}

/// Inserts all valid non-group recipe names from `group_targets` into `recipes`.
fn collect_group_recipes<'a>(group_targets: &'a [String], recipes: &mut BTreeSet<&'a str>) {
    for target in group_targets {
        if !target.starts_with('@') && is_valid_recipe_identifier(target) {
            recipes.insert(target.as_str());
        }
    }
}

/// Validates and collects all `<recipe>` and `@<group>` target arguments for a group directive.
fn collect_group_targets(directive: &PslDirective<'_>) -> Result<Vec<String>, ConfigError> {
    let mut targets = Vec::with_capacity(directive.arguments.len());
    for argument in &directive.arguments {
        let target = argument.as_ref();
        if !is_valid_group_target(target) {
            return Err(ConfigError::with_token(
                ConfigErrorKind::InvalidGroupTarget,
                directive.line,
                target,
            ));
        }
        targets.push(target.to_owned());
    }
    Ok(targets)
}

/// Verifies that a configuration block header does not declare any arguments.
fn ensure_no_block_arguments(block: &PslBlock<'_>) -> Result<(), ConfigError> {
    if !block.arguments.is_empty() {
        return Err(ConfigError::with_token(
            ConfigErrorKind::InvalidArgumentCount,
            block.line,
            block.name,
        ));
    }
    Ok(())
}

/// Recursively expands a single `@<group>` target from `config.recipes.groups` into `collected`.
fn expand_group_target<'a>(
    config: &'a Config,
    group_target: &'a str,
    group_name: &'a str,
    visiting: &mut BTreeSet<&'a str>,
    expanded_groups: &mut BTreeSet<&'a str>,
    collected: &mut Vec<String>,
) -> Result<(), ConfigError> {
    if !is_valid_identifier(group_name) {
        return Err(ConfigError::without_line(
            ConfigErrorKind::UnknownGroup,
            group_target,
        ));
    }
    let Some(entries) = config.recipes.groups.get(group_name) else {
        return Err(ConfigError::without_line(
            ConfigErrorKind::UnknownGroup,
            group_target,
        ));
    };
    if visiting.contains(group_name) {
        return Err(ConfigError::without_line(
            ConfigErrorKind::InvalidGroupTarget,
            group_target,
        ));
    }
    if expanded_groups.contains(group_name) {
        return Ok(());
    }
    visiting.insert(group_name);
    for entry in entries {
        expand_single_group_entry(
            config,
            entry.as_str(),
            visiting,
            expanded_groups,
            collected,
        )?;
    }
    visiting.remove(group_name);
    expanded_groups.insert(group_name);
    Ok(())
}

/// Expands a single entry (`@<subgroup>` or `<recipe>`) declared inside a group in `config.recipes.groups`.
fn expand_single_group_entry<'a>(
    config: &'a Config,
    entry: &'a str,
    visiting: &mut BTreeSet<&'a str>,
    expanded_groups: &mut BTreeSet<&'a str>,
    collected: &mut Vec<String>,
) -> Result<(), ConfigError> {
    if let Some(subgroup_name) = entry.strip_prefix('@') {
        return expand_group_target(
            config,
            entry,
            subgroup_name,
            visiting,
            expanded_groups,
            collected,
        );
    }
    if !is_valid_recipe_identifier(entry) {
        return Err(ConfigError::without_line(
            ConfigErrorKind::InvalidGroupTarget,
            entry,
        ));
    }
    collected.push(String::from(entry));
    Ok(())
}

/// Expands a single CLI target (`@<group>` or `<recipe>`) into `collected`.
fn expand_single_target<'a>(
    config: &'a Config,
    target: &'a str,
    visiting: &mut BTreeSet<&'a str>,
    expanded_groups: &mut BTreeSet<&'a str>,
    collected: &mut Vec<String>,
) -> Result<(), ConfigError> {
    if let Some(group_name) = target.strip_prefix('@') {
        return expand_group_target(
            config,
            target,
            group_name,
            visiting,
            expanded_groups,
            collected,
        );
    }
    if !has_recipe(config, target) {
        return Err(ConfigError::without_line(
            ConfigErrorKind::RecipeNotFound,
            target,
        ));
    }
    collected.push(String::from(target));
    Ok(())
}

/// Evaluates whether `entries` contains `recipe_name`.
#[must_use]
fn group_contains_recipe(entries: &[String], recipe_name: &str) -> bool {
    entries.iter().any(|entry| entry == recipe_name)
}

/// Sets `flag` in `seen` or returns a duplicate block/directive [`ConfigError`] if already set.
fn mark_seen(
    seen: &mut u8,
    flag: u8,
    kind: ConfigErrorKind,
    line: usize,
    token: &str,
) -> Result<(), ConfigError> {
    if *seen & flag != 0 {
        return Err(ConfigError::with_token(kind, line, token));
    }
    *seen |= flag;
    Ok(())
}

/// Parses a single-argument boolean directive (`"true"` or `"false"`).
fn parse_bool_directive(directive: &PslDirective<'_>) -> Result<bool, ConfigError> {
    let value = parse_single_argument(directive)?;
    match value {
        "false" => Ok(false),
        "true" => Ok(true),
        _ => Err(ConfigError::with_token(
            ConfigErrorKind::InvalidBoolean,
            directive.line,
            value,
        )),
    }
}

/// Parses and validates a configuration string, attaching a compiler-style diagnostic on failure.
fn parse_config_with_path(input: &str, file_path: &str) -> Result<Config, PodError> {
    let document = parse_psl(input)
        .map_err(|error| PodError::from(error.with_diagnostic(input, file_path)))?;
    let config = build_config(&document)
        .map_err(|error| PodError::from(error.with_diagnostic(input, file_path)))?;
    Ok(config)
}

/// Parses and validates the `general.editor` directive argument.
fn parse_editor_directive(directive: &PslDirective<'_>) -> Result<String, ConfigError> {
    let value = parse_single_argument(directive)?;
    if !is_valid_editor_value(value) {
        return Err(ConfigError::with_token(
            ConfigErrorKind::InvalidPath,
            directive.line,
            value,
        ));
    }
    Ok(value.to_owned())
}

/// Parses all statements inside the `general` block of `pods.conf`.
fn parse_general_block(config: &mut Config, block: &PslBlock<'_>) -> Result<(), ConfigError> {
    let mut seen_directives = 0_u8;
    for statement in &block.statements {
        apply_general_statement(config, statement, &mut seen_directives)?;
    }
    Ok(())
}

/// Parses and validates the `groups` sub-block inside `recipes`, ensuring it is non-empty and acyclic.
fn parse_groups_block(recipes: &mut RecipesConfig, block: &PslBlock<'_>) -> Result<(), ConfigError> {
    ensure_no_block_arguments(block)?;
    if block.statements.is_empty() {
        return Err(ConfigError::new(
            ConfigErrorKind::EmptyGroupsBlock,
            Some(block.line),
        ));
    }
    let mut group_lines = BTreeMap::new();
    for statement in &block.statements {
        apply_group_statement(&mut recipes.groups, &mut group_lines, statement)?;
    }
    validate_groups_acyclic(&recipes.groups, &group_lines, block.line)
}

/// Parses and validates a single-argument directory `path` directive.
fn parse_path_directive(directive: &PslDirective<'_>) -> Result<PathBuf, ConfigError> {
    let value = parse_single_argument(directive)?;
    if !is_valid_directory_path(value) {
        return Err(ConfigError::with_token(
            ConfigErrorKind::InvalidPath,
            directive.line,
            value,
        ));
    }
    Ok(PathBuf::from(value))
}

/// Parses all statements inside the `pods` block of `pods.conf`.
fn parse_pods_block(pods: &mut PodsConfig, block: &PslBlock<'_>) -> Result<(), ConfigError> {
    let mut seen_directives = 0_u8;
    for statement in &block.statements {
        apply_pods_statement(pods, statement, &mut seen_directives)?;
    }
    Ok(())
}

/// Parses all statements inside the `recipes` block and verifies that a `groups` sub-block is present.
fn parse_recipes_block(recipes: &mut RecipesConfig, block: &PslBlock<'_>) -> Result<(), ConfigError> {
    let mut seen_items = 0_u8;
    for statement in &block.statements {
        apply_recipes_statement(recipes, statement, &mut seen_items)?;
    }
    if seen_items & SEEN_GROUPS == 0 {
        return Err(ConfigError::new(
            ConfigErrorKind::MissingGroupsBlock,
            Some(block.line),
        ));
    }
    Ok(())
}

/// Extracts the sole argument of a directive or returns [`ConfigErrorKind::InvalidArgumentCount`].
fn parse_single_argument<'a>(directive: &'a PslDirective<'a>) -> Result<&'a str, ConfigError> {
    if directive.arguments.len() != 1 {
        return Err(ConfigError::with_token(
            ConfigErrorKind::InvalidArgumentCount,
            directive.line,
            directive.name,
        ));
    }
    Ok(directive.arguments[0].as_ref())
}

/// Reads the configuration file at `path`, mapping a missing file to [`ConfigErrorKind::NotFound`].
fn read_config_file(path: &Path) -> Result<String, PodError> {
    match fs::read_file(path) {
        Ok(content) => Ok(content),
        Err(PodError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Err(PodError::from(
            ConfigError::without_line(ConfigErrorKind::NotFound, sanitize_path_token(path)),
        )),
        Err(error) => Err(error),
    }
}

/// Verifies that `@<group>` references in `groups` form a directed acyclic graph (DAG) without cycles.
fn validate_groups_acyclic(
    groups: &BTreeMap<String, Vec<String>>,
    group_lines: &BTreeMap<&str, usize>,
    fallback_line: usize,
) -> Result<(), ConfigError> {
    let mut visiting = BTreeSet::new();
    let mut verified = BTreeSet::new();
    for group_name in groups.keys() {
        verify_group_acyclic(
            group_name.as_str(),
            groups,
            group_lines,
            &mut visiting,
            &mut verified,
            fallback_line,
        )?;
    }
    Ok(())
}

/// Validates that `path` is a safe configuration file path and returns its configuration stem.
fn validate_load_path(path: &Path) -> Result<&str, PodError> {
    path.to_str()
        .filter(|_| !has_parent_dir(path))
        .and_then(extract_config_stem)
        .ok_or_else(|| {
            PodError::from(ConfigError::without_line(
                ConfigErrorKind::InvalidPath,
                sanitize_path_token(path),
            ))
        })
}

/// Performs depth-first cycle detection starting from `group_name`.
fn verify_group_acyclic<'a>(
    group_name: &'a str,
    groups: &'a BTreeMap<String, Vec<String>>,
    group_lines: &BTreeMap<&str, usize>,
    visiting: &mut BTreeSet<&'a str>,
    verified: &mut BTreeSet<&'a str>,
    fallback_line: usize,
) -> Result<(), ConfigError> {
    if verified.contains(group_name) {
        return Ok(());
    }
    let Some(targets) = groups.get(group_name) else {
        return Ok(());
    };
    let line = group_lines.get(group_name).copied().unwrap_or(fallback_line);
    visiting.insert(group_name);
    verify_group_targets(targets, groups, group_lines, visiting, verified, line)?;
    visiting.remove(group_name);
    verified.insert(group_name);
    Ok(())
}

/// Inspects all `@<subgroup>` targets of the currently visiting group for cycles.
fn verify_group_targets<'a>(
    targets: &'a [String],
    groups: &'a BTreeMap<String, Vec<String>>,
    group_lines: &BTreeMap<&str, usize>,
    visiting: &mut BTreeSet<&'a str>,
    verified: &mut BTreeSet<&'a str>,
    line: usize,
) -> Result<(), ConfigError> {
    for target in targets {
        let Some(subgroup) = target.strip_prefix('@') else {
            continue;
        };
        if visiting.contains(subgroup) {
            return Err(ConfigError::with_token(
                ConfigErrorKind::InvalidGroupTarget,
                line,
                target.as_str(),
            ));
        }
        verify_group_acyclic(subgroup, groups, group_lines, visiting, verified, line)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStringExt;
    use std::path::{Path, PathBuf};
    use std::process;
    use std::str::FromStr;

    use crate::core::log;
    use crate::error::{ConfigErrorKind, MAX_ERROR_TOKEN_BYTES, PodError, PslErrorKind};
    use crate::io::system::fs as system_fs;
    use super::{
        Config, DEFAULT_CONFIG_NAME, DEFAULT_CONFIG_PATH, DEFAULT_RECIPES_PATH, PodsConfig,
        RecipesConfig, expand_targets, get_recipes, has_recipe, load,
    };

    fn assert_config_error(input: &str, expected_kind: ConfigErrorKind) {
        let error = Config::from_str(input).unwrap_err();
        match error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, expected_kind);
                let diagnostic = config_error.diagnostic.as_ref().unwrap();
                assert_eq!(diagnostic.file_path, "<input>");
            }
            other => panic!("expected PodError::Config({expected_kind:?}), got {other:?}"),
        }
    }

    #[test]
    fn expand_targets_and_has_recipe_preserve_first_appearance_and_reject_invalid_targets() {
        let config_input = "\
recipes {
    groups {
        single target
        stack push pop
        glados the cake lie
        nested @stack @glados
        diamond @nested @stack
    }
}
";
        let config = Config::from_str(config_input).unwrap();

        assert!(config.has_recipe("target"));
        assert!(has_recipe(&config, "push"));
        assert!(!config.has_recipe("invalid"));
        assert!(!config.has_recipe("@stack"));
        assert!(!config.has_recipe("../evil"));

        assert!(config.expand_targets(&[]).unwrap().is_empty());

        log::set_debug_enabled(true);
        assert_eq!(
            config.expand_targets(&[String::from("target")]).unwrap(),
            vec![String::from("target")]
        );
        assert_eq!(
            expand_targets(&config, &[String::from("@stack")]).unwrap(),
            vec![String::from("push"), String::from("pop")]
        );
        assert_eq!(
            config
                .expand_targets(&[String::from("target"), String::from("@stack")])
                .unwrap(),
            vec![
                String::from("target"),
                String::from("push"),
                String::from("pop"),
            ]
        );
        assert_eq!(
            config
                .expand_targets(&[
                    String::from("target"),
                    String::from("@stack"),
                    String::from("target"),
                ])
                .unwrap(),
            vec![
                String::from("target"),
                String::from("push"),
                String::from("pop"),
            ]
        );
        assert_eq!(
            config
                .expand_targets(&[
                    String::from("the"),
                    String::from("@glados"),
                    String::from("lie"),
                ])
                .unwrap(),
            vec![
                String::from("the"),
                String::from("cake"),
                String::from("lie"),
            ]
        );
        assert_eq!(
            config
                .expand_targets(&[
                    String::from("lie"),
                    String::from("cake"),
                    String::from("@glados"),
                ])
                .unwrap(),
            vec![
                String::from("lie"),
                String::from("cake"),
                String::from("the"),
            ]
        );
        assert_eq!(
            config.expand_targets(&[String::from("@diamond")]).unwrap(),
            vec![
                String::from("push"),
                String::from("pop"),
                String::from("the"),
                String::from("cake"),
                String::from("lie"),
            ]
        );
        log::set_debug_enabled(false);

        let unknown_recipe = config
            .expand_targets(&[
                String::from("push"),
                String::from("invalid"),
                String::from("pop"),
            ])
            .unwrap_err();
        match unknown_recipe {
            PodError::Config(error) => {
                assert_eq!(error.kind, ConfigErrorKind::RecipeNotFound);
                assert_eq!(error.token.as_deref(), Some("invalid"));
            }
            other => panic!("expected RecipeNotFound, got {other:?}"),
        }

        for bad_group in ["@invalid", "@", "@-bad"] {
            let unknown_group = config
                .expand_targets(&[String::from(bad_group)])
                .unwrap_err();
            match unknown_group {
                PodError::Config(error) => {
                    assert_eq!(error.kind, ConfigErrorKind::UnknownGroup);
                    assert_eq!(error.token.as_deref(), Some(bad_group));
                }
                other => panic!("expected UnknownGroup, got {other:?}"),
            }
        }

        // Manually constructed cyclic or invalid group entries must not recurse infinitely
        let mut manual_config = Config::new();
        manual_config
            .recipes
            .groups
            .insert(String::from("cycle"), vec![String::from("@cycle")]);
        manual_config
            .recipes
            .groups
            .insert(String::from("bad_entry"), vec![String::from("../bad")]);

        let cycle_error = manual_config
            .expand_targets(&[String::from("@cycle")])
            .unwrap_err();
        match cycle_error {
            PodError::Config(error) => assert_eq!(error.kind, ConfigErrorKind::InvalidGroupTarget),
            other => panic!("expected InvalidGroupTarget, got {other:?}"),
        }

        let entry_error = manual_config
            .expand_targets(&[String::from("@bad_entry")])
            .unwrap_err();
        match entry_error {
            PodError::Config(error) => assert_eq!(error.kind, ConfigErrorKind::InvalidGroupTarget),
            other => panic!("expected InvalidGroupTarget, got {other:?}"),
        }
    }

    #[test]
    fn get_recipes_collects_unique_valid_recipes_and_rejects_invalid_entries() {
        let config_input = "\
recipes {
    groups {
        backend db-server cache-server
        frontend web-frontend @backend db-server
    }
}
";
        let config = Config::from_str(config_input).unwrap();
        let expected = BTreeSet::from(["cache-server", "db-server", "web-frontend"]);
        assert_eq!(config.get_recipes(), expected);
        assert_eq!(get_recipes(&config), expected);

        let mut manual_config = Config::new();
        manual_config.recipes.groups.insert(
            String::from("mixed"),
            vec![
                String::from("valid_recipe"),
                String::from("@subgroup"),
                String::from("../escape"),
                String::from(" app "),
                String::from("bad\nname"),
                String::new(),
            ],
        );
        let manual_expected = BTreeSet::from(["valid_recipe"]);
        assert_eq!(manual_config.get_recipes(), manual_expected);
        assert_eq!(get_recipes(&manual_config), manual_expected);
    }

    #[test]
    fn group_and_recipe_sharing_name_remain_distinct_in_parsing_membership_and_expansion() {
        let config_input = "\
general {
    editor vim
    simulate false
}

pods {
    path /pods
}

recipes {
    path ./recipes

    groups {
        all @alpha @stack
        stack media cloud proxy
        alpha beta alpha
    }
}
";
        let config = Config::from_str(config_input).unwrap();
        assert_eq!(config.editor, "vim");
        assert!(!config.simulate);
        assert_eq!(config.pods.path, PathBuf::from("/pods"));
        assert_eq!(config.recipes.path, PathBuf::from("./recipes"));
        assert_eq!(config.recipes.groups.len(), 3);
        assert_eq!(
            config.recipes.groups.get("all"),
            Some(&vec![String::from("@alpha"), String::from("@stack")])
        );
        assert_eq!(
            config.recipes.groups.get("stack"),
            Some(&vec![
                String::from("media"),
                String::from("cloud"),
                String::from("proxy"),
            ])
        );
        assert_eq!(
            config.recipes.groups.get("alpha"),
            Some(&vec![String::from("beta"), String::from("alpha")])
        );

        // `alpha` is both a group (`@alpha`) and a recipe (`alpha`), whereas `stack` and `all` are only groups.
        assert!(config.has_recipe("alpha"));
        assert!(config.has_recipe("beta"));
        assert!(config.has_recipe("media"));
        assert!(config.has_recipe("cloud"));
        assert!(config.has_recipe("proxy"));
        assert!(!config.has_recipe("@alpha"));
        assert!(!config.has_recipe("@stack"));
        assert!(!config.has_recipe("@all"));
        assert!(!config.has_recipe("stack"));
        assert!(!config.has_recipe("all"));

        let expected_recipes = BTreeSet::from(["alpha", "beta", "cloud", "media", "proxy"]);
        assert_eq!(config.get_recipes(), expected_recipes);

        // `@alpha` (group) and `alpha` (recipe) are distinct targets and expand differently.
        assert_eq!(
            config.expand_targets(&[String::from("@alpha")]).unwrap(),
            vec![String::from("beta"), String::from("alpha")]
        );
        assert_eq!(
            config.expand_targets(&[String::from("alpha")]).unwrap(),
            vec![String::from("alpha")]
        );
        assert_eq!(
            config
                .expand_targets(&[String::from("alpha"), String::from("@alpha")])
                .unwrap(),
            vec![String::from("alpha"), String::from("beta")]
        );
        assert_eq!(
            config
                .expand_targets(&[String::from("@alpha"), String::from("alpha")])
                .unwrap(),
            vec![String::from("beta"), String::from("alpha")]
        );
        assert_eq!(
            config.expand_targets(&[String::from("@stack")]).unwrap(),
            vec![
                String::from("media"),
                String::from("cloud"),
                String::from("proxy"),
            ]
        );
        assert_eq!(
            config.expand_targets(&[String::from("@all")]).unwrap(),
            vec![
                String::from("beta"),
                String::from("alpha"),
                String::from("media"),
                String::from("cloud"),
                String::from("proxy"),
            ]
        );

        // Targeting a group-only identifier without `@` fails with RecipeNotFound.
        for group_only in ["stack", "all"] {
            let error = config
                .expand_targets(&[String::from(group_only)])
                .unwrap_err();
            match error {
                PodError::Config(config_error) => {
                    assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
                    assert_eq!(config_error.token.as_deref(), Some(group_only));
                }
                other => panic!("expected RecipeNotFound for {group_only}, got {other:?}"),
            }
        }

        // Targeting a recipe-only identifier with `@` fails with UnknownGroup.
        for recipe_only_as_group in ["@beta", "@media", "@cloud", "@proxy"] {
            let error = config
                .expand_targets(&[String::from(recipe_only_as_group)])
                .unwrap_err();
            match error {
                PodError::Config(config_error) => {
                    assert_eq!(config_error.kind, ConfigErrorKind::UnknownGroup);
                    assert_eq!(config_error.token.as_deref(), Some(recipe_only_as_group));
                }
                other => panic!("expected UnknownGroup for {recipe_only_as_group}, got {other:?}"),
            }
        }

        // Replacing recipe `alpha` with subgroup `@alpha` inside group `alpha` forms a self-cycle on that exact line.
        let cyclic_input = "\
recipes {
    path ./recipes

    groups {
        all @alpha @stack
        stack media cloud proxy
        alpha beta @alpha
    }
}
";
        let cyclic_error = Config::from_str(cyclic_input).unwrap_err();
        match cyclic_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::InvalidGroupTarget);
                assert_eq!(config_error.line, Some(7));
                assert_eq!(config_error.token.as_deref(), Some("@alpha"));
                let diagnostic = config_error.diagnostic.as_ref().unwrap();
                assert_eq!(diagnostic.line_number, 7);
                assert_eq!(diagnostic.col_number, 20);
                assert_eq!(
                    diagnostic.annotation_carets,
                    "                   ^^^^^^"
                );
            }
            other => panic!("expected InvalidGroupTarget, got {other:?}"),
        }
    }

    #[test]
    fn load_root_pods_conf_and_temp_config_files() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();
        assert_eq!(root_config.name, DEFAULT_CONFIG_NAME);
        assert_eq!(root_config.path, PathBuf::from(DEFAULT_CONFIG_PATH));
        assert_eq!(root_config.editor, "vim");
        assert!(root_config.simulate);
        assert_eq!(root_config.pods.path, PathBuf::from("./pods"));
        assert_eq!(root_config.recipes.path, PathBuf::from(DEFAULT_RECIPES_PATH));
        assert_eq!(
            root_config.recipes.groups.get("all"),
            Some(&vec![String::from("recipe")])
        );

        let temp_dir = env::temp_dir().join(format!("podoxide_config_test_{}", process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let staging_path = temp_dir.join("staging.conf");
        let staging_content = "\
general {
    editor nano
    simulate false
}
pods {
    path /var/lib/pods
}
recipes {
    path ./recipes
    groups {
        backend db-server cache-server
        all @backend web-frontend
    }
}
";
        system_fs::write_file(&staging_path, staging_content).unwrap();

        let loaded = load(&staging_path).unwrap();
        assert_eq!(loaded.name, "staging");
        assert_eq!(loaded.path, staging_path);
        assert_eq!(loaded.editor, "nano");
        assert!(!loaded.simulate);
        assert_eq!(loaded.pods.path, PathBuf::from("/var/lib/pods"));
        assert_eq!(loaded.recipes.path, PathBuf::from("./recipes"));
        assert_eq!(loaded.recipes.groups.len(), 2);
        assert_eq!(
            loaded.recipes.groups.get("backend"),
            Some(&vec![String::from("db-server"), String::from("cache-server")])
        );
        assert_eq!(
            loaded.recipes.groups.get("all"),
            Some(&vec![String::from("@backend"), String::from("web-frontend")])
        );

        let missing_path = temp_dir.join("missing.conf");
        let missing_error = Config::load(&missing_path).unwrap_err();
        match &missing_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::NotFound);
                assert_eq!(config_error.line, None);
                assert_eq!(
                    config_error.token.as_deref(),
                    Some(missing_path.to_str().unwrap())
                );
            }
            other => panic!("expected PodError::Config(NotFound), got {other:?}"),
        }
        assert!(
            missing_error
                .to_string()
                .contains("Config error: configuration file not found:")
        );
        assert!(
            missing_error
                .to_string()
                .contains(missing_path.to_str().unwrap())
        );

        let directory_config_path = temp_dir.join("directory.conf");
        fs::create_dir_all(&directory_config_path).unwrap();
        let directory_error = Config::load(&directory_config_path).unwrap_err();
        assert!(matches!(directory_error, PodError::Io(_)));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parse_minimal_config_with_defaults() {
        let minimal = "\
recipes {
    groups {
        default example-app
    }
}
";
        let config = Config::from_str(minimal).unwrap();
        assert_eq!(config.editor, "");
        assert!(config.simulate);
        assert_eq!(config.pods, PodsConfig::new());
        assert_eq!(config.recipes.path, PathBuf::from(DEFAULT_RECIPES_PATH));
        assert_eq!(
            config.recipes.groups.get("default"),
            Some(&vec![String::from("example-app")])
        );
        assert_eq!(RecipesConfig::new().path, PathBuf::from(DEFAULT_RECIPES_PATH));
    }

    #[test]
    fn red_team_cycles_path_traversal_editor_and_target_exploits() {
        // 1. Cyclic @group references in recipes.groups
        assert_config_error(
            "recipes {\n    groups {\n        all @all\n    }\n}",
            ConfigErrorKind::InvalidGroupTarget,
        );
        assert_config_error(
            "recipes {\n    groups {\n        a @b\n        b @a\n    }\n}",
            ConfigErrorKind::InvalidGroupTarget,
        );
        assert_config_error(
            "recipes {\n    groups {\n        a app @b\n        b @c\n        c @a\n    }\n}",
            ConfigErrorKind::InvalidGroupTarget,
        );

        // Diamond DAG and missing subgroup reference remain valid
        let valid_dag = "\
recipes {
    groups {
        db postgres
        cache redis
        backend @db @cache
        all @backend @db @missing_group frontend
    }
}
";
        assert!(Config::from_str(valid_dag).is_ok());

        // 2. Wildcards, dot-files, backslashes, and invalid @group identifiers
        for malicious_target in [
            "*",
            "*db",
            "@*",
            "app*",
            "app?",
            ".hidden",
            "..hidden",
            "app.pod",
            "a..b",
            "@.hidden",
            "@123",
            "@-bad",
            "\"app\\\\evil\"",
        ] {
            let input = format!(
                "recipes {{\n    groups {{\n        all {malicious_target}\n    }}\n}}"
            );
            assert_config_error(&input, ConfigErrorKind::InvalidGroupTarget);
        }

        // 3. Path traversal, volume colon injection, flag injection, whitespace, and root '/'
        for malicious_path in [
            "..",
            "../pods",
            "pods/..",
            "/var/pods/../../etc",
            "/var/pods:ro,Z",
            "pods:rw",
            "-rf",
            "--privileged",
            "\"   \"",
            "\" /var/pods\"",
            "\"/var/pods \"",
            "/",
            "///",
            "/var/pods/\u{202E}evil",
        ] {
            let pods_input = format!(
                "pods {{\n    path {malicious_path}\n}}\nrecipes {{\n    groups {{\n        all app\n    }}\n}}"
            );
            assert_config_error(&pods_input, ConfigErrorKind::InvalidPath);

            let recipes_input = format!(
                "recipes {{\n    path {malicious_path}\n    groups {{\n        all app\n    }}\n}}"
            );
            assert_config_error(&recipes_input, ConfigErrorKind::InvalidPath);
        }

        // 4. Unvalidated general.editor directive
        for malicious_editor in [
            "\"\"",
            "\"   \"",
            "\"vim --flag\"",
            "\"vim\\t-u\"",
            "\"vim\\nevil\"",
            "\"vim\\r\"",
            "-rf",
            "--help",
            "/usr/bin/-vim",
            ".",
            "..",
            "vim/.",
            "/usr/bin/vim/.",
            "/usr/bin/",
            "../../bin/vim",
            "/usr/bin/../../etc/passwd",
            "/usr/bin/\u{202E}miv",
        ] {
            let editor_input = format!(
                "general {{\n    editor {malicious_editor}\n}}\nrecipes {{\n    groups {{\n        all app\n    }}\n}}"
            );
            assert_config_error(&editor_input, ConfigErrorKind::InvalidPath);
        }

        for valid_editor in ["vim", "nvim", "/usr/bin/vim", "./bin/editor_1"] {
            let editor_input = format!(
                "general {{\n    editor {valid_editor}\n}}\nrecipes {{\n    groups {{\n        all app\n    }}\n}}"
            );
            assert!(Config::from_str(&editor_input).is_ok());
        }

        // 5. Unvalidated path argument in Config::load / load
        for malicious_load_path in [
            "",
            ".",
            "..",
            "../staging.conf",
            "/tmp/../staging.conf",
            "-staging.conf",
            "/tmp/-staging.conf",
            ".conf",
            "/tmp/.conf",
            "..conf",
            "/tmp/staging.conf/",
            " /tmp/staging.conf",
            "/tmp/staging.conf\n",
            "/tmp/\u{202E}staging.conf",
        ] {
            let error = Config::load(Path::new(malicious_load_path)).unwrap_err();
            match error {
                PodError::Config(config_error) => {
                    assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
                }
                other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
            }
        }

        let injected_load_error =
            Config::load(Path::new("/tmp/../bad\n\x1b[31m\u{202E}.conf")).unwrap_err();
        match injected_load_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
                let token = config_error.token.as_deref().unwrap();
                assert!(!token.contains('\n'));
                assert!(!token.contains('\x1b'));
                assert!(!token.contains('\u{202E}'));
            }
            other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
        }

        let oversized_path = format!("/tmp/../{}.conf", "a".repeat(MAX_ERROR_TOKEN_BYTES + 128));
        let oversized_load_error = Config::load(Path::new(&oversized_path)).unwrap_err();
        match oversized_load_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
                assert!(config_error.token.as_deref().unwrap().len() <= MAX_ERROR_TOKEN_BYTES);
            }
            other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
        }

        let invalid_utf8_os_string =
            OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff, 0xfe]);
        let invalid_utf8_error = Config::load(Path::new(&invalid_utf8_os_string)).unwrap_err();
        match invalid_utf8_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::InvalidPath);
            }
            other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
        }
    }

    #[test]
    fn reject_all_semantic_and_syntactic_config_errors() {
        assert_config_error("", ConfigErrorKind::MissingRecipesBlock);
        assert_config_error(
            "general {\n    editor vim\n}",
            ConfigErrorKind::MissingRecipesBlock,
        );

        assert_config_error(
            "recipes {\n    path .\n}",
            ConfigErrorKind::MissingGroupsBlock,
        );
        assert_config_error(
            "recipes {\n    groups {}\n}",
            ConfigErrorKind::EmptyGroupsBlock,
        );

        assert_config_error(
            "unknown_root {}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::UnexpectedBlock,
        );
        assert_config_error(
            "general {\n    nested {}\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::UnexpectedBlock,
        );
        assert_config_error(
            "pods {\n    nested {}\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::UnexpectedBlock,
        );
        assert_config_error(
            "recipes {\n    other {}\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::UnexpectedBlock,
        );
        assert_config_error(
            "recipes {\n    groups {\n        nested {}\n    }\n}",
            ConfigErrorKind::UnexpectedBlock,
        );

        assert_config_error(
            "general {}\ngeneral {}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::DuplicateBlock,
        );
        assert_config_error(
            "pods {}\npods {}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::DuplicateBlock,
        );
        assert_config_error(
            "recipes {\n    groups {\n        all app\n    }\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::DuplicateBlock,
        );
        assert_config_error(
            "recipes {\n    groups {\n        all app\n    }\n    groups {\n        web frontend\n    }\n}",
            ConfigErrorKind::DuplicateBlock,
        );

        assert_config_error(
            "general {\n    editor vim\n    editor nano\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::DuplicateDirective,
        );
        assert_config_error(
            "general {\n    simulate true\n    simulate false\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::DuplicateDirective,
        );
        assert_config_error(
            "pods {\n    path /a\n    path /b\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::DuplicateDirective,
        );
        assert_config_error(
            "recipes {\n    path ./a\n    path ./b\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::DuplicateDirective,
        );
        assert_config_error(
            "recipes {\n    groups {\n        all app\n        all web\n    }\n}",
            ConfigErrorKind::DuplicateDirective,
        );

        assert_config_error(
            "general {\n    unknown_key val\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::UnknownDirective,
        );
        assert_config_error(
            "pods {\n    unknown_key val\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::UnknownDirective,
        );
        assert_config_error(
            "recipes {\n    unknown_key val\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::UnknownDirective,
        );

        assert_config_error(
            "general arg {}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::InvalidArgumentCount,
        );
        assert_config_error(
            "recipes {\n    groups arg {\n        all app\n    }\n}",
            ConfigErrorKind::InvalidArgumentCount,
        );
        assert_config_error(
            "general {\n    editor vim --flag\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::InvalidArgumentCount,
        );
        assert_config_error(
            "general {\n    simulate true false\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::InvalidArgumentCount,
        );
        assert_config_error(
            "pods {\n    path /a /b\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::InvalidArgumentCount,
        );

        assert_config_error(
            "general {\n    simulate yes\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::InvalidBoolean,
        );

        assert_config_error(
            "pods {\n    path \"\"\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::InvalidPath,
        );
        assert_config_error(
            "recipes {\n    path \"\"\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::InvalidPath,
        );
        assert_config_error(
            "pods {\n    path \"/bad\\npath\"\n}\nrecipes {\n    groups {\n        all app\n    }\n}",
            ConfigErrorKind::InvalidPath,
        );

        for invalid_target in [
            "\"\"",
            ".",
            "..",
            "-flag",
            "@",
            "@@double",
            "@group/container",
            "recipe/container",
            "@group:1",
            "\"space target\"",
            "\"newline\\ntarget\"",
        ] {
            let input = format!("recipes {{\n    groups {{\n        all {invalid_target}\n    }}\n}}");
            assert_config_error(&input, ConfigErrorKind::InvalidGroupTarget);
        }

        let syntax_error = Config::from_str("editor vim").unwrap_err();
        match syntax_error {
            PodError::Psl(psl_error) => {
                assert_eq!(psl_error.kind, PslErrorKind::UnexpectedRootDirective);
                let diagnostic = psl_error.diagnostic.as_ref().unwrap();
                assert_eq!(diagnostic.file_path, "<input>");
                assert_eq!(diagnostic.col_number, 1);
                assert_eq!(diagnostic.annotation_carets, "^^^^^^");
            }
            other => panic!("expected PodError::Psl, got {other:?}"),
        }
    }
}
