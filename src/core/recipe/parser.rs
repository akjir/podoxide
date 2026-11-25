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

//! PSL `.pod` recipe file loading, path validation, and block-level AST translation.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use crate::core::container::Container;
use crate::core::pod::{Pod, PodCommand};
use crate::error::{PodError, RecipeError, RecipeErrorKind};
use crate::io::psl::{self, PslBlock, PslDirective, PslDocument, PslStatement};
use crate::io::system::fs;
use crate::utilities::{collection, path, string};
use super::directives::{
    self, SEEN_COMMAND_CONTAINER, SEEN_COMMAND_DESCRIPTION, SEEN_COMMAND_EXECUTE,
    SEEN_COMMAND_USER, SEEN_CONTAINER_REGISTRY, SEEN_CONTAINER_RESTART, SEEN_POD_DESCRIPTION,
    SEEN_POD_REGISTRY,
};
use super::{RECIPE_EXTENSION, Recipe};

/// Evaluates whether `recipes_dir` and `recipe_name` form a valid recipe load target.
#[must_use]
pub(super) fn is_valid_load_target(recipes_dir: &Path, recipe_name: &str) -> bool {
    let stem = recipe_name
        .strip_suffix(RECIPE_EXTENSION)
        .unwrap_or(recipe_name);
    path::is_valid_directory(recipes_dir) && string::is_valid_recipe_identifier(stem)
}

/// Loads and parses a `.pod` recipe file from `recipes_dir` and `recipe_name`, returning the parsed [`Recipe`], raw content, and sanitized display path.
pub(super) fn load(
    recipes_dir: &Path,
    recipe_name: &str,
) -> Result<(Recipe, String, String), PodError> {
    validate_load_target(recipes_dir, recipe_name)?;
    let recipe_path = path::build_full_path(recipes_dir, recipe_name, RECIPE_EXTENSION);
    let content = read_validated_file(&recipe_path)?;
    let display_path = path::sanitize_path_token(&recipe_path);
    let recipe = parse_recipe_with_path(&content, &display_path)?;
    Ok((recipe, content, display_path))
}

/// Parses and validates a recipe string, attaching a compiler-style diagnostic on failure.
pub(super) fn parse_recipe_with_path(input: &str, file_path: &str) -> Result<Recipe, PodError> {
    let document = psl::parse_psl(input)
        .map_err(|error| PodError::from(error.with_diagnostic(input, file_path)))?;
    let recipe = build_recipe(&document)
        .map_err(|error| PodError::from(error.with_diagnostic(input, file_path)))?;
    Ok(recipe)
}

/// Reads the raw `.pod` file at `recipe_path`, mapping a missing file to [`RecipeErrorKind::NotFound`].
pub(super) fn read_validated_file(recipe_path: &Path) -> Result<String, PodError> {
    match fs::read_file(recipe_path) {
        Ok(content) => Ok(content),
        Err(PodError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            Err(PodError::from(RecipeError::without_line(
                RecipeErrorKind::NotFound,
                path::sanitize_path_token(recipe_path),
            )))
        }
        Err(error) => Err(error),
    }
}

/// Validates that `recipe_path` is a safe `.pod` recipe file path before reading from disk.
pub(super) fn validate_load_path(recipe_path: &Path) -> Result<(), PodError> {
    if !is_valid_load_path(recipe_path) {
        return Err(PodError::from(RecipeError::without_line(
            RecipeErrorKind::InvalidPath,
            path::sanitize_path_token(recipe_path),
        )));
    }
    Ok(())
}

/// Validates and applies a directive inside a `command <name>` sub-block.
fn apply_command_directive(
    command: &mut PodCommand,
    directive: &PslDirective<'_>,
    seen: &mut u8,
) -> Result<(), RecipeError> {
    let kind = RecipeErrorKind::InvalidCommand;
    match directive.name {
        "container" => {
            command.container = directives::parse_unique_text_directive(
                directive,
                seen,
                SEEN_COMMAND_CONTAINER,
                kind,
            )?;
        }
        "description" => {
            command.description = directives::parse_unique_text_directive(
                directive,
                seen,
                SEEN_COMMAND_DESCRIPTION,
                kind,
            )?;
        }
        "execute" => {
            command.execute = directives::parse_unique_text_directive(
                directive,
                seen,
                SEEN_COMMAND_EXECUTE,
                kind,
            )?;
        }
        "user" => {
            command.user = Some(directives::parse_unique_token_directive(
                directive,
                seen,
                SEEN_COMMAND_USER,
                kind,
            )?);
        }
        _ => {
            return Err(RecipeError::with_token(
                RecipeErrorKind::UnknownDirective,
                directive.line,
                directive.name,
            ));
        }
    }
    Ok(())
}

