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

use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::core::config::Config;
use crate::core::container::{
    Container, VolumeMount, is_valid_volume_mount, resolve_container_name_value,
};
use crate::core::log;
use crate::core::pod::{Pod, PodCommand};
use crate::error::{PodError, RecipeError, RecipeErrorKind};
use crate::io::psl::{PslBlock, PslDirective, PslDocument, PslStatement, parse_psl};
use crate::io::system::fs;
use crate::utilities::path::{build_full_path, is_valid_directory_path, sanitize_path_token};
use crate::utilities::string::{
    contains_control_or_whitespace, is_bidi_control, is_valid_identifier,
    is_valid_pod_or_container_name, is_valid_recipe_identifier, normalize_name, parse_ascii_u32,
};

/// File extension for PodScript Language recipe files.
pub const RECIPE_EXTENSION: &str = ".pod";
const SEEN_COMMAND_CONTAINER: u8 = 1 << 0;
const SEEN_COMMAND_DESCRIPTION: u8 = 1 << 1;
const SEEN_COMMAND_EXECUTE: u8 = 1 << 2;
const SEEN_COMMAND_USER: u8 = 1 << 3;
const SEEN_CONTAINER_DETACH: u8 = 1 << 0;
const SEEN_CONTAINER_IMAGE: u8 = 1 << 1;
const SEEN_CONTAINER_REGISTRY: u8 = 1 << 2;
const SEEN_CONTAINER_RESTART: u8 = 1 << 3;
const SEEN_POD_DESCRIPTION: u8 = 1 << 0;
const SEEN_POD_NAME: u8 = 1 << 1;
const SEEN_POD_PATH: u8 = 1 << 2;
const SEEN_POD_REGISTRY: u8 = 1 << 3;

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

    /// Loads and parses a `.pod` recipe file from `recipes_dir` and `recipe_name`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] when the recipe path is invalid, the file is missing, or PSL parsing fails.
    pub fn load(recipes_dir: &Path, recipe_name: &str) -> Result<Self, PodError> {
        load(recipes_dir, recipe_name)
    }

    /// Resolves a container reference (`1..=N`, `*suffix`, or name) against this recipe's containers.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] with [`RecipeErrorKind::ContainerNotFound`] when no container matches.
    pub fn resolve_container_name(&self, container_spec: &str) -> Result<String, RecipeError> {
        resolve_container_name(self, container_spec)
    }

    /// Validates required pod and container fields, applies default pod paths, and resolves container names.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] when required blocks or directives are missing or invalid.
    pub fn validate(&mut self, config: &Config, file_name: &str) -> Result<(), PodError> {
        validate(self, config, file_name)
    }
}

impl FromStr for Recipe {
    type Err = PodError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let document = parse_psl(input)?;
        let recipe = build_recipe(&document)?;
        Ok(recipe)
    }
}

/// Loads and parses a `.pod` recipe file from `recipes_dir` and `recipe_name`.
///
/// # Errors
///
/// Returns [`PodError`] when the recipe path is invalid, the file is missing, or PSL parsing fails.
pub fn load(recipes_dir: &Path, recipe_name: &str) -> Result<Recipe, PodError> {
    validate_load_target(recipes_dir, recipe_name)?;
    let path = build_full_path(recipes_dir, recipe_name, RECIPE_EXTENSION);
    let content = read_recipe_file(&path)?;
    Recipe::from_str(&content)
}

/// Resolves a container reference (`1..=N`, `*suffix`, or name) against `recipe.containers`.
///
/// # Errors
///
/// Returns [`RecipeError`] with [`RecipeErrorKind::ContainerNotFound`] when no container matches.
pub fn resolve_container_name(recipe: &Recipe, container_spec: &str) -> Result<String, RecipeError> {
    if container_spec.trim().is_empty()
        || container_spec
            .chars()
            .any(|character| character.is_control() || is_bidi_control(character))
    {
        return Err(RecipeError::with_token(
            RecipeErrorKind::ContainerNotFound,
            recipe.pod.line,
            container_spec,
        ));
    }
    if let Some(index) =
        parse_ascii_u32(container_spec).and_then(|value| usize::try_from(value).ok())
    {
        return resolve_by_index(recipe, index, container_spec);
    }
    let normalized = normalize_name(container_spec);
    if let Some(suffix) = normalized.strip_prefix('*') {
        let target = format!("{}-{suffix}", recipe.pod.id);
        if let Some(found) = find_container_by_name(&recipe.containers, &recipe.pod.id, &target) {
            return Ok(found);
        }
        return Err(RecipeError::with_token(
            RecipeErrorKind::ContainerNotFound,
            recipe.pod.line,
            container_spec,
        ));
    }
    let prefixed = format!("{}-{normalized}", recipe.pod.id);
    if let Some(found) = find_container_by_exact_or_prefixed(
        &recipe.containers,
        &recipe.pod.id,
        &normalized,
        &prefixed,
    ) {
        return Ok(found);
    }
    Err(RecipeError::with_token(
        RecipeErrorKind::ContainerNotFound,
        recipe.pod.line,
        container_spec,
    ))
}

/// Validates required pod and container fields, applies default pod paths, and resolves container names.
///
/// # Errors
///
/// Returns [`PodError`] when required blocks or directives are missing or invalid.
pub fn validate(recipe: &mut Recipe, config: &Config, file_name: &str) -> Result<(), PodError> {
    if !recipe.has_pod || recipe.pod.id.is_empty() {
        return Err(PodError::from(RecipeError::with_token(
            RecipeErrorKind::MissingPodBlock,
            recipe.pod.line,
            file_name,
        )));
    }
    validate_pod_fields(&mut recipe.pod, config, file_name)?;
    if recipe.containers.is_empty() {
        return Err(PodError::from(RecipeError::with_token(
            RecipeErrorKind::MissingContainersBlock,
            recipe.pod.line,
            file_name,
        )));
    }
    validate_and_resolve_containers(&mut recipe.containers, &recipe.pod.id)?;
    Ok(())
}

