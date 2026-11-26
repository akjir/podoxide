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

//! Recipe group membership queries, DAG cycle validation, and single-pass `@<group>` target expansion.

use std::collections::{BTreeMap, BTreeSet};

use crate::core::log;
use crate::error::{ConfigError, ConfigErrorKind, PodError};
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::string;
use super::{Config, MAX_GROUP_EXPANSION_DEPTH};

/// Depth-bounded DFS validator verifying acyclicity and [`MAX_GROUP_EXPANSION_DEPTH`] across groups.
struct GroupDepthValidator<'a> {
    group_lines: &'a BTreeMap<&'a str, usize>,
    groups: &'a BTreeMap<String, Vec<String>>,
    heights: BTreeMap<&'a str, usize>,
    visiting: BTreeSet<&'a str>,
}

impl<'a> GroupDepthValidator<'a> {
    /// Creates a new [`GroupDepthValidator`] for `groups` and their source line map `group_lines`.
    #[must_use]
    fn new(
        groups: &'a BTreeMap<String, Vec<String>>,
        group_lines: &'a BTreeMap<&'a str, usize>,
    ) -> Self {
        Self {
            group_lines,
            groups,
            heights: BTreeMap::new(),
            visiting: BTreeSet::new(),
        }
    }

    /// Computes and memoizes the maximum defined-group nesting height starting from `group_name`.
    fn compute_group_height(
        &mut self,
        group_name: &'a str,
        targets: &'a [String],
        fallback_line: usize,
    ) -> Result<usize, ConfigError> {
        if let Some(&cached_height) = self.heights.get(group_name) {
            return Ok(cached_height);
        }
        let line = self
            .group_lines
            .get(group_name)
            .copied()
            .unwrap_or(fallback_line);
        self.visiting.insert(group_name);
        let mut max_height = 1_usize;
        for target in targets {
            let candidate_height = self.compute_target_height(target.as_str(), line)?;
            max_height = max_height.max(candidate_height);
        }
        self.visiting.remove(group_name);
        self.heights.insert(group_name, max_height);
        Ok(max_height)
    }

    /// Evaluates a single target entry of the currently visiting group for cycles and [`MAX_GROUP_EXPANSION_DEPTH`].
    fn compute_target_height(
        &mut self,
        target: &'a str,
        line: usize,
    ) -> Result<usize, ConfigError> {
        let Some(subgroup) = target.strip_prefix('@') else {
            return Ok(1);
        };
        let Some(subgroup_targets) = self.groups.get(subgroup) else {
            return Ok(1);
        };
        if self.visiting.contains(subgroup) {
            return Err(ConfigError::with_token(
                ConfigErrorKind::InvalidGroupTarget,
                line,
                target,
            ));
        }
        if self.visiting.len() >= MAX_GROUP_EXPANSION_DEPTH {
            return Err(ConfigError::with_token(
                ConfigErrorKind::GroupDepthExceeded,
                line,
                target,
            ));
        }
        let child_height = self.compute_group_height(subgroup, subgroup_targets, line)?;
        let candidate_height = child_height.saturating_add(1);
        if candidate_height > MAX_GROUP_EXPANSION_DEPTH {
            return Err(ConfigError::with_token(
                ConfigErrorKind::GroupDepthExceeded,
                line,
                target,
            ));
        }
        Ok(candidate_height)
    }
}

/// Mutable single-pass state for target expansion, cycle detection, height memoization, and deduplication.
struct TargetExpander<'a> {
    collected: Vec<String>,
    expanded_groups: BTreeMap<&'a str, usize>,
    seen_recipes: BTreeSet<&'a str>,
    visiting: BTreeSet<&'a str>,
}

impl<'a> TargetExpander<'a> {
    /// Creates a new [`TargetExpander`] with bounded initial output capacity.
    #[must_use]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            collected: Vec::with_capacity(capacity),
            expanded_groups: BTreeMap::new(),
            seen_recipes: BTreeSet::new(),
            visiting: BTreeSet::new(),
        }
    }

    /// Appends `recipe` to `self.collected` if it has not been emitted already.
    fn push_unique(&mut self, recipe: &'a str) {
        if self.seen_recipes.insert(recipe) {
            self.collected.push(recipe.to_owned());
        }
    }
}

/// Expands `@<group>` and `<recipe>` targets against `config.recipes.groups` in a single deduplicated pass.
pub(super) fn expand_targets<'a>(
    config: &'a Config,
    targets: &'a [String],
) -> Result<Vec<String>, PodError> {
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    if log::is_debug_enabled() {
        log::debug(&format!("Targets   - {}", targets.join(" ")));
    }
    let capacity = collection::bounded_capacity(targets.len(), MAX_COLLECTION_PREALLOC);
    let mut expander = TargetExpander::with_capacity(capacity);

    for target in targets {
        expand_single_target(config, target.as_str(), &mut expander)?;
    }

    if log::is_debug_enabled() && !expander.collected.is_empty() {
        log::debug(&format!("Untangled - {}", expander.collected.join(" ")));
    }
    Ok(expander.collected)
}