/// Validates and applies a statement inside a `command <name>` sub-block.
fn apply_command_statement(
    command: &mut PodCommand,
    statement: &PslStatement<'_>,
    seen: &mut u8,
) -> Result<(), RecipeError> {
    match statement {
        PslStatement::Block(block) => Err(RecipeError::with_token(
            RecipeErrorKind::UnexpectedBlock,
            block.line,
            block.name,
        )),
        PslStatement::Directive(directive) => apply_command_directive(command, directive, seen),
    }
}

/// Validates and applies a directive inside a `container` block.
fn apply_container_directive(
    container: &mut Container,
    directive: &PslDirective<'_>,
    seen: &mut u8,
) -> Result<(), RecipeError> {
    match directive.name {
        "command" => directives::parse_multi_text_directive(
            &mut container.commands,
            directive,
            RecipeErrorKind::InvalidCommand,
        )?,
        "image" => container.image = directives::parse_unique_image_directive(directive, seen)?,
        "option" => directives::parse_option_directive(&mut container.options, directive)?,
        "registry" => {
            container.registry = directives::parse_unique_registry_directive(
                directive,
                seen,
                SEEN_CONTAINER_REGISTRY,
            )?;
        }
        "restart" => {
            let kind = RecipeErrorKind::InvalidRestartPolicy;
            container.restart = directives::parse_unique_token_directive(
                directive,
                seen,
                SEEN_CONTAINER_RESTART,
                kind,
            )?;
        }
        "volume" => {
            directives::ensure_entry_capacity(
                container.volumes.len(),
                1,
                directive.line,
                directive.name,
                RecipeErrorKind::InvalidVolume,
            )?;
            container
                .volumes
                .push(directives::parse_volume_directive(directive)?);
        }
        _ => {
            return Err(RecipeError::with_token(
                RecipeErrorKind::UnknownDirective,
                directive.line,
                directive.name,
            ));
        }
    }
    Ok(())
}

/// Validates and applies a statement inside a `container` block.
fn apply_container_statement(
    container: &mut Container,
    statement: &PslStatement<'_>,
    seen: &mut u8,
) -> Result<(), RecipeError> {
    match statement {
        PslStatement::Block(block) => Err(RecipeError::with_token(
            RecipeErrorKind::UnexpectedBlock,
            block.line,
            block.name,
        )),
        PslStatement::Directive(directive) => apply_container_directive(container, directive, seen),
    }
}

/// Validates and applies a directive inside the `pod` block.
fn apply_pod_directive(
    pod: &mut Pod,
    directive: &PslDirective<'_>,
    seen: &mut u8,
) -> Result<(), RecipeError> {
    match directive.name {
        "description" => {
            let kind = RecipeErrorKind::InvalidDescription;
            pod.description = directives::parse_unique_text_directive(
                directive,
                seen,
                SEEN_POD_DESCRIPTION,
                kind,
            )?;
        }
        "name" => pod.name = directives::parse_pod_name_directive(directive, seen)?,
        "option" => directives::parse_option_directive(&mut pod.options, directive)?,
        "path" => pod.path = directives::parse_pod_path_directive(directive, seen)?,
        "publish" => {
            directives::ensure_entry_capacity(
                pod.publish.len(),
                1,
                directive.line,
                directive.name,
                RecipeErrorKind::InvalidPublish,
            )?;
            pod.publish
                .push(directives::parse_publish_directive(directive)?);
        }
        "registry" => {
            pod.registry =
                directives::parse_unique_registry_directive(directive, seen, SEEN_POD_REGISTRY)?;
        }
        _ => {
            return Err(RecipeError::with_token(
                RecipeErrorKind::UnknownDirective,
                directive.line,
                directive.name,
            ));
        }
    }
    Ok(())
}

/// Validates and applies a statement (`command` sub-block or directive) inside the `pod` block.
fn apply_pod_statement(
    pod: &mut Pod,
    statement: &PslStatement<'_>,
    seen: &mut u8,
) -> Result<(), RecipeError> {
    match statement {
        PslStatement::Block(block) => {
            if block.name != "command" {
                return Err(RecipeError::with_token(
                    RecipeErrorKind::UnexpectedBlock,
                    block.line,
                    block.name,
                ));
            }
            let command = parse_command_block(block, &pod.commands)?;
            pod.commands.insert(command.name.clone(), command);
            Ok(())
        }
        PslStatement::Directive(directive) => apply_pod_directive(pod, directive, seen),
    }
}

