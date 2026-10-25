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

use std::collections::{BTreeMap, BTreeSet};
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
use crate::utilities::path::{
    build_full_path, is_valid_directory, is_valid_directory_path, sanitize_path_token,
};
use crate::utilities::string::{
    contains_control_or_bidi, is_valid_identifier, is_valid_pod_or_container_name,
    is_valid_recipe_identifier, is_valid_text, is_valid_token, normalize_name, parse_ascii_u32,
};

/// File extension for PodScript Language recipe files.
pub const RECIPE_EXTENSION: &str = ".pod";
const SEEN_COMMAND_CONTAINER: u8 = 1 << 0;
const SEEN_COMMAND_DESCRIPTION: u8 = 1 << 1;
const SEEN_COMMAND_EXECUTE: u8 = 1 << 2;
const SEEN_COMMAND_USER: u8 = 1 << 3;
const SEEN_CONTAINER_IMAGE: u8 = 1 << 0;
const SEEN_CONTAINER_REGISTRY: u8 = 1 << 1;
const SEEN_CONTAINER_RESTART: u8 = 1 << 2;
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

    /// Evaluates whether `<recipes_dir>/<recipe_name>.pod` is a valid path that exists on disk.
    #[must_use]
    pub fn file_exists(recipes_dir: &Path, recipe_name: &str) -> bool {
        if !is_valid_load_target(recipes_dir, recipe_name) {
            return false;
        }
        fs::file_exists(&build_full_path(recipes_dir, recipe_name, RECIPE_EXTENSION))
    }

    /// Loads, parses, and semantically validates `<recipes.path>/<recipe_name>.pod` against `config`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] when the recipe path is invalid, the file is missing, PSL parsing fails,
    /// or required pod and container fields are missing or invalid.
    pub fn inspect(config: &Config, recipe_name: &str) -> Result<Self, PodError> {
        let (mut recipe, content, display_path) = load(&config.recipes.path, recipe_name)?;
        validate_recipe_inner(&mut recipe, config, recipe_name)
            .map_err(|error| PodError::from(error.with_diagnostic(&content, &display_path)))?;
        Ok(recipe)
    }

    /// Reads the raw `.pod` file at `path`, mapping a missing file to [`RecipeErrorKind::NotFound`].
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] with [`RecipeErrorKind::NotFound`] if `path` does not exist, or
    /// [`PodError::Io`] if reading the file fails.
    pub fn read_file(path: &Path) -> Result<String, PodError> {
        match fs::read_file(path) {
            Ok(content) => Ok(content),
            Err(PodError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Err(
                PodError::from(RecipeError::without_line(
                    RecipeErrorKind::NotFound,
                    sanitize_path_token(path),
                )),
            ),
            Err(error) => Err(error),
        }
    }

    /// Resolves a container reference (`1..=N`, `*suffix`, or name) to its matching [`Container`] in this recipe.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] with [`RecipeErrorKind::ContainerNotFound`] when no container matches.
    pub fn resolve_container(&self, container_spec: &str) -> Result<&Container, RecipeError> {
        resolve_container_entry(self, container_spec).map(|(container, _)| container)
    }

    /// Resolves a container reference (`1..=N`, `*suffix`, or name) against this recipe's containers.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] with [`RecipeErrorKind::ContainerNotFound`] when no container matches.
    pub fn resolve_container_name(&self, container_spec: &str) -> Result<String, RecipeError> {
        resolve_container_entry(self, container_spec).map(|(_, name)| name)
    }
}

impl FromStr for Recipe {
    type Err = PodError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        parse_recipe_with_path(input, "<input>")
    }
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
            command.container =
                parse_unique_text_directive(directive, seen, SEEN_COMMAND_CONTAINER, kind)?;
        }
        "description" => {
            command.description =
                parse_unique_text_directive(directive, seen, SEEN_COMMAND_DESCRIPTION, kind)?;
        }
        "execute" => {
            command.execute =
                parse_unique_text_directive(directive, seen, SEEN_COMMAND_EXECUTE, kind)?;
        }
        "user" => {
            command.user = Some(parse_unique_token_directive(
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
        "command" => parse_multi_text_directive(
            &mut container.commands,
            directive,
            RecipeErrorKind::InvalidCommand,
        )?,
        "image" => {
            let kind = RecipeErrorKind::InvalidImage;
            container.image =
                parse_unique_token_directive(directive, seen, SEEN_CONTAINER_IMAGE, kind)?;
        }
        "option" => parse_multi_text_directive(
            &mut container.options,
            directive,
            RecipeErrorKind::InvalidOption,
        )?,
        "registry" => {
            let kind = RecipeErrorKind::InvalidRegistry;
            container.registry =
                parse_unique_token_directive(directive, seen, SEEN_CONTAINER_REGISTRY, kind)?;
        }
        "restart" => {
            let kind = RecipeErrorKind::InvalidRestartPolicy;
            container.restart =
                parse_unique_token_directive(directive, seen, SEEN_CONTAINER_RESTART, kind)?;
        }
        "volume" => container.volumes.push(parse_volume_directive(directive)?),
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
            pod.description =
                parse_unique_text_directive(directive, seen, SEEN_POD_DESCRIPTION, kind)?;
        }
        "name" => pod.name = parse_pod_name_directive(directive, seen)?,
        "option" => {
            parse_multi_text_directive(&mut pod.options, directive, RecipeErrorKind::InvalidOption)?
        }
        "path" => pod.path = parse_pod_path_directive(directive, seen)?,
        "publish" => pod.publish.push(parse_publish_directive(directive)?),
        "registry" => {
            let kind = RecipeErrorKind::InvalidRegistry;
            pod.registry = parse_unique_token_directive(directive, seen, SEEN_POD_REGISTRY, kind)?;
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
    recipe.containers = Vec::with_capacity(document.blocks.len().saturating_sub(1));
    for block in &document.blocks {
        apply_root_block(&mut recipe, block)?;
    }
    Ok(recipe)
}

/// Searches `containers` in a single pass for an exact match on `exact` or a fallback match on `prefixed`.
#[must_use]
fn find_container_by_exact_or_prefixed<'a>(
    containers: &'a [Container],
    pod_id: &str,
    exact: &str,
    prefixed: &str,
) -> Option<(&'a Container, String)> {
    let mut fallback = None;
    for (index, container) in containers.iter().enumerate() {
        let Ok(resolved) =
            resolve_container_name_value(&container.name, pod_id, index + 1, container.line)
        else {
            continue;
        };
        if resolved == exact {
            return Some((container, resolved));
        }
        if fallback.is_none() && resolved == prefixed {
            fallback = Some((container, resolved));
        }
    }
    fallback
}

/// Validates and formats a 3-argument `publish` directive into `<first>:<second>/<third>`.
fn format_three_arg_publish(
    first: &str,
    second: &str,
    third: &str,
    directive: &PslDirective<'_>,
) -> Result<String, RecipeError> {
    for candidate in [first, second, third] {
        if !is_valid_token(candidate) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidPublish,
                directive.line,
                invalid_publish_token(candidate, directive.name),
            ));
        }
    }
    Ok(format!("{first}:{second}/{third}"))
}