/// Collects the sorted set of unique recipe names referenced across all groups in `config.recipes.groups`.
#[must_use]
pub(super) fn get_recipes(config: &Config) -> BTreeSet<&str> {
    let mut recipes = BTreeSet::new();
    for group_targets in config.recipes.groups.values() {
        collect_group_recipes(group_targets, &mut recipes);
    }
    recipes
}

/// Evaluates whether `recipe_name` is referenced in any group of `config.recipes.groups`.
#[must_use]
pub(super) fn has_recipe(config: &Config, recipe_name: &str) -> bool {
    if recipe_name.starts_with('@') || !string::is_valid_recipe_identifier(recipe_name) {
        return false;
    }
    config
        .recipes
        .groups
        .values()
        .any(|entries| group_contains_recipe(entries, recipe_name))
}

/// Verifies that `@<group>` references in `groups` form an acyclic DAG within [`MAX_GROUP_EXPANSION_DEPTH`].
pub(super) fn validate_groups_acyclic(
    groups: &BTreeMap<String, Vec<String>>,
    group_lines: &BTreeMap<&str, usize>,
    fallback_line: usize,
) -> Result<(), ConfigError> {
    let mut validator = GroupDepthValidator::new(groups, group_lines);
    for (group_name, targets) in groups {
        validator.compute_group_height(group_name.as_str(), targets, fallback_line)?;
    }
    Ok(())
}

/// Inserts all valid non-group recipe names from `group_targets` into `recipes`.
fn collect_group_recipes<'a>(group_targets: &'a [String], recipes: &mut BTreeSet<&'a str>) {
    for target in group_targets {
        if !target.starts_with('@') && string::is_valid_recipe_identifier(target) {
            recipes.insert(target.as_str());
        }
    }
}

/// Recursively expands a single `@<group>` target within [`MAX_GROUP_EXPANSION_DEPTH`], returning its nesting height.
fn expand_group_target<'a>(
    config: &'a Config,
    group_target: &'a str,
    group_name: &'a str,
    expander: &mut TargetExpander<'a>,
) -> Result<usize, ConfigError> {
    if !string::is_valid_identifier(group_name) {
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
    if expander.visiting.contains(group_name) {
        return Err(ConfigError::without_line(
            ConfigErrorKind::InvalidGroupTarget,
            group_target,
        ));
    }
    if let Some(&cached_height) = expander.expanded_groups.get(group_name) {
        if expander.visiting.len().saturating_add(cached_height) > MAX_GROUP_EXPANSION_DEPTH {
            return Err(ConfigError::without_line(
                ConfigErrorKind::GroupDepthExceeded,
                group_target,
            ));
        }
        return Ok(cached_height);
    }
    if expander.visiting.len() >= MAX_GROUP_EXPANSION_DEPTH {
        return Err(ConfigError::without_line(
            ConfigErrorKind::GroupDepthExceeded,
            group_target,
        ));
    }
    expander.visiting.insert(group_name);
    let mut max_height = 1_usize;
    for entry in entries {
        let child_height = expand_single_group_entry(config, entry.as_str(), expander)?;
        max_height = max_height.max(child_height.saturating_add(1));
    }
    expander.visiting.remove(group_name);
    expander.expanded_groups.insert(group_name, max_height);
    Ok(max_height)
}

/// Expands a single entry (`@<subgroup>` or `<recipe>`) declared inside a group in `config.recipes.groups`.
fn expand_single_group_entry<'a>(
    config: &'a Config,
    entry: &'a str,
    expander: &mut TargetExpander<'a>,
) -> Result<usize, ConfigError> {
    if let Some(subgroup_name) = entry.strip_prefix('@') {
        return expand_group_target(config, entry, subgroup_name, expander);
    }
    if !string::is_valid_recipe_identifier(entry) {
        return Err(ConfigError::without_line(
            ConfigErrorKind::InvalidGroupTarget,
            entry,
        ));
    }
    expander.push_unique(entry);
    Ok(0)
}

/// Expands a single CLI target (`@<group>` or `<recipe>`) into `expander`.
fn expand_single_target<'a>(
    config: &'a Config,
    target: &'a str,
    expander: &mut TargetExpander<'a>,
) -> Result<(), ConfigError> {
    if let Some(group_name) = target.strip_prefix('@') {
        expand_group_target(config, target, group_name, expander)?;
        return Ok(());
    }
    if !has_recipe(config, target) {
        return Err(ConfigError::without_line(
            ConfigErrorKind::RecipeNotFound,
            target,
        ));
    }
    expander.push_unique(target);
    Ok(())
}

/// Evaluates whether `entries` contains `recipe_name`.
#[must_use]
fn group_contains_recipe(entries: &[String], recipe_name: &str) -> bool {
    entries.iter().any(|entry| entry == recipe_name)
}
