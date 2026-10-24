//! Configuration model (`pods.conf`), PSL configuration loading, and semantic validation.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use crate::core::cli;
use crate::error::{ConfigError, ConfigErrorKind, PodError};
use crate::io::psl::{PslBlock, PslDirective, PslDocument, PslStatement, parse_psl};
use crate::io::system::fs;

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
    let content = fs::read_file(path)?;
    let mut config = Config::from_str(&content)?;
    config.name = cli::resolve_config_name(Some(path));
    config.path = path.to_path_buf();
    Ok(config)
}

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

fn has_parent_dir(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn is_valid_directory_path(value: &str) -> bool {
    if value.is_empty()
        || value != value.trim()
        || value.starts_with('-')
        || value.contains(':')
        || value.chars().any(char::is_control)
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

fn is_valid_editor_value(value: &str) -> bool {
    if value.is_empty()
        || value.starts_with('-')
        || value.ends_with('/')
        || value
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
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

fn is_valid_group_identifier(identifier: &str) -> bool {
    let Some(&first) = identifier.as_bytes().first() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != b'_' {
        return false;
    }
    identifier
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn is_valid_group_target(target: &str) -> bool {
    if let Some(group_name) = target.strip_prefix('@') {
        return is_valid_group_identifier(group_name);
    }
    is_valid_recipe_identifier(target)
}

fn is_valid_load_path(path: &Path) -> bool {
    let Some(raw) = path.to_str() else {
        return false;
    };
    if raw.is_empty()
        || raw != raw.trim()
        || raw.starts_with('-')
        || raw.ends_with('/')
        || raw.chars().any(char::is_control)
        || has_parent_dir(path)
    {
        return false;
    }
    let file_name = raw.rsplit('/').next().unwrap_or(raw);
    let stem = file_name.strip_suffix(".conf").unwrap_or(file_name);
    !file_name.is_empty()
        && file_name != "."
        && file_name != ".."
        && !file_name.starts_with('-')
        && !stem.is_empty()
        && stem != "."
        && stem != ".."
}

fn is_valid_recipe_identifier(identifier: &str) -> bool {
    let Some(&first) = identifier.as_bytes().first() else {
        return false;
    };
    if !first.is_ascii_alphanumeric() && first != b'_' {
        return false;
    }
    identifier
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

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

fn parse_general_block(config: &mut Config, block: &PslBlock<'_>) -> Result<(), ConfigError> {
    let mut seen_directives = 0_u8;
    for statement in &block.statements {
        apply_general_statement(config, statement, &mut seen_directives)?;
    }
    Ok(())
}

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

fn parse_pods_block(pods: &mut PodsConfig, block: &PslBlock<'_>) -> Result<(), ConfigError> {
    let mut seen_directives = 0_u8;
    for statement in &block.statements {
        apply_pods_statement(pods, statement, &mut seen_directives)?;
    }
    Ok(())
}

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

fn validate_load_path(path: &Path) -> Result<(), PodError> {
    if !is_valid_load_path(path) {
        let token = path.to_string_lossy().into_owned();
        return Err(PodError::from(ConfigError {
            kind: ConfigErrorKind::InvalidPath,
            line: None,
            token: Some(token),
        }));
    }
    Ok(())
}

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

    use crate::error::{ConfigErrorKind, PodError, PslErrorKind};
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

        assert!(Config::load(&temp_dir.join("missing.conf")).is_err());

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
        ] {
            let err = Config::load(Path::new(malicious_load_path)).unwrap_err();
            match err {
                PodError::Config(config_err) => {
                    assert_eq!(config_err.kind, ConfigErrorKind::InvalidPath);
                }
                other => panic!("expected PodError::Config(InvalidPath), got {other:?}"),
            }
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