/// Validates and applies a directive inside a `command <name>` sub-block.
fn apply_command_directive(
    command: &mut PodCommand,
    directive: &PslDirective<'_>,
    seen: &mut u8,
) -> Result<(), RecipeError> {
    match directive.name {
        "container" => {
            mark_seen(
                seen,
                SEEN_COMMAND_CONTAINER,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            command.container = parse_text_directive(directive, RecipeErrorKind::InvalidCommand)?;
            Ok(())
        }
        "description" => {
            mark_seen(
                seen,
                SEEN_COMMAND_DESCRIPTION,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            command.description = parse_text_directive(directive, RecipeErrorKind::InvalidCommand)?;
            Ok(())
        }
        "execute" => {
            mark_seen(
                seen,
                SEEN_COMMAND_EXECUTE,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            command.execute = parse_text_directive(directive, RecipeErrorKind::InvalidCommand)?;
            Ok(())
        }
        "user" => {
            mark_seen(
                seen,
                SEEN_COMMAND_USER,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            let user = parse_token_directive(directive, RecipeErrorKind::InvalidCommand)?;
            command.user = Some(user);
            Ok(())
        }
        _ => Err(RecipeError::with_token(
            RecipeErrorKind::UnknownDirective,
            directive.line,
            directive.name,
        )),
    }
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
        "command" => parse_multi_text_directive(
            &mut container.commands,
            directive,
            RecipeErrorKind::InvalidCommand,
        ),
        "detach" => {
            mark_seen(
                seen,
                SEEN_CONTAINER_DETACH,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            container.detach = parse_bool_directive(directive)?;
            Ok(())
        }
        "image" => {
            mark_seen(
                seen,
                SEEN_CONTAINER_IMAGE,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            container.image = parse_token_directive(directive, RecipeErrorKind::InvalidImage)?;
            Ok(())
        }
        "option" => parse_multi_text_directive(
            &mut container.options,
            directive,
            RecipeErrorKind::InvalidOption,
        ),
        "registry" => {
            mark_seen(
                seen,
                SEEN_CONTAINER_REGISTRY,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            container.registry =
                parse_token_directive(directive, RecipeErrorKind::InvalidRegistry)?;
            Ok(())
        }
        "restart" => {
            mark_seen(
                seen,
                SEEN_CONTAINER_RESTART,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            container.restart =
                parse_token_directive(directive, RecipeErrorKind::InvalidRestartPolicy)?;
            Ok(())
        }
        "volume" => {
            let volume = parse_volume_directive(directive)?;
            container.volumes.push(volume);
            Ok(())
        }
        _ => Err(RecipeError::with_token(
            RecipeErrorKind::UnknownDirective,
            directive.line,
            directive.name,
        )),
    }
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
            mark_seen(
                seen,
                SEEN_POD_DESCRIPTION,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            pod.description =
                parse_text_directive(directive, RecipeErrorKind::InvalidDescription)?;
            Ok(())
        }
        "name" => {
            mark_seen(
                seen,
                SEEN_POD_NAME,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            pod.name = parse_pod_name_directive(directive)?;
            Ok(())
        }
        "option" => {
            parse_multi_text_directive(&mut pod.options, directive, RecipeErrorKind::InvalidOption)
        }
        "path" => {
            mark_seen(
                seen,
                SEEN_POD_PATH,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            pod.path = parse_pod_path_directive(directive)?;
            Ok(())
        }
        "publish" => {
            let entry = parse_publish_directive(directive)?;
            pod.publish.push(entry);
            Ok(())
        }
        "registry" => {
            mark_seen(
                seen,
                SEEN_POD_REGISTRY,
                RecipeErrorKind::DuplicateDirective,
                directive.line,
                directive.name,
            )?;
            pod.registry = parse_token_directive(directive, RecipeErrorKind::InvalidRegistry)?;
            Ok(())
        }
        _ => Err(RecipeError::with_token(
            RecipeErrorKind::UnknownDirective,
            directive.line,
            directive.name,
        )),
    }
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
            let command = parse_command_block(block)?;
            if pod.commands.contains_key(&command.name) {
                return Err(RecipeError::with_token(
                    RecipeErrorKind::DuplicateBlock,
                    block.line,
                    &command.name,
                ));
            }
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
    recipe.containers = Vec::with_capacity(document.blocks.len().saturating_sub(1));
    for block in &document.blocks {
        apply_root_block(&mut recipe, block)?;
    }
    Ok(recipe)
}

/// Verifies that the resolved container name at `index` does not collide with an earlier container.
fn ensure_unique_container_name(
    containers: &[Container],
    index: usize,
) -> Result<(), RecipeError> {
    let current = &containers[index];
    let is_duplicate = containers[..index]
        .iter()
        .any(|previous| previous.name == current.name);
    if is_duplicate {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidContainerName,
            current.line,
            &current.name,
        ));
    }
    Ok(())
}

/// Searches `containers` in a single pass for an exact match on `exact` or a fallback match on `prefixed`.
#[must_use]
fn find_container_by_exact_or_prefixed(
    containers: &[Container],
    pod_id: &str,
    exact: &str,
    prefixed: &str,
) -> Option<String> {
    let mut fallback = None;
    for (index, container) in containers.iter().enumerate() {
        let Ok(resolved) = resolved_container_name(container, pod_id, index + 1) else {
            continue;
        };
        if resolved == exact {
            return Some(resolved);
        }
        if fallback.is_none() && resolved == prefixed {
            fallback = Some(resolved);
        }
    }
    fallback
}

/// Searches `containers` for a container whose resolved name matches `target`.
#[must_use]
fn find_container_by_name(
    containers: &[Container],
    pod_id: &str,
    target: &str,
) -> Option<String> {
    for (index, container) in containers.iter().enumerate() {
        let Ok(resolved) = resolved_container_name(container, pod_id, index + 1) else {
            continue;
        };
        if resolved == target {
            return Some(resolved);
        }
    }
    None
}

/// Formats a 2-argument `publish` directive into a single Podman `--publish` specification string.
#[must_use]
fn format_two_arg_publish(first: &str, second: &str) -> String {
    if first.is_empty() {
        return second.to_owned();
    }
    if second.eq_ignore_ascii_case("TCP") || second.eq_ignore_ascii_case("UDP") {
        return format!("{first}/{second}");
    }
    format!("{first}:{second}")
}

/// Evaluates whether `value` contains at least one non-whitespace character and is free of control and Unicode BiDi characters.
#[must_use]
fn is_safe_text_argument(value: &str) -> bool {
    !value.trim().is_empty()
        && !value
            .chars()
            .any(|character| character.is_control() || is_bidi_control(character))
}

/// Evaluates whether `value` is a non-empty single token without leading hyphens, whitespace, or control characters.
#[must_use]
fn is_safe_token_argument(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-') && !contains_control_or_whitespace(value)
}

/// Evaluates whether `recipes_dir` is a valid, UTF-8 directory path without `..` traversal.
#[must_use]
fn is_valid_recipes_dir(recipes_dir: &Path) -> bool {
    let Some(raw_dir) = recipes_dir.to_str() else {
        return false;
    };
    is_valid_directory_path(raw_dir)
}

/// Sets `flag` in `seen` or returns a duplicate [`RecipeError`] if already set.
fn mark_seen(
    seen: &mut u8,
    flag: u8,
    kind: RecipeErrorKind,
    line: usize,
    token: &str,
) -> Result<(), RecipeError> {
    if *seen & flag != 0 {
        return Err(RecipeError::with_token(kind, line, token));
    }
    *seen |= flag;
    Ok(())
}

/// Parses a single-argument boolean directive (`"true"` or `"false"`).
fn parse_bool_directive(directive: &PslDirective<'_>) -> Result<bool, RecipeError> {
    let value = parse_single_argument(directive)?;
    match value {
        "false" => Ok(false),
        "true" => Ok(true),
        _ => Err(RecipeError::with_token(
            RecipeErrorKind::InvalidBoolean,
            directive.line,
            value,
        )),
    }
}

/// Parses and validates a `command <name> { ... }` sub-block inside a `pod` block.
fn parse_command_block(block: &PslBlock<'_>) -> Result<PodCommand, RecipeError> {
    if block.arguments.len() != 1 {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            block.line,
            block.name,
        ));
    }
    let command_name = block.arguments[0].as_ref();
    if !is_valid_identifier(command_name) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidCommand,
            block.line,
            command_name,
        ));
    }
    if block.statements.is_empty() {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidCommand,
            block.line,
            command_name,
        ));
    }
    let mut command = PodCommand::new();
    command.name = command_name.to_owned();
    let mut seen = 0_u8;
    for statement in &block.statements {
        apply_command_statement(&mut command, statement, &mut seen)?;
    }
    Ok(command)
}

