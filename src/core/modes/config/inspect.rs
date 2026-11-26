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

//! Handler for `pods config` (`pods config inspect`).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use crate::core::config::Config;
use crate::core::context::Context;
use crate::core::modes::targets::{self, RecipeStatus};
use crate::core::recipe::RECIPE_EXTENSION;
use crate::error::PodError;
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use super::render;

/// Upper bound on rendered lines in the `Groups:` tree output.
pub(super) const MAX_GROUP_OUTPUT_LINES: usize = 4096;

/// Entry in the rendered group hierarchy output queue.
pub(super) enum GroupOutputLine {
    Status { line: String, status: &'static str },
    Text(String),
}

/// Mutable state accumulated while traversing configured recipe groups for display and validation.
#[derive(Default)]
pub(super) struct GroupValidationState<'a> {
    pub(super) invalid_recipes: BTreeSet<&'a str>,
    pub(super) max_line_length: usize,
    pub(super) missing_recipes: BTreeSet<&'a str>,
    pub(super) output_lines: Vec<GroupOutputLine>,
    pub(super) referenced_recipes: BTreeSet<&'a str>,
    pub(super) truncated: bool,
    validation_cache: BTreeMap<&'a str, RecipeStatus>,
}

impl GroupValidationState<'_> {
    /// Creates a new empty [`GroupValidationState`].
    #[must_use]
    fn new() -> Self {
        Self::default()
    }
}

/// Displays the active configuration settings, directory status, group hierarchy tree,
/// unreferenced recipes, and validation summary.
///
/// # Workflow
///
/// 1. Use `context.config` already loaded from `context.config.path`.
/// 2. Print the resolved absolute path of `context.config.path` and `Settings:` (`Editor`,
///    `Simulate`), rendering `"-"` when `Editor` is empty.
/// 3. Validate `context.config.pods.path` and `context.config.recipes.path` on disk, count
///    known and unreferenced `.pod` files in `recipes.path`, and print the aligned
///    `Directories:` status lines (`[OK]`, `[OK, K known recipe(s) found]`,
///    `[OK, K known and U unreferenced recipe(s) found]`, `[NOT SET]`, or `[NOT FOUND]`).
/// 4. Iterate through sorted `context.config.recipes.groups`, expand `@<group>` references
///    for display, inspect `<recipe>.pod` files using [`targets::inspect_recipe`] with a local
///    lookup cache, and render the aligned `Groups:` tree (`[OK]`, `[ERROR]`, `[NOT FOUND]`,
///    `[MISSING GROUP]`, `[CYCLIC GROUP]`), emitting a truncation notice if output reaches
///    [`MAX_GROUP_OUTPUT_LINES`].
/// 5. When unreferenced `.pod` files exist in `recipes.path`, list their recipe stems under
///    `Unreferenced Recipes:` with status `[UNREFERENCED]`.
/// 6. Print the `Validation Summary:` for missing, invalid, or unreferenced recipes.
///
/// # Errors
///
/// Returns [`PodError`] if a required filesystem query fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    inspect_with(&context.config, &mut io::stdout().lock())
}

/// Appends a single group element (`@<subgroup>` or `<recipe>`) to `state`.
fn append_group_element<'a>(
    state: &mut GroupValidationState<'a>,
    config: &'a Config,
    element: &'a str,
    prefix: &str,
    is_last: bool,
    visiting: &mut BTreeSet<&'a str>,
) {
    if !has_output_capacity(state) {
        return;
    }
    let branch = if is_last { "└── " } else { "├── " };
    if let Some(subgroup_name) = element.strip_prefix('@') {
        state
            .output_lines
            .push(GroupOutputLine::Text(format!("{prefix}{branch}{element}")));
        append_subgroup(state, config, subgroup_name, prefix, is_last, visiting);
        return;
    }
    let line = format!("{prefix}{branch}{element}");
    append_recipe_status_line(state, config, element, line);
}

/// Appends all direct elements of a single recipe group to `state`.
fn append_group_elements<'a>(
    state: &mut GroupValidationState<'a>,
    config: &'a Config,
    elements: &'a [String],
    prefix: &str,
    visiting: &mut BTreeSet<&'a str>,
) {
    let element_count = elements.len();
    for (j, element) in elements.iter().enumerate() {
        if !has_output_capacity(state) {
            return;
        }
        let is_last = j + 1 == element_count;
        append_group_element(state, config, element.as_str(), prefix, is_last, visiting);
    }
}

/// Evaluates `<recipe>.pod` status (using `state.validation_cache`) and appends a status line to `state`.
fn append_recipe_status_line<'a>(
    state: &mut GroupValidationState<'a>,
    config: &'a Config,
    recipe_name: &'a str,
    line: String,
) {
    let status = record_referenced_recipe(state, config, recipe_name).badge();
    let visible_length = line.chars().count();
    if visible_length > state.max_line_length {
        state.max_line_length = visible_length;
    }
    state
        .output_lines
        .push(GroupOutputLine::Status { line, status });
}