/// Validates and formats a 2-argument `publish` directive into a single Podman `--publish` specification string.
fn format_two_arg_publish(
    first: &str,
    second: &str,
    directive: &PslDirective<'_>,
) -> Result<String, RecipeError> {
    if !(first.is_empty() || is_valid_token(first)) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPublish,
            directive.line,
            first,
        ));
    }
    if !is_valid_token(second) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPublish,
            directive.line,
            invalid_publish_token(second, directive.name),
        ));
    }
    if first.is_empty() {
        return Ok(second.to_owned());
    }
    if second.eq_ignore_ascii_case("TCP") || second.eq_ignore_ascii_case("UDP") {
        return Ok(format!("{first}/{second}"));
    }
    Ok(format!("{first}:{second}"))
}

/// Returns `candidate` when non-empty, or `fallback` when `candidate` is empty.
#[must_use]
const fn invalid_publish_token<'a>(candidate: &'a str, fallback: &'a str) -> &'a str {
    if candidate.is_empty() {
        fallback
    } else {
        candidate
    }
}

/// Identifies the offending argument token in an invalid [`VolumeMount`], falling back to `fallback`.
#[must_use]
fn invalid_volume_token<'a>(volume: &'a VolumeMount, fallback: &'a str) -> &'a str {
    if !volume.host_dir.is_empty() && !is_valid_directory_path(&volume.host_dir) {
        return &volume.host_dir;
    }
    if !volume.container_dir.starts_with('/') || !is_valid_directory_path(&volume.container_dir) {
        return invalid_publish_token(&volume.container_dir, fallback);
    }
    &volume.options
}

/// Evaluates whether `recipes_dir` and `recipe_name` form a valid recipe load target.
#[must_use]
fn is_valid_load_target(recipes_dir: &Path, recipe_name: &str) -> bool {
    let stem = recipe_name
        .strip_suffix(RECIPE_EXTENSION)
        .unwrap_or(recipe_name);
    is_valid_directory(recipes_dir) && is_valid_recipe_identifier(stem)
}

/// Loads and parses a `.pod` recipe file from `recipes_dir` and `recipe_name`, returning the parsed [`Recipe`], raw content, and sanitized display path.
fn load(recipes_dir: &Path, recipe_name: &str) -> Result<(Recipe, String, String), PodError> {
    validate_load_target(recipes_dir, recipe_name)?;
    let path = build_full_path(recipes_dir, recipe_name, RECIPE_EXTENSION);
    let content = Recipe::read_file(&path)?;
    let display_path = sanitize_path_token(&path);
    let recipe = parse_recipe_with_path(&content, &display_path)?;
    Ok((recipe, content, display_path))
}

/// Sets `flag` in `seen` for `directive` or returns [`RecipeErrorKind::DuplicateDirective`].
fn mark_seen(seen: &mut u8, flag: u8, directive: &PslDirective<'_>) -> Result<(), RecipeError> {
    if *seen & flag != 0 {
        return Err(RecipeError::with_token(
            RecipeErrorKind::DuplicateDirective,
            directive.line,
            directive.name,
        ));
    }
    *seen |= flag;
    Ok(())
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
    if !is_valid_identifier(command_name) || block.statements.is_empty() {
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
    let Some(raw_argument) = block.arguments.first() else {
        return Ok(String::new());
    };
    let raw_name = raw_argument.as_ref();
    if !is_valid_text(raw_name) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidContainerName,
            block.line,
            raw_name,
        ));
    }
    let normalized = normalize_name(raw_name);
    let candidate = normalized.strip_prefix('*').unwrap_or(&normalized);
    if !is_valid_pod_or_container_name(candidate) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidContainerName,
            block.line,
            raw_name,
        ));
    }
    Ok(normalized)
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
        if !is_valid_text(value) {
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
    let [argument] = block.arguments.as_slice() else {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            block.line,
            block.name,
        ));
    };
    let raw_id = argument.as_ref();
    if contains_control_or_bidi(raw_id) {
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

/// Marks `SEEN_POD_NAME` as seen and parses the `pod.name` display name directive.
fn parse_pod_name_directive(
    directive: &PslDirective<'_>,
    seen: &mut u8,
) -> Result<String, RecipeError> {
    mark_seen(seen, SEEN_POD_NAME, directive)?;
    let value = parse_single_argument(directive)?;
    if !is_valid_text(value) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::MissingPodName,
            directive.line,
            directive.name,
        ));
    }
    Ok(value.trim().to_owned())
}

