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

//! PSL configuration parsing and semantic validation for `pods.conf`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::error::{ConfigError, ConfigErrorKind, PodError};
use crate::io::psl::{self, PslBlock, PslDirective, PslDocument, PslStatement};
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::path as path_util;
use crate::utilities::string;
use super::{Config, PodsConfig, RecipesConfig, targets};

const SEEN_EDITOR: u8 = 1 << 0;
const SEEN_GENERAL: u8 = 1 << 0;
const SEEN_GROUPS: u8 = 1 << 0;
const SEEN_PODS: u8 = 1 << 1;
const SEEN_PODS_PATH: u8 = 1 << 0;
const SEEN_RECIPES: u8 = 1 << 2;
const SEEN_RECIPES_PATH: u8 = 1 << 1;
const SEEN_SIMULATE: u8 = 1 << 1;

/// Parses and validates a configuration string, attaching a compiler-style diagnostic on failure.
pub(super) fn parse_config_with_path(input: &str, file_path: &str) -> Result<Config, PodError> {
    let document = psl::parse_psl(input)
        .map_err(|error| PodError::from(error.with_diagnostic(input, file_path)))?;
    let config = build_config(&document)
        .map_err(|error| PodError::from(error.with_diagnostic(input, file_path)))?;
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

/// Validates and collects all `<recipe>` and `@<group>` target arguments for a group directive.
fn collect_group_targets(directive: &PslDirective<'_>) -> Result<Vec<String>, ConfigError> {
    let capacity = collection::bounded_capacity(directive.arguments.len(), MAX_COLLECTION_PREALLOC);
    let mut targets = Vec::with_capacity(capacity);
    for argument in &directive.arguments {
        let target = argument.as_ref();
        if !string::is_valid_group_target(target) {
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
    if !path_util::is_valid_editor_value(value) {
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

/// Parses and validates the `groups` sub-block inside `recipes`, ensuring it is non-empty, acyclic, and within depth bounds.
fn parse_groups_block(
    recipes: &mut RecipesConfig,
    block: &PslBlock<'_>,
) -> Result<(), ConfigError> {
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
    targets::validate_groups_acyclic(&recipes.groups, &group_lines, block.line)
}

/// Parses and validates a single-argument directory `path` directive.
fn parse_path_directive(directive: &PslDirective<'_>) -> Result<PathBuf, ConfigError> {
    let value = parse_single_argument(directive)?;
    if !path_util::is_valid_directory_path(value) {
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
fn parse_recipes_block(
    recipes: &mut RecipesConfig,
    block: &PslBlock<'_>,
) -> Result<(), ConfigError> {
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
    let [argument] = directive.arguments.as_slice() else {
        return Err(ConfigError::with_token(
            ConfigErrorKind::InvalidArgumentCount,
            directive.line,
            directive.name,
        ));
    };
    Ok(argument.as_ref())
}