/// Parses and validates a `container [<id>] { ... }` block.
fn parse_container_block(block: &PslBlock<'_>) -> Result<Container, RecipeError> {
    let raw_name = parse_container_name_arg(block)?;
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

/// Extracts and validates the optional `<id>` argument from a `container [<id>]` block header.
fn parse_container_name_arg(block: &PslBlock<'_>) -> Result<String, RecipeError> {
    if block.arguments.len() > 1 {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            block.line,
            block.name,
        ));
    }
    let Some(raw_arg) = block.arguments.first() else {
        return Ok(String::new());
    };
    let raw_name = raw_arg.as_ref();
    if raw_name.trim().is_empty() {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidContainerName,
            block.line,
            raw_name,
        ));
    }
    let _ = resolve_container_name_value(raw_name, "pod", 1, block.line)?;
    Ok(raw_name.to_owned())
}

/// Appends `1..N` validated text arguments from `directive` into `target`.
fn parse_multi_text_directive(
    target: &mut Vec<String>,
    directive: &PslDirective<'_>,
    invalid_kind: RecipeErrorKind,
) -> Result<(), RecipeError> {
    if directive.arguments.is_empty() {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            directive.line,
            directive.name,
        ));
    }
    target.reserve(directive.arguments.len());
    for argument in &directive.arguments {
        let value = argument.as_ref();
        if !is_safe_text_argument(value) {
            return Err(RecipeError::with_token(invalid_kind, directive.line, value));
        }
        target.push(value.to_owned());
    }
    Ok(())
}

/// Parses and validates the root `pod <id> { ... }` block.
fn parse_pod_block(pod: &mut Pod, block: &PslBlock<'_>) -> Result<(), RecipeError> {
    let pod_id = parse_pod_id_arg(block)?;
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

/// Extracts, normalizes, and validates the `<id>` argument from a `pod <id>` block header.
fn parse_pod_id_arg(block: &PslBlock<'_>) -> Result<String, RecipeError> {
    if block.arguments.len() != 1 {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            block.line,
            block.name,
        ));
    }
    let raw_id = block.arguments[0].as_ref();
    if raw_id
        .chars()
        .any(|character| character.is_control() || is_bidi_control(character))
    {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPodId,
            block.line,
            raw_id,
        ));
    }
    let normalized = normalize_name(raw_id);
    if !is_valid_pod_or_container_name(&normalized) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPodId,
            block.line,
            raw_id,
        ));
    }
    Ok(normalized)
}

/// Parses and validates the `pod.name` display name directive.
fn parse_pod_name_directive(directive: &PslDirective<'_>) -> Result<String, RecipeError> {
    let value = parse_single_argument(directive)?;
    let trimmed = value.trim();
    if !is_safe_text_argument(trimmed) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::MissingPodName,
            directive.line,
            directive.name,
        ));
    }
    Ok(trimmed.to_owned())
}

