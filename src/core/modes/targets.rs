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

//! Shared target resolution, lifecycle dispatch, recipe inspection, and directory discovery helpers for CLI modes.

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::core::config::Config;
use crate::core::container::Container;
use crate::core::context::Context;
use crate::core::log;
use crate::core::pod::Pod;
use crate::core::recipe::{RECIPE_EXTENSION, Recipe};
use crate::error::{CliError, CliErrorKind, ConfigError, ConfigErrorKind, PodError, RecipeErrorKind};
use crate::io::exec::command::ExecOutcome;
use crate::io::system::fs;
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::{path, string};

/// Loaded target recipes paired with deduplicated `(recipe_name, container_index)` target tuples.
pub(crate) type ResolvedContainerTargets<'a> = (BTreeMap<&'a str, Recipe>, Vec<(&'a str, usize)>);

/// Outcome of inspecting a configured `.pod` recipe file, retaining the loaded [`Recipe`] on success.
#[derive(Debug)]
pub(crate) enum RecipeInspection {
    Error,
    NotFound,
    Ok(Box<Recipe>),
}

impl RecipeInspection {
    /// Converts this inspection result into its lightweight [`RecipeStatus`] summary.
    #[inline]
    #[must_use]
    pub(crate) const fn status(&self) -> RecipeStatus {
        match self {
            Self::Error => RecipeStatus::Error,
            Self::NotFound => RecipeStatus::NotFound,
            Self::Ok(_) => RecipeStatus::Ok,
        }
    }
}

/// Lightweight validation status for a configured `.pod` recipe file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RecipeStatus {
    Error,
    NotFound,
    Ok,
}

impl RecipeStatus {
    /// Returns the bracketed status badge for this recipe inspection outcome.
    #[inline]
    #[must_use]
    pub(crate) const fn badge(self) -> &'static str {
        match self {
            Self::Error => "[ERROR]",
            Self::NotFound => "[NOT FOUND]",
            Self::Ok => "[OK]",
        }
    }
}

/// Parses and validates all `<recipe>/<container>` parameters in `context` and checks configuration membership.
///
/// # Errors
///
/// Returns [`PodError`] with [`CliErrorKind::MixedTargets`] if any parameter omits `'/'`,
/// [`CliErrorKind::InvalidTarget`] if a segment is malformed, or [`ConfigErrorKind::RecipeNotFound`]
/// if a referenced recipe is not registered in `context.config`.
pub(crate) fn collect_container_specs(context: &Context) -> Result<Vec<(&str, &str)>, PodError> {
    let capacity = collection::bounded_capacity(context.parameters.len(), MAX_COLLECTION_PREALLOC);
    let mut specs = Vec::with_capacity(capacity);
    for parameter in &context.parameters {
        if !parameter.contains('/') {
            return Err(PodError::from(CliError::with_argument(
                CliErrorKind::MixedTargets,
                parameter,
            )));
        }
        let (recipe_name, Some(container_spec)) = split_recipe_container_spec(parameter)? else {
            return Err(PodError::from(CliError::with_argument(
                CliErrorKind::InvalidTarget,
                parameter,
            )));
        };
        if !context.config.has_recipe(recipe_name) {
            return Err(PodError::from(ConfigError::without_line(
                ConfigErrorKind::RecipeNotFound,
                recipe_name,
            )));
        }
        specs.push((recipe_name, container_spec));
    }
    Ok(specs)
}

/// Expands `@<group>`, `<recipe>`, or `<recipe>/<container>` targets from `context.parameters`, pre-validates all target `.pod` recipes upfront, and executes `pod_action` or `container_action`.
///
/// # Errors
///
/// Returns [`PodError`] if `context.parameters` is empty, target validation or expansion fails, a
/// targeted `.pod` recipe file cannot be read or validated, a container target cannot be resolved,
/// or the lifecycle action fails.
pub(crate) fn execute_lifecycle_action<P, C>(
    context: &Context,
    pod_action: P,
    container_action: C,
) -> Result<ExecOutcome, PodError>
where
    P: FnMut(&Recipe, bool) -> Result<ExecOutcome, PodError>,
    C: FnMut(&Container, &Pod, bool) -> Result<ExecOutcome, PodError>,
{
    let Some(first) = context.parameters.first() else {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    };
    if first.contains('/') {
        return execute_container_targets(context, container_action);
    }
    execute_pod_targets(context, pod_action)
}