/// Marks `SEEN_POD_PATH` as seen and parses the `pod.path` directory path directive.
fn parse_pod_path_directive(
    directive: &PslDirective<'_>,
    seen: &mut u8,
) -> Result<PathBuf, RecipeError> {
    mark_seen(seen, SEEN_POD_PATH, directive)?;
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
    match directive.arguments.as_slice() {
        [single] => {
            let single = single.as_ref();
            if !is_valid_token(single) {
                return Err(RecipeError::with_token(
                    RecipeErrorKind::InvalidPublish,
                    directive.line,
                    invalid_publish_token(single, directive.name),
                ));
            }
            Ok(single.to_owned())
        }
        [first, second] => format_two_arg_publish(first.as_ref(), second.as_ref(), directive),
        [first, second, third] => {
            format_three_arg_publish(first.as_ref(), second.as_ref(), third.as_ref(), directive)
        }
        _ => Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            directive.line,
            directive.name,
        )),
    }
}

/// Parses and validates a recipe string, attaching a compiler-style diagnostic on failure.
fn parse_recipe_with_path(input: &str, file_path: &str) -> Result<Recipe, PodError> {
    let document = parse_psl(input)
        .map_err(|error| PodError::from(error.with_diagnostic(input, file_path)))?;
    let recipe = build_recipe(&document)
        .map_err(|error| PodError::from(error.with_diagnostic(input, file_path)))?;
    Ok(recipe)
}

/// Extracts the sole argument of a directive or returns [`RecipeErrorKind::InvalidArgumentCount`].
fn parse_single_argument<'a>(directive: &'a PslDirective<'a>) -> Result<&'a str, RecipeError> {
    let [argument] = directive.arguments.as_slice() else {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidArgumentCount,
            directive.line,
            directive.name,
        ));
    };
    Ok(argument.as_ref())
}

/// Parses a single-argument text directive, rejecting empty strings and control/BiDi characters.
fn parse_text_directive(
    directive: &PslDirective<'_>,
    invalid_kind: RecipeErrorKind,
) -> Result<String, RecipeError> {
    let value = parse_single_argument(directive)?;
    if !is_valid_text(value) {
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
    if !is_valid_token(value) {
        return Err(RecipeError::with_token(invalid_kind, directive.line, value));
    }
    Ok(value.to_owned())
}

/// Marks `flag` as seen and parses a single-argument text directive.
fn parse_unique_text_directive(
    directive: &PslDirective<'_>,
    seen: &mut u8,
    flag: u8,
    invalid_kind: RecipeErrorKind,
) -> Result<String, RecipeError> {
    mark_seen(seen, flag, directive)?;
    parse_text_directive(directive, invalid_kind)
}

/// Marks `flag` as seen and parses a single-argument token directive.
fn parse_unique_token_directive(
    directive: &PslDirective<'_>,
    seen: &mut u8,
    flag: u8,
    invalid_kind: RecipeErrorKind,
) -> Result<String, RecipeError> {
    mark_seen(seen, flag, directive)?;
    parse_token_directive(directive, invalid_kind)
}

/// Parses and validates a `volume` directive (`2..=3` arguments) into a [`VolumeMount`].
fn parse_volume_directive(directive: &PslDirective<'_>) -> Result<VolumeMount, RecipeError> {
    let volume = match directive.arguments.as_slice() {
        [host_dir, container_dir] => VolumeMount {
            container_dir: container_dir.as_ref().to_owned(),
            host_dir: host_dir.as_ref().to_owned(),
            options: String::new(),
        },
        [host_dir, container_dir, options] => VolumeMount {
            container_dir: container_dir.as_ref().to_owned(),
            host_dir: host_dir.as_ref().to_owned(),
            options: options.as_ref().to_owned(),
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
        let token = invalid_volume_token(&volume, directive.name);
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidVolume,
            directive.line,
            token,
        ));
    }
    Ok(volume)
}

/// Creates a [`RecipeError`] at `line` when non-zero, or without a line number when `line == 0`.
#[must_use]
fn pod_error_at_line(line: usize, kind: RecipeErrorKind, token: impl AsRef<str>) -> RecipeError {
    if line == 0 {
        RecipeError::without_line(kind, token)
    } else {
        RecipeError::with_token(kind, line, token)
    }
}

/// Resolves a 1-based numeric container index (`1..=N`) against `recipe.containers`.
fn resolve_by_index<'a>(
    recipe: &'a Recipe,
    index: usize,
    raw_spec: &str,
) -> Result<(&'a Container, String), RecipeError> {
    let Some(container) = index
        .checked_sub(1)
        .and_then(|zero_based| recipe.containers.get(zero_based))
    else {
        return Err(pod_error_at_line(
            recipe.pod.line,
            RecipeErrorKind::ContainerNotFound,
            raw_spec,
        ));
    };
    let resolved =
        resolve_container_name_value(&container.name, &recipe.pod.id, index, container.line)?;
    Ok((container, resolved))
}