/// Parses and validates the `pod.path` directory path directive.
fn parse_pod_path_directive(directive: &PslDirective<'_>) -> Result<PathBuf, RecipeError> {
    let value = parse_single_argument(directive)?;
    if !is_valid_directory_path(value) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPath,
            directive.line,
            value,
        ));
    }
    Ok(PathBuf::from(value))
}

/// Parses and validates a `publish` directive (`1..=3` arguments) into a `--publish` value string.
fn parse_publish_directive(directive: &PslDirective<'_>) -> Result<String, RecipeError> {
    let formatted = match directive.arguments.as_slice() {
        [single] => {
            let single = single.as_ref();
            if !is_safe_token_argument(single) {
                return Err(RecipeError::with_token(
                    RecipeErrorKind::InvalidPublish,
                    directive.line,
                    directive.name,
                ));
            }
            single.to_owned()
        }
        [first, second] => {
            let first = first.as_ref();
            let second = second.as_ref();
            if !(first.is_empty() || is_safe_token_argument(first))
                || !is_safe_token_argument(second)
            {
                return Err(RecipeError::with_token(
                    RecipeErrorKind::InvalidPublish,
                    directive.line,
                    directive.name,
                ));
            }
            format_two_arg_publish(first, second)
        }
        [first, second, third] => {
            let first = first.as_ref();
            let second = second.as_ref();
            let third = third.as_ref();
            if !is_safe_token_argument(first)
                || !is_safe_token_argument(second)
                || !is_safe_token_argument(third)
            {
                return Err(RecipeError::with_token(
                    RecipeErrorKind::InvalidPublish,
                    directive.line,
                    directive.name,
                ));
            }
            format!("{first}:{second}/{third}")
        }
        _ => {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidArgumentCount,
                directive.line,
                directive.name,
            ));
        }
    };
    if !is_safe_token_argument(&formatted) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPublish,
            directive.line,
            &formatted,
        ));
    }
    Ok(formatted)
}

/// Extracts the sole argument of a directive or returns [`RecipeErrorKind::InvalidArgumentCount`].
fn parse_single_argument<'a>(directive: &'a PslDirective<'a>) -> Result<&'a str, RecipeError> {
    if directive.arguments.len() != 1 {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            directive.line,
            directive.name,
        ));
    }
    Ok(directive.arguments[0].as_ref())
}

/// Parses a single-argument text directive, rejecting empty strings and control/BiDi characters.
fn parse_text_directive(
    directive: &PslDirective<'_>,
    invalid_kind: RecipeErrorKind,
) -> Result<String, RecipeError> {
    let value = parse_single_argument(directive)?;
    if !is_safe_text_argument(value) {
        return Err(RecipeError::with_token(invalid_kind, directive.line, value));
    }
    Ok(value.to_owned())
}

/// Parses a single-argument token directive (`registry`, `image`, `restart`, `user`).
fn parse_token_directive(
    directive: &PslDirective<'_>,
    invalid_kind: RecipeErrorKind,
) -> Result<String, RecipeError> {
    let value = parse_single_argument(directive)?;
    if !is_safe_token_argument(value) {
        return Err(RecipeError::with_token(invalid_kind, directive.line, value));
    }
    Ok(value.to_owned())
}

/// Parses and validates a `volume` directive (`1..=3` arguments) into a [`VolumeMount`].
fn parse_volume_directive(directive: &PslDirective<'_>) -> Result<VolumeMount, RecipeError> {
    let volume = match directive.arguments.len() {
        1 => {
            let (container_dir, options) =
                split_container_dir_and_options(directive.arguments[0].as_ref());
            VolumeMount {
                container_dir: container_dir.to_owned(),
                host_dir: String::new(),
                options: options.to_owned(),
            }
        }
        2 => {
            let (container_dir, options) =
                split_container_dir_and_options(directive.arguments[1].as_ref());
            VolumeMount {
                container_dir: container_dir.to_owned(),
                host_dir: directive.arguments[0].as_ref().to_owned(),
                options: options.to_owned(),
            }
        }
        3 => VolumeMount {
            container_dir: directive.arguments[1].as_ref().to_owned(),
            host_dir: directive.arguments[0].as_ref().to_owned(),
            options: directive.arguments[2].as_ref().to_owned(),
        },
        _ => {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidArgumentCount,
                directive.line,
                directive.name,
            ));
        }
    };
    if !is_valid_volume_mount(&volume) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidVolume,
            directive.line,
            directive.name,
        ));
    }
    Ok(volume)
}

/// Reads the `.pod` file at `path`, mapping a missing file to [`RecipeErrorKind::NotFound`].
fn read_recipe_file(path: &Path) -> Result<String, PodError> {
    match fs::read_file(path) {
        Ok(content) => Ok(content),
        Err(PodError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            Err(PodError::from(RecipeError {
                kind: RecipeErrorKind::NotFound,
                line: None,
                token: Some(sanitize_path_token(path)),
            }))
        }
        Err(error) => Err(error),
    }
}

/// Resolves a 1-based numeric container index (`1..=N`) against `recipe.containers`.
fn resolve_by_index(
    recipe: &Recipe,
    index: usize,
    raw_spec: &str,
) -> Result<String, RecipeError> {
    if index == 0 || index > recipe.containers.len() {
        return Err(RecipeError::with_token(
            RecipeErrorKind::ContainerNotFound,
            recipe.pod.line,
            raw_spec,
        ));
    }
    resolved_container_name(&recipe.containers[index - 1], &recipe.pod.id, index)
}

