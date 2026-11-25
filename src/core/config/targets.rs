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

/// Mutable single-pass state for target expansion, cycle detection, and order-preserving deduplication.
struct TargetExpander<'a> {
    collected: Vec<String>,
    expanded_groups: BTreeSet<&'a str>,
    seen_recipes: BTreeSet<&'a str>,
    visiting: BTreeSet<&'a str>,
}

impl<'a> TargetExpander<'a> {
    /// Creates a new [`TargetExpander`] with bounded initial output capacity.
    #[must_use]
    fn with_capacity(capacity: usize) -> Self {
        Self {
            collected: Vec::with_capacity(capacity),
            expanded_groups: BTreeSet::new(),
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

/// Inserts all valid non-group recipe names from `group_targets` into `recipes`.
fn collect_group_recipes<'a>(group_targets: &'a [String], recipes: &mut BTreeSet<&'a str>) {
    for target in group_targets {
        if !target.starts_with('@') && string::is_valid_recipe_identifier(target) {
            recipes.insert(target.as_str());
        }
    }
}

/// Recursively expands a single `@<group>` target from `config.recipes.groups` within [`MAX_GROUP_EXPANSION_DEPTH`].
fn expand_group_target<'a>(
    config: &'a Config,
    group_target: &'a str,
    group_name: &'a str,
    expander: &mut TargetExpander<'a>,
) -> Result<(), ConfigError> {
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
    if expander.visiting.contains(group_name)
        || expander.visiting.len() >= MAX_GROUP_EXPANSION_DEPTH
    {
        return Err(ConfigError::without_line(
            ConfigErrorKind::InvalidGroupTarget,
            group_target,
        ));
    }
    if expander.expanded_groups.contains(group_name) {
        return Ok(());
    }
    expander.visiting.insert(group_name);
    for entry in entries {
        expand_single_group_entry(config, entry.as_str(), expander)?;
    }
    expander.visiting.remove(group_name);
    expander.expanded_groups.insert(group_name);
    Ok(())
}

/// Expands a single entry (`@<subgroup>` or `<recipe>`) declared inside a group in `config.recipes.groups`.
fn expand_single_group_entry<'a>(
    config: &'a Config,
    entry: &'a str,
    expander: &mut TargetExpander<'a>,
) -> Result<(), ConfigError> {
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
    Ok(())
}

/// Expands a single CLI target (`@<group>` or `<recipe>`) into `expander`.
fn expand_single_target<'a>(
    config: &'a Config,
    target: &'a str,
    expander: &mut TargetExpander<'a>,
) -> Result<(), ConfigError> {
    if let Some(group_name) = target.strip_prefix('@') {
        return expand_group_target(config, target, group_name, expander);
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

/// Performs depth-bounded DFS cycle detection starting from `group_name`.
fn verify_group_acyclic<'a>(
    group_name: &'a str,
    groups: &'a BTreeMap<String, Vec<String>>,
    group_lines: &BTreeMap<&str, usize>,
    visiting: &mut BTreeSet<&'a str>,
    verified: &mut BTreeSet<&'a str>,
    fallback_line: usize,
) -> Result<(), ConfigError> {
    let Some(targets) = groups.get(group_name) else {
        return Ok(());
    };
    let line = group_lines
        .get(group_name)
        .copied()
        .unwrap_or(fallback_line);
    if visiting.len() >= MAX_GROUP_EXPANSION_DEPTH {
        return Err(ConfigError::with_token(
            ConfigErrorKind::InvalidGroupTarget,
            line,
            group_name,
        ));
    }
    if verified.contains(group_name) {
        return Ok(());
    }
    visiting.insert(group_name);
    verify_group_targets(targets, groups, group_lines, visiting, verified, line)?;
    visiting.remove(group_name);
    verified.insert(group_name);
    Ok(())
}

/// Inspects all `@<subgroup>` targets of the currently visiting group for cycles and depth limits.
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
        if visiting.contains(subgroup) || visiting.len() >= MAX_GROUP_EXPANSION_DEPTH {
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
