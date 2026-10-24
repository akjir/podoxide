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

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use crate::error::{ConfigError, ConfigErrorKind, MAX_ERROR_TOKEN_BYTES, PodError};
use crate::io::psl::{PslBlock, PslDirective, PslDocument, PslStatement, parse_psl};
use crate::io::system::fs;
use crate::utilities::path::{extract_config_stem, has_parent_dir};
use crate::utilities::string::{
    clamp_utf8_prefix, contains_control_or_whitespace, is_bidi_control, is_valid_identifier,
    is_valid_recipe_identifier, sanitize_char,
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
        let document = parse_psl(input)?;
        let config = build_config(&document)?;
        Ok(config)
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

/// Loads and validates a configuration file from `path`.
///
/// # Errors
///
/// Returns [`PodError`] when reading, parsing, or validating the configuration fails.
pub fn load(path: &Path) -> Result<Config, PodError> {
    validate_load_path(path)?;
    let content = read_config_file(path)?;
    let mut config = Config::from_str(&content)?;
    config.name = path
        .to_str()
        .and_then(extract_config_stem)
        .unwrap_or(DEFAULT_CONFIG_NAME)
        .to_owned();
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
fn apply_group_statement(
    groups: &mut BTreeMap<String, Vec<String>>,
    statement: &PslStatement<'_>,
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

/// Evaluates whether `value` is a safe directory path for `pods.path` or `recipes.path`.
fn is_valid_directory_path(value: &str) -> bool {
    if value.is_empty()
        || value != value.trim()
        || value.starts_with('-')
        || value.contains(':')
        || value
            .chars()
            .any(|character| character.is_control() || is_bidi_control(character))
    {
        return false;
    }
    let path = Path::new(value);
    if has_parent_dir(path) {
        return false;
    }
    path.components()
        .any(|component| matches!(component, Component::CurDir | Component::Normal(_)))
}

/// Evaluates whether `value` is a safe executable name or path for `general.editor`.
fn is_valid_editor_value(value: &str) -> bool {
    if value.is_empty()
        || value.starts_with('-')
        || value.ends_with('/')
        || contains_control_or_whitespace(value)
    {
        return false;
    }
    let path = Path::new(value);
    if has_parent_dir(path) {
        return false;
    }
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    !file_name.is_empty()
        && file_name != "."
        && file_name != ".."
        && !file_name.starts_with('-')
}

/// Evaluates whether `target` is a valid `@<group>` reference or recipe stem identifier.
fn is_valid_group_target(target: &str) -> bool {
    if let Some(group_name) = target.strip_prefix('@') {
        return is_valid_identifier(group_name);
    }
    is_valid_recipe_identifier(target)
}

/// Evaluates whether `path` is a safe configuration file path without `..` traversal or invalid filenames.
fn is_valid_load_path(path: &Path) -> bool {
    let Some(raw) = path.to_str() else {
        return false;
    };
    !has_parent_dir(path) && extract_config_stem(raw).is_some()
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
    for statement in &block.statements {
        apply_group_statement(&mut recipes.groups, statement)?;
    }
    validate_groups_acyclic(&recipes.groups, block.line)
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
        Err(PodError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            Err(PodError::from(ConfigError {
                kind: ConfigErrorKind::NotFound,
                line: None,
                token: Some(sanitize_path_token(path)),
            }))
        }
        Err(error) => Err(error),
    }
}

/// Clamps and sanitizes a filesystem `path` into a bounded [`MAX_ERROR_TOKEN_BYTES`] error token.
fn sanitize_path_token(path: &Path) -> String {
    let raw_bytes = path.as_os_str().as_encoded_bytes();
    let prefix_len = raw_bytes.len().min(MAX_ERROR_TOKEN_BYTES);
    let lossy = String::from_utf8_lossy(&raw_bytes[..prefix_len]);
    let clamped = clamp_utf8_prefix(&lossy, MAX_ERROR_TOKEN_BYTES);
    let mut sanitized = String::with_capacity(clamped.len());
    for character in clamped.chars() {
        sanitized.push(sanitize_char(character));
    }
    sanitized
}

/// Verifies that `@<group>` references in `groups` form a directed acyclic graph (DAG) without cycles.
fn validate_groups_acyclic(
    groups: &BTreeMap<String, Vec<String>>,
    line: usize,
) -> Result<(), ConfigError> {
    let mut visiting = Vec::with_capacity(groups.len());
    let mut verified = Vec::with_capacity(groups.len());
    for group_name in groups.keys() {
        verify_group_acyclic(
            group_name.as_str(),
            groups,
            &mut visiting,
            &mut verified,
            line,
        )?;
    }
    Ok(())
}

/// Validates that `path` is a safe configuration file path before reading from disk.
fn validate_load_path(path: &Path) -> Result<(), PodError> {
    if !is_valid_load_path(path) {
        return Err(PodError::from(ConfigError {
            kind: ConfigErrorKind::InvalidPath,
            line: None,
            token: Some(sanitize_path_token(path)),
        }));
    }
    Ok(())
}

/// Performs depth-first cycle detection starting from `group_name`.
fn verify_group_acyclic<'a>(
    group_name: &'a str,
    groups: &'a BTreeMap<String, Vec<String>>,
    visiting: &mut Vec<&'a str>,
    verified: &mut Vec<&'a str>,
    line: usize,
) -> Result<(), ConfigError> {
    if verified.contains(&group_name) {
        return Ok(());
    }
    let Some(targets) = groups.get(group_name) else {
        return Ok(());
    };
    visiting.push(group_name);
    verify_group_targets(targets, groups, visiting, verified, line)?;
    visiting.pop();
    verified.push(group_name);
    Ok(())
}

/// Inspects all `@<subgroup>` targets of the currently visiting group for cycles.
fn verify_group_targets<'a>(
    targets: &'a [String],
    groups: &'a BTreeMap<String, Vec<String>>,
    visiting: &mut Vec<&'a str>,
    verified: &mut Vec<&'a str>,
    line: usize,
) -> Result<(), ConfigError> {
    for target in targets {
        let Some(subgroup) = target.strip_prefix('@') else {
            continue;
        };
        if visiting.contains(&subgroup) {
            return Err(ConfigError::with_token(
                ConfigErrorKind::InvalidGroupTarget,
                line,
                target.as_str(),
            ));
        }
        verify_group_acyclic(subgroup, groups, visiting, verified, line)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStringExt;
    use std::path::{Path, PathBuf};
    use std::process;
    use std::str::FromStr;

    use crate::error::{ConfigErrorKind, MAX_ERROR_TOKEN_BYTES, PodError, PslErrorKind};
    use crate::io::system::fs as system_fs;
    use super::{
        Config, DEFAULT_CONFIG_NAME, DEFAULT_CONFIG_PATH, DEFAULT_RECIPES_PATH, PodsConfig,
        RecipesConfig, load,
    };

    fn assert_config_error(input: &str, expected_kind: ConfigErrorKind) {
        let error = Config::from_str(input).unwrap_err();
        match error {
            PodError::Config(config_error) => assert_eq!(config_error.kind, expected_kind),
            other => panic!("expected PodError::Config({expected_kind:?}), got {other:?}"),
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
            let err = Config::load(Path::new(malicious_load_path)).unwrap_err();
            match err {
                PodError::Config(config_err) => {
                    assert_eq!(config_err.kind, ConfigErrorKind::InvalidPath);
                }
                other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
            }
        }

        let injected_load_err =
            Config::load(Path::new("/tmp/../bad\n\x1b[31m\u{202E}.conf")).unwrap_err();
        match injected_load_err {
            PodError::Config(config_err) => {
                assert_eq!(config_err.kind, ConfigErrorKind::InvalidPath);
                let token = config_err.token.as_deref().unwrap();
                assert!(!token.contains('\n'));
                assert!(!token.contains('\x1b'));
                assert!(!token.contains('\u{202E}'));
            }
            other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
        }

        let oversized_path = format!("/tmp/../{}.conf", "a".repeat(MAX_ERROR_TOKEN_BYTES + 128));
        let oversized_load_err = Config::load(Path::new(&oversized_path)).unwrap_err();
        match oversized_load_err {
            PodError::Config(config_err) => {
                assert_eq!(config_err.kind, ConfigErrorKind::InvalidPath);
                assert!(config_err.token.as_deref().unwrap().len() <= MAX_ERROR_TOKEN_BYTES);
            }
            other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
        }

        let invalid_utf8_os = OsString::from_vec(vec![b'/', b't', b'm', b'p', b'/', 0xff, 0xfe]);
        let invalid_utf8_err = Config::load(Path::new(&invalid_utf8_os)).unwrap_err();
        match invalid_utf8_err {
            PodError::Config(config_err) => {
                assert_eq!(config_err.kind, ConfigErrorKind::InvalidPath);
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
            }
            other => panic!("expected PodError::Psl, got {other:?}"),
        }
    }
}