/// Validates that `context.parameters` contain no container target slashes and expands `@<group>` and `<recipe>` targets.
///
/// # Errors
///
/// Returns [`PodError`] with [`CliErrorKind::MixedTargets`] if any parameter contains `'/'`, or
/// propagates configuration target expansion errors.
pub(crate) fn expand_pod_targets(context: &Context) -> Result<Vec<String>, PodError> {
    for parameter in &context.parameters {
        if parameter.contains('/') {
            return Err(PodError::from(CliError::with_argument(
                CliErrorKind::MixedTargets,
                parameter,
            )));
        }
    }
    context.config.expand_targets(&context.parameters)
}

/// Checks whether `<recipe_name>.pod` exists on disk and validates it via [`Recipe::inspect`].
#[must_use]
pub(crate) fn inspect_recipe(config: &Config, recipe_name: &str) -> RecipeInspection {
    if !Recipe::file_exists(&config.recipes.path, recipe_name) {
        return RecipeInspection::NotFound;
    }
    match Recipe::inspect(config, recipe_name) {
        Ok(recipe) => RecipeInspection::Ok(Box::new(recipe)),
        Err(PodError::Recipe(ref error)) if error.kind == RecipeErrorKind::NotFound => {
            RecipeInspection::NotFound
        }
        Err(_) => RecipeInspection::Error,
    }
}

/// Lists `.pod` filenames in `recipes_path` whose recipe stem is absent from `referenced_recipes`, paired with the total `.pod` file count, or returns `Ok(None)` when `recipes_path` is invalid or missing.
///
/// # Errors
///
/// Returns [`PodError::Io`] if reading an existing valid `recipes_path` directory fails.
pub(crate) fn list_unreferenced_recipes(
    recipes_path: &Path,
    referenced_recipes: &BTreeSet<&str>,
) -> Result<Option<(usize, Vec<String>)>, PodError> {
    if !path::is_valid_directory(recipes_path) || !fs::directory_exists(recipes_path) {
        return Ok(None);
    }
    let files = fs::list_directory(recipes_path, Some(RECIPE_EXTENSION))?;
    let total_files = files.len();
    let capacity = collection::bounded_capacity(total_files, MAX_COLLECTION_PREALLOC);
    let mut unreferenced = Vec::with_capacity(capacity);
    for file in files {
        let stem = file.strip_suffix(RECIPE_EXTENSION).unwrap_or(file.as_str());
        if !referenced_recipes.contains(stem) {
            unreferenced.push(file);
        }
    }
    Ok(Some((total_files, unreferenced)))
}

/// Loads and validates all target `.pod` recipes upfront in expansion order.
///
/// # Errors
///
/// Returns [`PodError`] if any target `.pod` recipe file cannot be read or fails validation.
pub(crate) fn load_target_recipes(
    config: &Config,
    targets: &[String],
) -> Result<Vec<Recipe>, PodError> {
    let capacity = collection::bounded_capacity(targets.len(), MAX_COLLECTION_PREALLOC);
    let mut recipes = Vec::with_capacity(capacity);
    for target in targets {
        recipes.push(Recipe::inspect(config, target)?);
    }
    Ok(recipes)
}