/// Resolves `container_spec` (`1..=N`, `*suffix`, or name) against `recipe.containers`, returning the matched [`Container`] and its resolved name.
fn resolve_container_entry<'a>(
    recipe: &'a Recipe,
    container_spec: &str,
) -> Result<(&'a Container, String), RecipeError> {
    if !recipe.has_pod
        || !is_valid_pod_or_container_name(&recipe.pod.id)
        || !is_valid_text(container_spec)
    {
        return Err(pod_error_at_line(
            recipe.pod.line,
            RecipeErrorKind::ContainerNotFound,
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
        if let Some(found) = find_container_by_exact_or_prefixed(
            &recipe.containers,
            &recipe.pod.id,
            &target,
            &target,
        ) {
            return Ok(found);
        }
        return Err(pod_error_at_line(
            recipe.pod.line,
            RecipeErrorKind::ContainerNotFound,
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
    Err(pod_error_at_line(
        recipe.pod.line,
        RecipeErrorKind::ContainerNotFound,
        container_spec,
    ))
}

/// Validates `pod.path` or populates it from `<config.pods.path>/<pod.id>` when omitted.
fn resolve_pod_path(pod: &mut Pod, config: &Config, file_name: &str) -> Result<(), RecipeError> {
    let used_fallback = pod.path.as_os_str().is_empty();
    if used_fallback {
        if config.pods.path.as_os_str().is_empty() {
            return Err(pod_error_at_line(
                pod.line,
                RecipeErrorKind::MissingPodPath,
                file_name,
            ));
        }
        if !is_valid_directory(&config.pods.path) {
            return Err(pod_error_at_line(
                pod.line,
                RecipeErrorKind::InvalidPath,
                sanitize_path_token(&config.pods.path),
            ));
        }
        pod.path = build_full_path(&config.pods.path, &pod.id, "");
    }
    if !is_valid_directory(&pod.path) {
        return Err(pod_error_at_line(
            pod.line,
            RecipeErrorKind::InvalidPath,
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

/// Resolves container names, validates each container, and rejects duplicate container names.
fn validate_and_resolve_containers(
    containers: &mut [Container],
    pod_id: &str,
) -> Result<(), RecipeError> {
    let mut seen_names = BTreeSet::new();
    for (index, container) in containers.iter_mut().enumerate() {
        container.ensure_name(pod_id, index + 1)?;
        container.validate()?;
        if !seen_names.insert(container.name.as_str()) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidContainerName,
                container.line,
                &container.name,
            ));
        }
    }
    Ok(())
}

/// Validates that `recipes_dir` and `recipe_name` are safe before constructing the `.pod` file path.
fn validate_load_target(recipes_dir: &Path, recipe_name: &str) -> Result<(), PodError> {
    if !is_valid_load_target(recipes_dir, recipe_name) {
        let invalid_path = build_full_path(recipes_dir, recipe_name, RECIPE_EXTENSION);
        return Err(PodError::from(RecipeError::without_line(
            RecipeErrorKind::InvalidPath,
            sanitize_path_token(&invalid_path),
        )));
    }
    Ok(())
}

/// Validates a single [`PodCommand`] entry inside `pod.commands`.
fn validate_pod_command(
    key: &str,
    command: &PodCommand,
    line: usize,
) -> Result<(), RecipeError> {
    if !is_valid_identifier(key) || key != command.name {
        return Err(pod_error_at_line(
            line,
            RecipeErrorKind::InvalidCommand,
            key,
        ));
    }
    if command.container.is_empty()
        && command.description.is_empty()
        && command.execute.is_empty()
        && command.user.is_none()
    {
        return Err(pod_error_at_line(
            line,
            RecipeErrorKind::InvalidCommand,
            key,
        ));
    }
    if !command.container.is_empty() && !is_valid_text(&command.container) {
        return Err(pod_error_at_line(
            line,
            RecipeErrorKind::InvalidCommand,
            &command.container,
        ));
    }
    if !command.description.is_empty() && !is_valid_text(&command.description) {
        return Err(pod_error_at_line(
            line,
            RecipeErrorKind::InvalidCommand,
            &command.description,
        ));
    }
    if !command.execute.is_empty() && !is_valid_text(&command.execute) {
        return Err(pod_error_at_line(
            line,
            RecipeErrorKind::InvalidCommand,
            &command.execute,
        ));
    }
    if let Some(user) = &command.user
        && !is_valid_token(user)
    {
        return Err(pod_error_at_line(
            line,
            RecipeErrorKind::InvalidCommand,
            user,
        ));
    }
    Ok(())
}

/// Validates all [`PodCommand`] entries in `pod.commands`.
fn validate_pod_commands(
    commands: &BTreeMap<String, PodCommand>,
    line: usize,
) -> Result<(), RecipeError> {
    for (key, command) in commands {
        validate_pod_command(key, command, line)?;
    }
    Ok(())
}

/// Validates pod fields (`id`, `name`, `description`, `registry`, `commands`, `publish`, `options`, and `path`) during [`validate_recipe_inner`].
fn validate_pod_fields(
    pod: &mut Pod,
    config: &Config,
    file_name: &str,
) -> Result<(), RecipeError> {
    if contains_control_or_bidi(&pod.id) {
        return Err(pod_error_at_line(
            pod.line,
            RecipeErrorKind::InvalidPodId,
            &pod.id,
        ));
    }
    let normalized_id = normalize_name(&pod.id);
    if !is_valid_pod_or_container_name(&normalized_id) {
        return Err(pod_error_at_line(
            pod.line,
            RecipeErrorKind::InvalidPodId,
            &pod.id,
        ));
    }
    pod.id = normalized_id;

    if !is_valid_text(&pod.name) {
        return Err(pod_error_at_line(
            pod.line,
            RecipeErrorKind::MissingPodName,
            file_name,
        ));
    }
    let trimmed_name = pod.name.trim();
    if pod.name.len() != trimmed_name.len() {
        pod.name = trimmed_name.to_owned();
    }

    if !pod.description.is_empty() && !is_valid_text(&pod.description) {
        return Err(pod_error_at_line(
            pod.line,
            RecipeErrorKind::InvalidDescription,
            &pod.description,
        ));
    }

    if pod.registry.is_empty() {
        return Err(pod_error_at_line(
            pod.line,
            RecipeErrorKind::MissingPodRegistry,
            file_name,
        ));
    }
    if !is_valid_token(&pod.registry) {
        return Err(pod_error_at_line(
            pod.line,
            RecipeErrorKind::InvalidRegistry,
            &pod.registry,
        ));
    }

    validate_pod_commands(&pod.commands, pod.line)?;
    validate_pod_publish(&pod.publish, pod.line)?;
    validate_pod_options(&pod.options, pod.line)?;
    resolve_pod_path(pod, config, file_name)
}

/// Validates that every `pod.options` entry is a safe text argument.
fn validate_pod_options(options: &[String], line: usize) -> Result<(), RecipeError> {
    for option in options {
        if !is_valid_text(option) {
            return Err(pod_error_at_line(
                line,
                RecipeErrorKind::InvalidOption,
                option,
            ));
        }
    }
    Ok(())
}

/// Validates that every `pod.publish` entry is a safe token argument.
fn validate_pod_publish(publish: &[String], line: usize) -> Result<(), RecipeError> {
    for entry in publish {
        if !is_valid_token(entry) {
            return Err(pod_error_at_line(
                line,
                RecipeErrorKind::InvalidPublish,
                entry,
            ));
        }
    }
    Ok(())
}

/// Validates required pod and container fields, applies default pod paths, and resolves container names.
fn validate_recipe_inner(
    recipe: &mut Recipe,
    config: &Config,
    file_name: &str,
) -> Result<(), RecipeError> {
    if !recipe.has_pod || recipe.pod.id.is_empty() {
        return Err(pod_error_at_line(
            recipe.pod.line,
            RecipeErrorKind::MissingPodBlock,
            file_name,
        ));
    }
    validate_pod_fields(&mut recipe.pod, config, file_name)?;
    if recipe.containers.is_empty() {
        return Err(pod_error_at_line(
            recipe.pod.line,
            RecipeErrorKind::MissingContainersBlock,
            file_name,
        ));
    }
    validate_and_resolve_containers(&mut recipe.containers, &recipe.pod.id)
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
    use crate::core::log;
    use crate::core::pod::PodCommand;
    use crate::error::{PodError, PslErrorKind, RecipeErrorKind};
    use crate::io::system::fs as system_fs;
    use super::{Recipe, load, validate_recipe_inner};

    fn assert_recipe_error(input: &str, expected_kind: RecipeErrorKind) {
        let error = Recipe::from_str(input).unwrap_err();
        match error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, expected_kind);
                let diagnostic = recipe_error.diagnostic.as_ref().unwrap();
                assert_eq!(diagnostic.file_path, "<input>");
            }
            other => panic!("expected PodError::Recipe({expected_kind:?}), got {other:?}"),
        }
    }

    fn assert_validate_error(
        recipe: &mut Recipe,
        config: &Config,
        file_name: &str,
        expected_kind: RecipeErrorKind,
    ) {
        let error = validate_recipe_inner(recipe, config, file_name).unwrap_err();
        assert_eq!(error.kind, expected_kind);
    }

    #[test]
    fn load_root_recipe_pod_and_temp_recipes() {
        let config = Config::load(Path::new("./pods.conf")).unwrap();
        let root_recipe = Recipe::inspect(&config, "recipe").unwrap();

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

        assert_eq!(root_recipe.pod.commands.len(), 3);
        let backup_command = root_recipe.pod.commands.get("backup").unwrap();
        assert_eq!(backup_command.name, "backup");
        assert_eq!(backup_command.description, "Create a database backup.");
        assert_eq!(backup_command.container, "*db");
        assert_eq!(backup_command.user.as_deref(), Some("dbuser"));
        assert_eq!(
            backup_command.execute,
            "db-dump -U dbuser -d app -f /var/lib/db/data/backup.sql"
        );

        let clear_cache_command = root_recipe.pod.commands.get("clear_cache").unwrap();
        assert_eq!(clear_cache_command.name, "clear_cache");
        assert_eq!(
            clear_cache_command.description,
            "Clear the web application cache."
        );
        assert_eq!(clear_cache_command.container, "*app");
        assert_eq!(clear_cache_command.user, None);
        assert_eq!(clear_cache_command.execute, "app-cli cache:clear");

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
        assert!(matches!(db_container.restart.as_str(), "always" | "never"));
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
        assert!(matches!(app_container.restart.as_str(), "always" | "never"));
        assert_eq!(app_container.image, "example/example-app:latest");
        assert_eq!(
            app_container.commands,
            vec![String::from("start-server.sh")]
        );
        assert_eq!(
            root_recipe.resolve_container("*app").unwrap().name,
            "example-stack-app"
        );
        assert_eq!(
            root_recipe.resolve_container_name("*app").unwrap(),
            "example-stack-app"
        );
        assert_eq!(
            root_recipe.resolve_container("1").unwrap().name,
            "example-stack-db"
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
        assert!(!Recipe::file_exists(&temp_dir, "missing_recipe"));
        let missing_path = temp_dir.join("missing_recipe.pod");
        assert!(matches!(
            Recipe::read_file(&missing_path),
            Err(PodError::Recipe(error)) if error.kind == RecipeErrorKind::NotFound
        ));
        assert!(matches!(
            Recipe::read_file(&temp_dir),
            Err(PodError::Io(_))
        ));

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
            assert!(!Recipe::file_exists(&temp_dir, invalid_name));
            let invalid_target_error = load(&temp_dir, invalid_name).unwrap_err();
            match invalid_target_error {
                PodError::Recipe(error) => assert_eq!(error.kind, RecipeErrorKind::InvalidPath),
                other => panic!("expected InvalidPath for '{invalid_name}', got {other:?}"),
            }
        }

        for invalid_dir in [Path::new("../escape"), Path::new("/bad:dir")] {
            assert!(!Recipe::file_exists(invalid_dir, "webapp"));
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
    restart never
    image alpine:latest
    volume \"\" /container/dir/anonymous \"\"
    volume named_volume /container/dir/named ro
    volume /absolute/path /container/dir/absolute
    volume ./relative/path /container/dir/relative z
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
        assert!(Recipe::file_exists(&temp_dir, "webapp"));
        assert!(Recipe::file_exists(&temp_dir, "webapp.pod"));
        assert_eq!(Recipe::read_file(&valid_path).unwrap(), webapp_content);

        let (mut loaded, _, _) = load(&temp_dir, "webapp.pod").unwrap();
        let mut custom_config = Config::new();
        custom_config.pods.path = PathBuf::from("/pods");
        validate_recipe_inner(&mut loaded, &custom_config, "webapp").unwrap();

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
        assert_eq!(loaded.containers[0].volumes.len(), 4);
        assert_eq!(loaded.containers[0].options, vec!["--env", "FOO=bar"]);
        assert_eq!(loaded.containers[0].commands, vec!["O'Hara", "Hancock 2"]);
        assert_eq!(loaded.containers[1].name, "si_po-db");
        assert_eq!(loaded.containers[1].registry, "db.io");
        assert_eq!(loaded.containers[2].name, "si_po-3");

        assert_eq!(loaded.resolve_container_name("1").unwrap(), "supr_app");
        assert_eq!(loaded.resolve_container_name("2").unwrap(), "si_po-db");
        assert_eq!(loaded.resolve_container_name("3").unwrap(), "si_po-3");
        assert_eq!(
            loaded.resolve_container_name("SuPr ApP").unwrap(),
            "supr_app"
        );
        assert_eq!(loaded.resolve_container_name("*db").unwrap(), "si_po-db");
        assert_eq!(loaded.resolve_container_name("db").unwrap(), "si_po-db");
        assert_eq!(loaded.resolve_container_name("3").unwrap(), "si_po-3");

        for unknown in ["", "   ", "0", "+1", "4", "*missing", "missing", "bad\nname"] {
            let error = loaded.resolve_container_name(unknown).unwrap_err();
            assert_eq!(error.kind, RecipeErrorKind::ContainerNotFound);
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parse_and_validate_multi_container_service_recipe_abstraction() {
        let input = "\
pod stack {
    name     \"Service Stack\"
    path     /srv/storage/stack
    registry registry.io
    publish  18080 80 TCP
    option   \"--network slirp4netns:port_handler=slirp4netns\"
    option   \"--sysctl net.ipv4.ip_unprivileged_port_start=80\"
    option   --userns=keep-id:uid=1000,gid=1000

    command sync_indices {
        description \"Synchronize database indices.\"
        container   *web
        user        1000
        execute     \"/srv/www/cli db:sync-indices\"
    }

    command repair_state {
        description \"Repair application state.\"
        container   *web
        user        1000
        execute     \"/srv/www/cli system:repair --include-slow\"
    }

    command scan_storage {
        description \"Scan storage files.\"
        container   *web
        user        1000
        execute     \"/srv/www/cli files:scan --all\"
    }
}

container *store {
    restart never
    image   kv-store:7-alpine
    volume  store /data Z
}

container *web {
    restart never
    image   web-service:20
    volume  web                 /srv/www        Z
    volume  config              /srv/www/config Z
    volume  files               /srv/www/files  Z
    volume  /srv/storage/shared /shared         z
    option  \"--user 1000:1000\"
}
";
        let mut recipe = Recipe::from_str(input).unwrap();
        let config = Config::new();
        validate_recipe_inner(&mut recipe, &config, "stack").unwrap();

        assert_eq!(recipe.pod.id, "stack");
        assert_eq!(recipe.pod.name, "Service Stack");
        assert!(recipe.pod.description.is_empty());
        assert_eq!(recipe.pod.path, PathBuf::from("/srv/storage/stack"));
        assert_eq!(recipe.pod.registry, "registry.io");
        assert_eq!(recipe.pod.publish, vec!["18080:80/TCP"]);
        assert_eq!(
            recipe.pod.options,
            vec![
                "--network slirp4netns:port_handler=slirp4netns",
                "--sysctl net.ipv4.ip_unprivileged_port_start=80",
                "--userns=keep-id:uid=1000,gid=1000",
            ]
        );
        assert_eq!(
            recipe.pod.build_create_args(),
            vec![
                "pod",
                "create",
                "--name",
                "stack",
                "--publish",
                "18080:80/TCP",
                "--network",
                "slirp4netns:port_handler=slirp4netns",
                "--sysctl",
                "net.ipv4.ip_unprivileged_port_start=80",
                "--userns=keep-id:uid=1000,gid=1000",
            ]
        );

        assert_eq!(recipe.pod.commands.len(), 3);
        let sync_command = recipe.pod.commands.get("sync_indices").unwrap();
        assert_eq!(sync_command.container, "*web");
        assert_eq!(sync_command.user.as_deref(), Some("1000"));
        assert_eq!(sync_command.execute, "/srv/www/cli db:sync-indices");
        assert_eq!(
            recipe.resolve_container_name(&sync_command.container).unwrap(),
            "stack-web"
        );

        assert_eq!(recipe.containers.len(), 2);
        assert_eq!(
            recipe.containers[0].build_create_args(&recipe.pod).unwrap(),
            vec![
                "create",
                "--name",
                "stack-store",
                "--pod",
                "stack",
                "--restart",
                "never",
                "--volume",
                "/srv/storage/stack/store:/data:Z",
                "registry.io/kv-store:7-alpine",
            ]
        );
        assert_eq!(
            recipe.containers[1].build_create_args(&recipe.pod).unwrap(),
            vec![
                "create",
                "--name",
                "stack-web",
                "--pod",
                "stack",
                "--restart",
                "never",
                "--volume",
                "/srv/storage/stack/web:/srv/www:Z",
                "--volume",
                "/srv/storage/stack/config:/srv/www/config:Z",
                "--volume",
                "/srv/storage/stack/files:/srv/www/files:Z",
                "--volume",
                "/srv/storage/shared:/shared:z",
                "--user",
                "1000:1000",
                "registry.io/web-service:20",
            ]
        );

        let nginx_input = "\
pod nginx {
    name     \"Nginx\"
    path     /pods/nginx
    registry docker.io
    publish  443 443 TCP
    publish  443 443 UDP
    # Use \"--network pasta\" on Podman >= 5.0
    option   \"--network slirp4netns:port_handler=slirp4netns\"
}

container *app {
    restart never
    image   nginx:stable-alpine
    volume  nginx.conf /etc/nginx/nginx.conf ro,Z
    volume  certs      /etc/nginx/certs      ro,Z
    volume  bots       /etc/nginx/bots.d     ro,Z
    volume  config     /etc/nginx/conf.d     ro,Z
    volume  log        /var/log/nginx        Z
}
";
        let mut nginx_recipe = Recipe::from_str(nginx_input).unwrap();
        validate_recipe_inner(&mut nginx_recipe, &config, "nginx").unwrap();
        assert_eq!(
            nginx_recipe.pod.build_create_args(),
            vec![
                "pod",
                "create",
                "--name",
                "nginx",
                "--publish",
                "443:443/TCP",
                "--publish",
                "443:443/UDP",
                "--network",
                "slirp4netns:port_handler=slirp4netns",
            ]
        );
        assert_eq!(
            nginx_recipe.containers[0]
                .build_create_args(&nginx_recipe.pod)
                .unwrap(),
            vec![
                "create",
                "--name",
                "nginx-app",
                "--pod",
                "nginx",
                "--restart",
                "never",
                "--volume",
                "/pods/nginx/nginx.conf:/etc/nginx/nginx.conf:ro,Z",
                "--volume",
                "/pods/nginx/certs:/etc/nginx/certs:ro,Z",
                "--volume",
                "/pods/nginx/bots:/etc/nginx/bots.d:ro,Z",
                "--volume",
                "/pods/nginx/config:/etc/nginx/conf.d:ro,Z",
                "--volume",
                "/pods/nginx/log:/var/log/nginx:Z",
                "docker.io/nginx:stable-alpine",
            ]
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
            "pod app {\n    name \"\\nApp\\n\"\n}\n",
            RecipeErrorKind::MissingPodName,
        );
        assert_recipe_error(
            "pod app {\n    name \"\\tApp\\t\"\n}\n",
            RecipeErrorKind::MissingPodName,
        );
        assert_recipe_error(
            "pod app {\n    name \"\u{0085}App\u{0085}\"\n}\n",
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
            "pod app {\n    publish \"\"\n}\n",
            RecipeErrorKind::InvalidPublish,
        );
        assert_recipe_error(
            "pod app {\n    publish -8080:80\n}\n",
            RecipeErrorKind::InvalidPublish,
        );
        assert_recipe_error(
            "pod app {\n    publish -8080 80\n}\n",
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
            "container {\n    volume /only_one\n}\n",
            RecipeErrorKind::InvalidArgumentCount,
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
            "container {\n    volume data /var/lib/db/data:Z\n}\n",
            RecipeErrorKind::InvalidVolume,
        );
        assert_recipe_error(
            "container {\n    volume ./data /data:\n}\n",
            RecipeErrorKind::InvalidVolume,
        );
        assert_recipe_error(
            "container {\n    volume data /data -bad\n}\n",
            RecipeErrorKind::InvalidVolume,
        );
        assert_recipe_error(
            "container {\n    nested {\n    }\n}\n",
            RecipeErrorKind::UnexpectedBlock,
        );

        let empty_container_dir_error =
            Recipe::from_str("container {\n    volume data \"\"\n}\n").unwrap_err();
        match empty_container_dir_error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidVolume);
                assert_eq!(recipe_error.token.as_deref(), Some("volume"));
            }
            other => panic!("expected InvalidVolume, got {other:?}"),
        }

        let volume_diag_error = Recipe::from_str(
            "container {\n    volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'\n}\n",
        )
        .unwrap_err();
        match volume_diag_error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::InvalidVolume);
                assert_eq!(recipe_error.token.as_deref(), Some("/var/lib/db/data:Z"));
                let diagnostic = recipe_error.diagnostic.as_ref().unwrap();
                assert_eq!(diagnostic.line_number, 2);
                assert_eq!(diagnostic.col_number, 19);
                assert_eq!(
                    diagnostic.annotation_carets,
                    "                  ^^^^^^^^^^^^^^^^^^"
                );
            }
            other => panic!("expected InvalidVolume, got {other:?}"),
        }

        let delimiter_error = Recipe::from_str("pod \"app\"{ name Test }\n").unwrap_err();
        match delimiter_error {
            PodError::Psl(psl_error) => {
                assert_eq!(psl_error.kind, PslErrorKind::InvalidBlockHeader);
                let diagnostic = psl_error.diagnostic.as_ref().unwrap();
                assert_eq!(diagnostic.file_path, "<input>");
                assert_eq!(diagnostic.col_number, 10);
                assert_eq!(diagnostic.annotation_carets, "         ^");
            }
            other => panic!("expected InvalidBlockHeader, got {other:?}"),
        }
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
        assert_validate_error(
            &mut no_pod,
            &config_with_pod_path,
            "../unsafe_name",
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
        for control_name in ["\nApp\n", "\tApp\t", "\u{0085}App\u{0085}"] {
            no_name.pod.name = String::from(control_name);
            assert_validate_error(
                &mut no_name,
                &config_with_pod_path,
                "control_pod_name",
                RecipeErrorKind::MissingPodName,
            );
        }
        no_name.pod.name = String::from("  Trimmed Name  ");
        validate_recipe_inner(&mut no_name, &config_with_pod_path, "trimmed_pod_name").unwrap();
        assert_eq!(no_name.pod.name, "Trimmed Name");

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
        for invalid_fallback in ["/", "-flag", "   ", "../escape"] {
            let mut config_invalid_fallback = Config::new();
            config_invalid_fallback.pods.path = PathBuf::from(invalid_fallback);
            let mut fallback_recipe = no_path.clone();
            assert_validate_error(
                &mut fallback_recipe,
                &config_invalid_fallback,
                "invalid_fallback_pods_path",
                RecipeErrorKind::InvalidPath,
            );
        }

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

        let unvalidated_no_pod =
            Recipe::from_str("container *f {\n    image alpine:latest\n}\n").unwrap();
        let resolve_error = unvalidated_no_pod.resolve_container_name("1").unwrap_err();
        assert_eq!(resolve_error.kind, RecipeErrorKind::ContainerNotFound);
        assert_eq!(resolve_error.line, None);
        assert!(!resolve_error.to_string().contains("line 0:"));

        let mut invalid_pod_recipe = unvalidated_no_pod.clone();
        invalid_pod_recipe.has_pod = true;
        invalid_pod_recipe.pod.id = String::from("-flag");
        invalid_pod_recipe.pod.line = 3;
        let invalid_pod_resolve = invalid_pod_recipe.resolve_container_name("*f").unwrap_err();
        assert_eq!(invalid_pod_resolve.kind, RecipeErrorKind::ContainerNotFound);
        assert_eq!(invalid_pod_resolve.line, Some(3));

        let missing_pod_error =
            validate_recipe_inner(&mut no_pod, &config_with_pod_path, "missing_pod").unwrap_err();
        assert_eq!(missing_pod_error.kind, RecipeErrorKind::MissingPodBlock);
        assert_eq!(missing_pod_error.line, None);
        assert!(!missing_pod_error.to_string().contains("line 0:"));

        let mut control_pod_id = Recipe::from_str(
            "pod test {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        control_pod_id.pod.id = String::from("bad\x07id");
        assert_validate_error(
            &mut control_pod_id,
            &config_with_pod_path,
            "control_pod_id",
            RecipeErrorKind::InvalidPodId,
        );

        let mut invalid_pod_id = Recipe::from_str(
            "pod test {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        invalid_pod_id.pod.id = String::from("-invalid");
        assert_validate_error(
            &mut invalid_pod_id,
            &config_with_pod_path,
            "invalid_pod_id",
            RecipeErrorKind::InvalidPodId,
        );

        let mut invalid_registry = Recipe::from_str(
            "pod test {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        invalid_registry.pod.registry = String::from("-bad-registry");
        assert_validate_error(
            &mut invalid_registry,
            &config_with_pod_path,
            "invalid_registry",
            RecipeErrorKind::InvalidRegistry,
        );

        let mut invalid_pod_path = Recipe::from_str(
            "pod test {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        invalid_pod_path.pod.path = PathBuf::from("../escape");
        assert_validate_error(
            &mut invalid_pod_path,
            &config_with_pod_path,
            "invalid_pod_path",
            RecipeErrorKind::InvalidPath,
        );

        let base_valid = Recipe::from_str(
            "pod test {\n    name Test\n    registry docker.io\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();

        let mut mismatched_command_key = base_valid.clone();
        let mut valid_command = PodCommand::new();
        valid_command.name = String::from("migrate");
        valid_command.execute = String::from("sh migrate.sh");
        mismatched_command_key
            .pod
            .commands
            .insert(String::from("other"), valid_command.clone());
        assert_validate_error(
            &mut mismatched_command_key,
            &config_with_pod_path,
            "mismatched_command_key",
            RecipeErrorKind::InvalidCommand,
        );

        let mut empty_command = base_valid.clone();
        let mut blank_command = PodCommand::new();
        blank_command.name = String::from("migrate");
        empty_command
            .pod
            .commands
            .insert(String::from("migrate"), blank_command);
        assert_validate_error(
            &mut empty_command,
            &config_with_pod_path,
            "empty_command",
            RecipeErrorKind::InvalidCommand,
        );

        let mut invalid_command_container = base_valid.clone();
        let mut bad_container_command = valid_command.clone();
        bad_container_command.container = String::from("   ");
        invalid_command_container
            .pod
            .commands
            .insert(String::from("migrate"), bad_container_command);
        assert_validate_error(
            &mut invalid_command_container,
            &config_with_pod_path,
            "invalid_command_container",
            RecipeErrorKind::InvalidCommand,
        );

        let mut invalid_command_description = base_valid.clone();
        let mut bad_description_command = valid_command.clone();
        bad_description_command.description = String::from("bad\x07desc");
        invalid_command_description
            .pod
            .commands
            .insert(String::from("migrate"), bad_description_command);
        assert_validate_error(
            &mut invalid_command_description,
            &config_with_pod_path,
            "invalid_command_description",
            RecipeErrorKind::InvalidCommand,
        );

        let mut invalid_command_execute = base_valid.clone();
        let mut bad_execute_command = valid_command.clone();
        bad_execute_command.execute = String::from("   ");
        invalid_command_execute
            .pod
            .commands
            .insert(String::from("migrate"), bad_execute_command);
        assert_validate_error(
            &mut invalid_command_execute,
            &config_with_pod_path,
            "invalid_command_execute",
            RecipeErrorKind::InvalidCommand,
        );

        let mut invalid_command_user = base_valid.clone();
        let mut bad_user_command = valid_command;
        bad_user_command.user = Some(String::from("-root"));
        invalid_command_user
            .pod
            .commands
            .insert(String::from("migrate"), bad_user_command);
        assert_validate_error(
            &mut invalid_command_user,
            &config_with_pod_path,
            "invalid_command_user",
            RecipeErrorKind::InvalidCommand,
        );

        let mut debug_fallback_recipe = base_valid;
        log::set_debug_enabled(true);
        let debug_result = validate_recipe_inner(
            &mut debug_fallback_recipe,
            &config_with_pod_path,
            "debug_fallback",
        );
        log::set_debug_enabled(false);
        debug_result.unwrap();
        assert_eq!(debug_fallback_recipe.pod.path, PathBuf::from("/pods/test"));

        let temp_dir = env::temp_dir().join(format!("podoxide_validate_diag_{}", process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        let broken_path = temp_dir.join("broken.pod");
        system_fs::write_file(
            &broken_path,
            "pod broken {\n    name Broken\n}\ncontainer {\n    image alpine:latest\n}\n",
        )
        .unwrap();
        let mut config_temp = Config::new();
        config_temp.pods.path = PathBuf::from("/pods");
        config_temp.recipes.path = temp_dir.clone();
        let broken_error = Recipe::inspect(&config_temp, "broken").unwrap_err();
        match broken_error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::MissingPodRegistry);
                assert!(recipe_error.diagnostic.is_some());
            }
            other => panic!("expected MissingPodRegistry with diagnostic, got {other:?}"),
        }
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