/// Validates and dispatches a top-level block (`pod` or `container`) in a `.pod` recipe.
fn apply_root_block(recipe: &mut Recipe, block: &PslBlock<'_>) -> Result<(), RecipeError> {
    match block.name {
        "container" => {
            let container = parse_container_block(block)?;
            recipe.containers.push(container);
            Ok(())
        }
        "pod" => {
            if recipe.has_pod {
                return Err(RecipeError::with_token(
                    RecipeErrorKind::DuplicateBlock,
                    block.line,
                    block.name,
                ));
            }
            parse_pod_block(&mut recipe.pod, block)?;
            recipe.has_pod = true;
            Ok(())
        }
        _ => Err(RecipeError::with_token(
            RecipeErrorKind::UnexpectedBlock,
            block.line,
            block.name,
        )),
    }
}

/// Builds a [`Recipe`] from a parsed [`PslDocument`].
fn build_recipe(document: &PslDocument<'_>) -> Result<Recipe, RecipeError> {
    let mut recipe = Recipe::new();
    let initial_capacity = collection::bounded_capacity(
        document.blocks.len().saturating_sub(1),
        collection::MAX_COLLECTION_PREALLOC,
    );
    recipe.containers = Vec::with_capacity(initial_capacity);
    for block in &document.blocks {
        apply_root_block(&mut recipe, block)?;
    }
    Ok(recipe)
}

/// Evaluates whether `recipe_path` is a structurally valid `.pod` recipe file path.
#[must_use]
fn is_valid_load_path(recipe_path: &Path) -> bool {
    if !path::has_regular_target_name(recipe_path) {
        return false;
    }
    let Some(raw) = recipe_path.to_str() else {
        return false;
    };
    if raw != raw.trim() {
        return false;
    }
    let Some(file_name) = recipe_path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    file_name.ends_with(RECIPE_EXTENSION)
        && is_valid_load_target(path::parent_or_cur_dir(recipe_path), file_name)
}

/// Parses and validates a `command <name> { ... }` sub-block inside a `pod` block.
fn parse_command_block(
    block: &PslBlock<'_>,
    existing_commands: &BTreeMap<String, PodCommand>,
) -> Result<PodCommand, RecipeError> {
    let [argument] = block.arguments.as_slice() else {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            block.line,
            block.name,
        ));
    };
    let command_name = argument.as_ref();
    if !string::is_valid_identifier(command_name) || block.statements.is_empty() {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidCommand,
            block.line,
            command_name,
        ));
    }
    if existing_commands.contains_key(command_name) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::DuplicateBlock,
            block.line,
            command_name,
        ));
    }
    directives::ensure_entry_capacity(
        existing_commands.len(),
        1,
        block.line,
        command_name,
        RecipeErrorKind::InvalidCommand,
    )?;
    let mut command = PodCommand::new();
    command.line = block.line;
    command.name = command_name.to_owned();
    let mut seen = 0_u8;
    for statement in &block.statements {
        apply_command_statement(&mut command, statement, &mut seen)?;
    }
    Ok(command)
}

/// Parses and validates a `container [<id>] { ... }` block.
fn parse_container_block(block: &PslBlock<'_>) -> Result<Container, RecipeError> {
    let raw_name = directives::parse_container_name_arg(block)?;
    if block.statements.is_empty() {
        let token = if raw_name.is_empty() {
            block.name
        } else {
            raw_name.as_str()
        };
        return Err(RecipeError::with_token(
            RecipeErrorKind::EmptyContainerBlock,
            block.line,
            token,
        ));
    }
    let mut container = Container::new();
    container.line = block.line;
    container.name = raw_name;
    let mut seen = 0_u8;
    for statement in &block.statements {
        apply_container_statement(&mut container, statement, &mut seen)?;
    }
    Ok(container)
}

/// Parses and validates the root `pod <id> { ... }` block.
fn parse_pod_block(pod: &mut Pod, block: &PslBlock<'_>) -> Result<(), RecipeError> {
    let pod_id = directives::parse_pod_id_arg(block)?;
    if block.statements.is_empty() {
        return Err(RecipeError::with_token(
            RecipeErrorKind::EmptyPodBlock,
            block.line,
            &pod_id,
        ));
    }
    pod.id = pod_id;
    pod.line = block.line;
    let mut seen = 0_u8;
    for statement in &block.statements {
        apply_pod_statement(pod, statement, &mut seen)?;
    }
    Ok(())
}

/// Validates that `recipes_dir` and `recipe_name` are safe before constructing the `.pod` file path.
fn validate_load_target(recipes_dir: &Path, recipe_name: &str) -> Result<(), PodError> {
    if !is_valid_load_target(recipes_dir, recipe_name) {
        let invalid_path = path::build_full_path(recipes_dir, recipe_name, RECIPE_EXTENSION);
        return Err(PodError::from(RecipeError::without_line(
            RecipeErrorKind::InvalidPath,
            path::sanitize_path_token(&invalid_path),
        )));
    }
    Ok(())
}