/// Loads target recipes, resolves each container specification to its 0-based container index, and deduplicates `(recipe_name, container_index)` pairs in first-appearance order.
///
/// # Errors
///
/// Returns [`PodError`] if loading or validating a target recipe fails or any container
/// specification cannot be resolved.
pub(crate) fn resolve_container_targets<'a>(
    config: &Config,
    specs: &[(&'a str, &'a str)],
) -> Result<ResolvedContainerTargets<'a>, PodError> {
    let capacity = collection::bounded_capacity(specs.len(), MAX_COLLECTION_PREALLOC);
    let mut recipes = BTreeMap::new();
    let mut resolved = Vec::with_capacity(capacity);
    let mut seen = BTreeSet::new();
    for &(recipe_name, container_spec) in specs {
        let recipe = load_or_get_recipe(&mut recipes, config, recipe_name)?;
        let container_index = recipe.resolve_container_index(container_spec)?;
        if seen.insert((recipe_name, container_index)) {
            resolved.push((recipe_name, container_index));
        }
    }
    Ok((recipes, resolved))
}

/// Splits `target` on an optional `'/'` separator and validates `<recipe>` and `<container>` segments when `'/'` is present.
///
/// When `'/'` is absent, returns `Ok((target, None))` without restricting `target` to lowercase
/// recipe identifiers so callers such as `connect` can accept unmanaged container names.
///
/// # Errors
///
/// Returns [`PodError`] with [`CliErrorKind::InvalidTarget`] when `'/'` is present and either
/// segment is empty or invalid.
pub(crate) fn split_recipe_container_spec(target: &str) -> Result<(&str, Option<&str>), PodError> {
    if let Some((recipe_name, container_spec)) = target.split_once('/') {
        if !string::is_valid_target_segment(recipe_name)
            || !string::is_valid_target_segment(container_spec)
        {
            return Err(PodError::from(CliError::with_argument(
                CliErrorKind::InvalidTarget,
                target,
            )));
        }
        return Ok((recipe_name, Some(container_spec)));
    }
    Ok((target, None))
}

/// Resolves, deduplicates, and executes `container_action` across explicit `<recipe>/<container>` targets.
fn execute_container_targets<C>(
    context: &Context,
    mut container_action: C,
) -> Result<ExecOutcome, PodError>
where
    C: FnMut(&Container, &Pod, bool) -> Result<ExecOutcome, PodError>,
{
    if log::is_debug_enabled() {
        log::debug(&format!("Targets   - {}", context.parameters.join(" ")));
    }
    let specs = collect_container_specs(context)?;
    let (recipes, resolved) = resolve_container_targets(&context.config, &specs)?;
    let mut outcome = ExecOutcome::Succeeded;
    for &(recipe_name, container_index) in &resolved {
        let Some(recipe) = recipes.get(recipe_name) else {
            continue;
        };
        let Some(container) = recipe.containers.get(container_index) else {
            continue;
        };
        outcome = outcome.combine(container_action(
            container,
            &recipe.pod,
            context.config.simulate,
        )?);
    }
    Ok(outcome)
}

/// Expands `@<group>` and `<recipe>` targets, pre-validates all recipes upfront, and executes `pod_action` for each recipe.
fn execute_pod_targets<P>(context: &Context, mut pod_action: P) -> Result<ExecOutcome, PodError>
where
    P: FnMut(&Recipe, bool) -> Result<ExecOutcome, PodError>,
{
    let targets = expand_pod_targets(context)?;
    let recipes = load_target_recipes(&context.config, &targets)?;
    let mut outcome = ExecOutcome::Succeeded;
    for recipe in &recipes {
        outcome = outcome.combine(pod_action(recipe, context.config.simulate)?);
    }
    Ok(outcome)
}

/// Returns the cached [`Recipe`] for `recipe_name` or loads, validates, and caches it.
fn load_or_get_recipe<'a, 'b>(
    recipes: &'b mut BTreeMap<&'a str, Recipe>,
    config: &Config,
    recipe_name: &'a str,
) -> Result<&'b Recipe, PodError> {
    match recipes.entry(recipe_name) {
        Entry::Occupied(entry) => Ok(entry.into_mut()),
        Entry::Vacant(entry) => {
            let recipe = Recipe::inspect(config, recipe_name)?;
            Ok(entry.insert(recipe))
        }
    }
}

#[cfg(test)]
#[path = "targets_tests.rs"]
mod tests;