/// Validates `pod.path` or populates it from `<config.pods.path>/<pod.id>` when omitted.
fn resolve_pod_path(pod: &mut Pod, config: &Config, file_name: &str) -> Result<(), RecipeError> {
    let used_fallback = pod.path.as_os_str().is_empty();
    if used_fallback {
        if config.pods.path.as_os_str().is_empty() {
            return Err(RecipeError::with_token(
                RecipeErrorKind::MissingPodPath,
                pod.line,
                file_name,
            ));
        }
        pod.path = build_full_path(&config.pods.path, &pod.id, "");
    }
    if !pod.path.to_str().is_some_and(is_valid_directory_path) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPath,
            pod.line,
            sanitize_path_token(&pod.path),
        ));
    }
    if used_fallback && log::is_debug_enabled() {
        log::debug(&format!(
            "No pod path in recipe '{file_name}' set. Path '{}' used.",
            pod.path.display()
        ));
    }
    Ok(())
}

/// Computes the resolved name for `container` at 1-based `index` without mutating it.
fn resolved_container_name(
    container: &Container,
    pod_id: &str,
    index: usize,
) -> Result<String, RecipeError> {
    resolve_container_name_value(&container.name, pod_id, index, container.line)
}

/// Splits a `<container_dir>[:<options>]` argument into `(container_dir, options)`.
#[must_use]
fn split_container_dir_and_options(raw_target: &str) -> (&str, &str) {
    let Some((container_dir, options)) = raw_target.split_once(':') else {
        return (raw_target, "");
    };
    if options.is_empty() {
        return ("", "");
    }
    (container_dir, options)
}

/// Resolves container names, validates each container, and rejects duplicate container names.
fn validate_and_resolve_containers(
    containers: &mut [Container],
    pod_id: &str,
) -> Result<(), RecipeError> {
    for index in 0..containers.len() {
        containers[index].ensure_name(pod_id, index + 1)?;
        containers[index].validate()?;
        ensure_unique_container_name(containers, index)?;
    }
    Ok(())
}

/// Validates that `recipes_dir` and `recipe_name` are safe before constructing the `.pod` file path.
fn validate_load_target(recipes_dir: &Path, recipe_name: &str) -> Result<(), PodError> {
    let stem = recipe_name
        .strip_suffix(RECIPE_EXTENSION)
        .unwrap_or(recipe_name);
    if !is_valid_recipes_dir(recipes_dir) || !is_valid_recipe_identifier(stem) {
        let invalid_path = build_full_path(recipes_dir, recipe_name, RECIPE_EXTENSION);
        return Err(PodError::from(RecipeError {
            kind: RecipeErrorKind::InvalidPath,
            line: None,
            token: Some(sanitize_path_token(&invalid_path)),
        }));
    }
    Ok(())
}

/// Validates pod fields (`id`, `name`, `description`, `registry`, `publish`, `options`, and `path`) during [`validate`].
fn validate_pod_fields(
    pod: &mut Pod,
    config: &Config,
    file_name: &str,
) -> Result<(), RecipeError> {
    if pod
        .id
        .chars()
        .any(|character| character.is_control() || is_bidi_control(character))
    {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPodId,
            pod.line,
            &pod.id,
        ));
    }
    let normalized_id = normalize_name(&pod.id);
    if !is_valid_pod_or_container_name(&normalized_id) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPodId,
            pod.line,
            &pod.id,
        ));
    }
    pod.id = normalized_id;

    let trimmed_name = pod.name.trim();
    if !is_safe_text_argument(trimmed_name) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::MissingPodName,
            pod.line,
            file_name,
        ));
    }
    if pod.name.len() != trimmed_name.len() {
        pod.name = trimmed_name.to_owned();
    }

    if !pod.description.is_empty() && !is_safe_text_argument(&pod.description) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidDescription,
            pod.line,
            &pod.description,
        ));
    }

    if pod.registry.is_empty() {
        return Err(RecipeError::with_token(
            RecipeErrorKind::MissingPodRegistry,
            pod.line,
            file_name,
        ));
    }
    if !is_safe_token_argument(&pod.registry) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidRegistry,
            pod.line,
            &pod.registry,
        ));
    }

    validate_pod_publish(&pod.publish, pod.line)?;
    validate_pod_options(&pod.options, pod.line)?;
    resolve_pod_path(pod, config, file_name)
}

/// Validates that every `pod.options` entry is a safe text argument.
fn validate_pod_options(options: &[String], line: usize) -> Result<(), RecipeError> {
    for option in options {
        if !is_safe_text_argument(option) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidOption,
                line,
                option,
            ));
        }
    }
    Ok(())
}