/// Expands a `@<subgroup>` reference and appends its entries, `[MISSING GROUP]`, or `[CYCLIC GROUP]` to `state`.
fn append_subgroup<'a>(
    state: &mut GroupValidationState<'a>,
    config: &'a Config,
    subgroup_name: &'a str,
    parent_prefix: &str,
    parent_is_last: bool,
    visiting: &mut BTreeSet<&'a str>,
) {
    if !has_output_capacity(state) {
        return;
    }
    let indent = if parent_is_last { "    " } else { "│   " };
    let child_prefix = format!("{parent_prefix}{indent}");
    let Some(sub_elements) = config.recipes.groups.get(subgroup_name) else {
        state.output_lines.push(GroupOutputLine::Text(format!(
            "{child_prefix}└── [MISSING GROUP]"
        )));
        return;
    };
    if !visiting.insert(subgroup_name) {
        state.output_lines.push(GroupOutputLine::Text(format!(
            "{child_prefix}└── [CYCLIC GROUP]"
        )));
        return;
    }
    append_group_elements(state, config, sub_elements, &child_prefix, visiting);
    visiting.remove(subgroup_name);
}

/// Builds the group hierarchy output lines and recipe validation sets from `config`.
#[must_use]
fn build_groups_state(config: &Config) -> GroupValidationState<'_> {
    let mut state = GroupValidationState::new();
    let group_count = config.recipes.groups.len();
    let estimated_lines =
        collection::bounded_capacity(group_count.saturating_mul(4), MAX_COLLECTION_PREALLOC);
    state.output_lines = Vec::with_capacity(estimated_lines);
    let mut visiting = BTreeSet::new();

    for (i, (group_name, elements)) in config.recipes.groups.iter().enumerate() {
        if !has_output_capacity(&mut state) {
            break;
        }
        state
            .output_lines
            .push(GroupOutputLine::Text(format!("  • {group_name}")));
        visiting.insert(group_name.as_str());
        append_group_elements(&mut state, config, elements, "    ", &mut visiting);
        visiting.remove(group_name.as_str());
        if i + 1 < group_count && has_output_capacity(&mut state) {
            state
                .output_lines
                .push(GroupOutputLine::Text(String::new()));
        }
    }
    for recipe_name in config.get_recipes() {
        record_referenced_recipe(&mut state, config, recipe_name);
    }
    state
}

/// Returns `true` while `state.output_lines` is below [`MAX_GROUP_OUTPUT_LINES`], marking `state.truncated` when full.
fn has_output_capacity(state: &mut GroupValidationState<'_>) -> bool {
    if state.output_lines.len() >= MAX_GROUP_OUTPUT_LINES {
        state.truncated = true;
        return false;
    }
    true
}

/// Renders the full configuration inspection report for `config` to `stdout`.
fn inspect_with<W: Write>(config: &Config, stdout: &mut W) -> Result<(), PodError> {
    let state = build_groups_state(config);
    let scan = targets::list_unreferenced_recipes(&config.recipes.path, &state.referenced_recipes)?;
    render::write_header_and_settings(config, stdout);
    let (total_recipe_files, unreferenced_files) = match scan {
        Some((total, files)) => (Some(total), files),
        None => (None, Vec::new()),
    };
    let unreferenced_stems: Vec<&str> = unreferenced_files
        .iter()
        .map(|file| file.strip_suffix(RECIPE_EXTENSION).unwrap_or(file.as_str()))
        .collect();
    render::write_directories_section(config, total_recipe_files, unreferenced_stems.len(), stdout);
    render::write_groups_section(&state, stdout);
    render::write_unreferenced_section(&unreferenced_stems, stdout);
    render::write_validation_summary(
        &state.missing_recipes,
        &state.invalid_recipes,
        unreferenced_stems.len(),
        stdout,
    );
    Ok(())
}

/// Records `recipe_name` in `state.referenced_recipes`, inspects `<recipe>.pod` via `state.validation_cache`, and updates `state.missing_recipes` and `state.invalid_recipes`.
fn record_referenced_recipe<'a>(
    state: &mut GroupValidationState<'a>,
    config: &'a Config,
    recipe_name: &'a str,
) -> RecipeStatus {
    state.referenced_recipes.insert(recipe_name);
    let status = *state
        .validation_cache
        .entry(recipe_name)
        .or_insert_with(|| targets::inspect_recipe(config, recipe_name).status());
    match status {
        RecipeStatus::Error => {
            state.invalid_recipes.insert(recipe_name);
        }
        RecipeStatus::NotFound => {
            state.missing_recipes.insert(recipe_name);
        }
        RecipeStatus::Ok => {}
    }
    status
}

#[cfg(test)]
#[path = "inspect_tests.rs"]
mod tests;