/// Validates that every `pod.publish` entry is a safe token argument.
fn validate_pod_publish(publish: &[String], line: usize) -> Result<(), RecipeError> {
    for entry in publish {
        if !is_safe_token_argument(entry) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidPublish,
                line,
                entry,
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process;
    use std::str::FromStr;

    use crate::core::config::Config;
    use crate::core::container::VolumeMount;
    use crate::error::{PodError, PslErrorKind, RecipeErrorKind};
    use crate::io::system::fs as system_fs;
    use super::{Recipe, load, resolve_container_name, validate};

    fn assert_recipe_error(input: &str, expected_kind: RecipeErrorKind) {
        let error = Recipe::from_str(input).unwrap_err();
        match error {
            PodError::Recipe(recipe_error) => assert_eq!(recipe_error.kind, expected_kind),
            other => panic!("expected PodError::Recipe({expected_kind:?}), got {other:?}"),
        }
    }

    fn assert_validate_error(
        recipe: &mut Recipe,
        config: &Config,
        file_name: &str,
        expected_kind: RecipeErrorKind,
    ) {
        let error = validate(recipe, config, file_name).unwrap_err();
        match error {
            PodError::Recipe(recipe_error) => assert_eq!(recipe_error.kind, expected_kind),
            other => panic!("expected PodError::Recipe({expected_kind:?}), got {other:?}"),
        }
    }

    #[test]
    fn load_root_recipe_pod_and_temp_recipes() {
        let config = Config::load(Path::new("./pods.conf")).unwrap();
        let mut root_recipe = Recipe::load(&config.recipes.path, "recipe").unwrap();
        root_recipe.validate(&config, "recipe").unwrap();

        assert_eq!(root_recipe.pod.id, "example-stack");
        assert_eq!(root_recipe.pod.name, "Example Web Stack");
        assert_eq!(
            root_recipe.pod.description,
            "A minimal but complete example recipe with a web app and database."
        );
        assert_eq!(root_recipe.pod.path, PathBuf::from("/pods/example-stack"));
        assert_eq!(root_recipe.pod.registry, "docker.io");
        assert_eq!(root_recipe.pod.publish, vec![String::from("8080:80/TCP")]);
        assert_eq!(
            root_recipe.pod.options,
            vec![String::from("--userns=keep-id")]
        );

        let migrate_command = root_recipe.pod.commands.get("migrate").unwrap();
        assert_eq!(migrate_command.name, "migrate");
        assert_eq!(migrate_command.description, "Run database migrations.");
        assert_eq!(migrate_command.container, "*db");
        assert_eq!(migrate_command.user.as_deref(), Some("dbuser"));
        assert_eq!(
            migrate_command.execute,
            "db-cli -U dbuser -d app -f /migrations/run.sql"
        );

        assert_eq!(root_recipe.containers.len(), 2);
        let db_container = &root_recipe.containers[0];
        assert_eq!(db_container.name, "example-stack-db");
        assert!(db_container.detach);
        assert_eq!(db_container.restart, "always");
        assert_eq!(db_container.registry, "docker.io");
        assert_eq!(db_container.image, "example/db-server:latest");
        assert_eq!(
            db_container.volumes,
            vec![VolumeMount {
                container_dir: String::from("/var/lib/db/data"),
                host_dir: String::from("data"),
                options: String::from("Z"),
            }]
        );
        assert_eq!(
            db_container.options,
            vec![String::from("--env DB_PASSWORD=secret")]
        );

        let app_container = &root_recipe.containers[1];
        assert_eq!(app_container.name, "example-stack-app");
        assert!(app_container.detach);
        assert_eq!(app_container.restart, "always");
        assert_eq!(app_container.image, "example/example-app:latest");
        assert_eq!(
            app_container.commands,
            vec![String::from("start-server.sh")]
        );
        assert_eq!(
            root_recipe.resolve_container_name("*app").unwrap(),
            "example-stack-app"
        );
        assert_eq!(
            root_recipe.resolve_container_name("1").unwrap(),
            "example-stack-db"
        );

        let temp_dir = env::temp_dir().join(format!("podoxide_recipe_test_{}", process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let missing_error = load(&temp_dir, "missing_recipe").unwrap_err();
        match &missing_error {
            PodError::Recipe(error) => assert_eq!(error.kind, RecipeErrorKind::NotFound),
            other => panic!("expected NotFound, got {other:?}"),
        }
        assert!(
            missing_error
                .to_string()
                .contains("Recipe error: recipe file not found:")
        );

        for invalid_name in [
            "../escape",
            "",
            ".",
            "..",
            ".pod",
            "..pod",
            ".hidden.pod",
            "app.extra.pod",
        ] {
            let invalid_target_error = load(&temp_dir, invalid_name).unwrap_err();
            match invalid_target_error {
                PodError::Recipe(error) => assert_eq!(error.kind, RecipeErrorKind::InvalidPath),
                other => panic!("expected InvalidPath for '{invalid_name}', got {other:?}"),
            }
        }

        for invalid_dir in [Path::new("../escape"), Path::new("/bad:dir")] {
            let invalid_dir_error = load(invalid_dir, "webapp").unwrap_err();
            match invalid_dir_error {
                PodError::Recipe(error) => assert_eq!(error.kind, RecipeErrorKind::InvalidPath),
                other => panic!("expected InvalidPath for '{invalid_dir:?}', got {other:?}"),
            }
        }

        let valid_path = temp_dir.join("webapp.pod");
        let webapp_content = "\
pod \"  Si  Po \" {
    name \" Simple Pod  \"
    registry registry.io
    publish 8433 433
    publish 8080 80 TCP
    publish \"127.0.0.1:\" 42
    publish 127.0.0.1:62 43 UDP
    publish 600-500
    publish \"\" 83
    publish 124
    publish 12 UDP
    option --network slirp4netns:port_handler=slirp4netns
    command migrate {
        execute \"sh migrate.sh\"
    }
}
container \" supR App \" {
    detach false
    restart never
    image alpine:latest
    volume \"\" /container/dir/anonymous \"\"
    volume named_volume /container/dir/named ro
    volume /absolute/path /container/dir/absolute
    volume ./relative/path /container/dir/relative:z
    option --env \"FOO=bar\"
    command \"O'Hara\"
    command \"Hancock 2\"
}
container *db {
    registry db.io
    image postgres:16
}
container {
    image redis:7
}
";
        system_fs::write_file(&valid_path, webapp_content).unwrap();

        let mut loaded = load(&temp_dir, "webapp.pod").unwrap();
        let mut custom_config = Config::new();
        custom_config.pods.path = PathBuf::from("/pods");
        loaded.validate(&custom_config, "webapp").unwrap();

        assert_eq!(loaded.pod.id, "si_po");
        assert_eq!(loaded.pod.name, "Simple Pod");
        assert_eq!(loaded.pod.path, PathBuf::from("/pods/si_po"));
        assert_eq!(
            loaded.pod.publish,
            vec![
                "8433:433",
                "8080:80/TCP",
                "127.0.0.1::42",
                "127.0.0.1:62:43/UDP",
                "600-500",
                "83",
                "124",
                "12/UDP",
            ]
        );
        assert_eq!(
            loaded.pod.options,
            vec!["--network", "slirp4netns:port_handler=slirp4netns"]
        );
        assert!(loaded.pod.commands.contains_key("migrate"));

        assert_eq!(loaded.containers.len(), 3);
        assert_eq!(loaded.containers[0].name, "supr_app");
        assert!(!loaded.containers[0].detach);
        assert_eq!(loaded.containers[0].volumes.len(), 4);
        assert_eq!(loaded.containers[0].options, vec!["--env", "FOO=bar"]);
        assert_eq!(loaded.containers[0].commands, vec!["O'Hara", "Hancock 2"]);
        assert_eq!(loaded.containers[1].name, "si_po-db");
        assert_eq!(loaded.containers[1].registry, "db.io");
        assert_eq!(loaded.containers[2].name, "si_po-3");

        assert_eq!(resolve_container_name(&loaded, "1").unwrap(), "supr_app");
        assert_eq!(resolve_container_name(&loaded, "2").unwrap(), "si_po-db");
        assert_eq!(resolve_container_name(&loaded, "3").unwrap(), "si_po-3");
        assert_eq!(
            resolve_container_name(&loaded, "SuPr ApP").unwrap(),
            "supr_app"
        );
        assert_eq!(resolve_container_name(&loaded, "*db").unwrap(), "si_po-db");
        assert_eq!(resolve_container_name(&loaded, "db").unwrap(), "si_po-db");
        assert_eq!(resolve_container_name(&loaded, "3").unwrap(), "si_po-3");

        for unknown in ["", "   ", "0", "+1", "4", "*missing", "missing", "bad\nname"] {
            let error = resolve_container_name(&loaded, unknown).unwrap_err();
            assert_eq!(error.kind, RecipeErrorKind::ContainerNotFound);
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn recipe_validation_rejects_missing_and_invalid_fields() {
        let mut config_no_pod_path = Config::new();
        config_no_pod_path.pods.path = PathBuf::new();

        let mut config_with_pod_path = Config::new();
        config_with_pod_path.pods.path = PathBuf::from("/pods");

        let mut no_pod = Recipe::from_str("container {\n    image alpine:latest\n}\n").unwrap();
        assert_validate_error(
            &mut no_pod,
            &config_with_pod_path,
            "recipe_002_no_pod",
            RecipeErrorKind::MissingPodBlock,
        );

        let mut no_name = Recipe::from_str(
            "pod test {\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        assert_validate_error(
            &mut no_name,
            &config_with_pod_path,
            "recipe_001_empty",
            RecipeErrorKind::MissingPodName,
        );

        let mut no_registry = Recipe::from_str(
            "pod test {\n    name Test\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        assert_validate_error(
            &mut no_registry,
            &config_with_pod_path,
            "recipe_004_no_registry",
            RecipeErrorKind::MissingPodRegistry,
        );

        let mut no_path = Recipe::from_str(
            "pod nopathpod {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        assert_validate_error(
            &mut no_path,
            &config_no_pod_path,
            "recipe_005_no_path",
            RecipeErrorKind::MissingPodPath,
        );

        let mut invalid_description = Recipe::from_str(
            "pod test {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        invalid_description.pod.description = String::from("   ");
        assert_validate_error(
            &mut invalid_description,
            &config_with_pod_path,
            "invalid_description",
            RecipeErrorKind::InvalidDescription,
        );

        let mut invalid_publish = Recipe::from_str(
            "pod test {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        invalid_publish.pod.publish.push(String::from("-8080:80"));
        assert_validate_error(
            &mut invalid_publish,
            &config_with_pod_path,
            "invalid_publish",
            RecipeErrorKind::InvalidPublish,
        );

        let mut invalid_option = Recipe::from_str(
            "pod test {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        invalid_option.pod.options.push(String::from("   "));
        assert_validate_error(
            &mut invalid_option,
            &config_with_pod_path,
            "invalid_option",
            RecipeErrorKind::InvalidOption,
        );

        let mut no_containers =
            Recipe::from_str("pod nocontainers {\n    name Test\n    registry docker.io\n}\n")
                .unwrap();
        assert_validate_error(
            &mut no_containers,
            &config_with_pod_path,
            "recipe_006_no_containers",
            RecipeErrorKind::MissingContainersBlock,
        );

        let mut missing_image = Recipe::from_str(
            "pod test {\n    name Test\n    registry docker.io\n}\ncontainer app {\n    restart never\n}\n",
        )
        .unwrap();
        assert_validate_error(
            &mut missing_image,
            &config_with_pod_path,
            "missing_image",
            RecipeErrorKind::MissingContainerImage,
        );

        let mut duplicate_containers = Recipe::from_str(
            "\
pod my_pod {
    name Test
    registry docker.io
}
container my_pod-2 {
    image alpine:1
}
container *2 {
    image alpine:2
}
",
        )
        .unwrap();
        assert_validate_error(
            &mut duplicate_containers,
            &config_with_pod_path,
            "dup_containers",
            RecipeErrorKind::InvalidContainerName,
        );
    }

    #[test]
    fn recipe_parser_rejects_structural_and_security_violations() {
        assert_recipe_error("unknown_block {\n}\n", RecipeErrorKind::UnexpectedBlock);
        assert_recipe_error(
            "pod {\n    name Test\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        );
        assert_recipe_error(
            "pod a b {\n    name Test\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        );
        assert_recipe_error("pod -bad {\n    name Test\n}\n", RecipeErrorKind::InvalidPodId);
        assert_recipe_error(
            "pod \"bad\\nname\" {\n    name Test\n}\n",
            RecipeErrorKind::InvalidPodId,
        );
        assert_recipe_error("pod app {\n}\n", RecipeErrorKind::EmptyPodBlock);
        assert_recipe_error(
            "pod a {\n    name A\n}\npod b {\n    name B\n}\n",
            RecipeErrorKind::DuplicateBlock,
        );
        assert_recipe_error(
            "pod app {\n    unknown value\n}\n",
            RecipeErrorKind::UnknownDirective,
        );
        assert_recipe_error(
            "pod app {\n    name A\n    name B\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "pod app {\n    description A\n    description B\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "pod app {\n    path /pods/a\n    path /pods/b\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "pod app {\n    registry a.io\n    registry b.io\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "pod app {\n    description \"\"\n}\n",
            RecipeErrorKind::InvalidDescription,
        );
        assert_recipe_error(
            "pod app {\n    description \"bad\\ndesc\"\n}\n",
            RecipeErrorKind::InvalidDescription,
        );
        assert_recipe_error(
            "pod app {\n    name \"   \"\n}\n",
            RecipeErrorKind::MissingPodName,
        );
        assert_recipe_error(
            "pod app {\n    path ../escape\n}\n",
            RecipeErrorKind::InvalidPath,
        );
        assert_recipe_error(
            "pod app {\n    registry -flag\n}\n",
            RecipeErrorKind::InvalidRegistry,
        );
        assert_recipe_error(
            "pod app {\n    publish 1 2 3 4\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        );
        assert_recipe_error(
            "pod app {\n    publish -8080:80\n}\n",
            RecipeErrorKind::InvalidPublish,
        );
        assert_recipe_error(
            "pod app {\n    publish 8080 \"\"\n}\n",
            RecipeErrorKind::InvalidPublish,
        );
        assert_recipe_error(
            "pod app {\n    publish 8080 -80\n}\n",
            RecipeErrorKind::InvalidPublish,
        );
        assert_recipe_error(
            "pod app {\n    publish \"\" 80 TCP\n}\n",
            RecipeErrorKind::InvalidPublish,
        );
        assert_recipe_error(
            "pod app {\n    publish 8080 \"\" TCP\n}\n",
            RecipeErrorKind::InvalidPublish,
        );
        assert_recipe_error(
            "pod app {\n    publish 8080 80 \"\"\n}\n",
            RecipeErrorKind::InvalidPublish,
        );
        assert_recipe_error(
            "pod app {\n    option \"\"\n}\n",
            RecipeErrorKind::InvalidOption,
        );
        assert_recipe_error(
            "pod app {\n    option \"   \"\n}\n",
            RecipeErrorKind::InvalidOption,
        );
        assert_recipe_error(
            "pod app {\n    sub {\n    }\n}\n",
            RecipeErrorKind::UnexpectedBlock,
        );
        assert_recipe_error(
            "pod app {\n    command {\n        execute ls\n    }\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        );
        assert_recipe_error(
            "pod app {\n    command -bad {\n        execute ls\n    }\n}\n",
            RecipeErrorKind::InvalidCommand,
        );
        assert_recipe_error(
            "pod app {\n    command run {\n    }\n}\n",
            RecipeErrorKind::InvalidCommand,
        );
        assert_recipe_error(
            "pod app {\n    command run {\n        execute a\n    }\n    command run {\n        execute b\n    }\n}\n",
            RecipeErrorKind::DuplicateBlock,
        );
        assert_recipe_error(
            "pod app {\n    command run {\n        container a\n        container b\n    }\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "pod app {\n    command run {\n        description a\n        description b\n    }\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "pod app {\n    command run {\n        execute a\n        execute b\n    }\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "pod app {\n    command run {\n        user a\n        user b\n    }\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "pod app {\n    command run {\n        nested {\n        }\n    }\n}\n",
            RecipeErrorKind::UnexpectedBlock,
        );
        assert_recipe_error(
            "pod app {\n    command run {\n        unknown val\n    }\n}\n",
            RecipeErrorKind::UnknownDirective,
        );

        assert_recipe_error("container {\n}\n", RecipeErrorKind::EmptyContainerBlock);
        assert_recipe_error(
            "container \"\" {\n    image alpine\n}\n",
            RecipeErrorKind::InvalidContainerName,
        );
        assert_recipe_error(
            "container a b {\n    image alpine\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        );
        assert_recipe_error(
            "container *-bad {\n    image alpine\n}\n",
            RecipeErrorKind::InvalidContainerName,
        );
        assert_recipe_error(
            "container {\n    detach true\n    detach false\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "container {\n    image alpine\n    image redis\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "container {\n    registry a.io\n    registry b.io\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "container {\n    restart always\n    restart never\n}\n",
            RecipeErrorKind::DuplicateDirective,
        );
        assert_recipe_error(
            "container {\n    unknown val\n}\n",
            RecipeErrorKind::UnknownDirective,
        );
        assert_recipe_error(
            "container {\n    detach maybe\n}\n",
            RecipeErrorKind::InvalidBoolean,
        );
        assert_recipe_error(
            "container {\n    image -bad\n}\n",
            RecipeErrorKind::InvalidImage,
        );
        assert_recipe_error(
            "container {\n    restart -always\n}\n",
            RecipeErrorKind::InvalidRestartPolicy,
        );
        assert_recipe_error(
            "container {\n    command \"\"\n}\n",
            RecipeErrorKind::InvalidCommand,
        );
        assert_recipe_error(
            "container {\n    command \"   \"\n}\n",
            RecipeErrorKind::InvalidCommand,
        );
        assert_recipe_error(
            "container {\n    volume /a /b ro extra\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
        );
        assert_recipe_error(
            "container {\n    volume ../escape /data\n}\n",
            RecipeErrorKind::InvalidVolume,
        );
        assert_recipe_error(
            "container {\n    volume ./data /data:\n}\n",
            RecipeErrorKind::InvalidVolume,
        );
        assert_recipe_error(
            "container {\n    nested {\n    }\n}\n",
            RecipeErrorKind::UnexpectedBlock,
        );

        let delimiter_error = Recipe::from_str("pod \"app\"{ name Test }\n").unwrap_err();
        match delimiter_error {
            PodError::Psl(psl_error) => {
                assert_eq!(psl_error.kind, PslErrorKind::InvalidBlockHeader);
            }
            other => panic!("expected InvalidBlockHeader, got {other:?}"),
        }
    }
}
