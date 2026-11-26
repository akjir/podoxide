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

//! Handler for `pods status [targets...]`.

mod table;

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use crate::core::config::Config;
use crate::core::context::Context;
use crate::core::log;
use crate::core::modes::targets::{self, RecipeInspection};
use crate::core::recipe::Recipe;
use crate::error::PodError;
use crate::io::exec::podman::{self, RECORD_DELIMITER};
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use self::table::StatusRow;

const INITIAL_STATUS_CAPACITY: usize = 16;
const MAX_STATUS_RECORDS: usize = 1024;

/// Expected metadata for a managed container declared in a configured `.pod` recipe.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ExpectedContainer {
    target_name: String,
}

/// Expected managed containers indexed by `pod_id -> container_name -> ExpectedContainer`.
type ExpectedContainerMap = BTreeMap<String, BTreeMap<String, ExpectedContainer>>;

/// Resolved expected container metadata paired with sorted invalid and missing recipe names.
#[derive(Debug, Default)]
struct ExpectedTargets {
    expected: ExpectedContainerMap,
    invalid_recipes: Vec<String>,
    missing_recipes: Vec<String>,
}

/// Queries `podman ps -a`, cross-references managed recipes, and formats the runtime status
/// table.
///
/// # Workflow
///
/// 1. Resolve the expected managed containers to inspect:
///    - If `context.parameters` contains `<recipe>/<container>` targets, resolve only the
///      targeted containers and their `<recipe>/<index>` target names.
///    - If `context.parameters` contains `@<group>` or `<recipe>` targets, expand and
///      deduplicate them against `context.config.recipes.groups` and inspect each recipe via
///      [`targets::inspect_recipe`].
///    - If `context.parameters` is empty, collect and inspect all unique configured recipes from
///      `context.config.recipes.groups`.
/// 2. Query `podman ps -a --format ...` to retrieve all host container records.
/// 3. Filter or merge container records (`--all` includes unmanaged host containers; missing
///    managed containers and missing `.pod` files are synthesized with status `"Not Found"`;
///    invalid recipes are synthesized with status `"Recipe Error"`).
/// 4. Sort rows by `(pod, name)` and print the aligned status table (`--full` inserts
///    `TARGET NAME` after `NAME` and appends `COMMAND` and `PORTS` columns), followed by a
///    `pods recipe inspect` hint only when one or more targeted recipes failed validation.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion or container resolution fails or `podman ps` fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let mut stdout = io::stdout().lock();
    let mut query_status = podman::query_ps_status;
    execute_with(context, &mut query_status, &mut stdout)
}

/// Appends synthesized `"Not Found"` rows for expected containers in `pod_id` absent from `found_keys`.
fn append_missing_pod_rows<'a>(
    pod_id: &'a str,
    pod_containers: &'a BTreeMap<String, ExpectedContainer>,
    found_keys: &BTreeSet<(&'a str, &'a str)>,
    display_rows: &mut Vec<StatusRow<'a>>,
) {
    for (expected_name, container) in pod_containers {
        if display_rows.len() >= MAX_STATUS_RECORDS {
            break;
        }
        if !found_keys.contains(&(pod_id, expected_name.as_str())) {
            display_rows.push(StatusRow::missing(
                expected_name.as_str(),
                pod_id,
                container.target_name.as_str(),
            ));
        }
    }
}

/// Appends synthesized `"Not Found"` rows for configured recipes whose `.pod` file does not exist on disk.
fn append_missing_recipe_rows<'a>(
    missing_recipes: &'a [String],
    display_rows: &mut Vec<StatusRow<'a>>,
) {
    for recipe_name in missing_recipes {
        if display_rows.len() >= MAX_STATUS_RECORDS {
            break;
        }
        display_rows.push(StatusRow::missing_recipe(recipe_name.as_str()));
    }
}

/// Appends synthesized `"Not Found"` rows for expected containers absent from `found_keys`.
fn append_missing_rows<'a>(
    expected: &'a ExpectedContainerMap,
    found_keys: &BTreeSet<(&'a str, &'a str)>,
    display_rows: &mut Vec<StatusRow<'a>>,
) {
    for (pod_id, pod_containers) in expected {
        if display_rows.len() >= MAX_STATUS_RECORDS {
            break;
        }
        append_missing_pod_rows(pod_id.as_str(), pod_containers, found_keys, display_rows);
    }
}

/// Appends synthesized `"Recipe Error"` rows for invalid recipes in `invalid_recipes`.
fn append_recipe_error_rows<'a>(
    invalid_recipes: &'a [String],
    display_rows: &mut Vec<StatusRow<'a>>,
) {
    for recipe_name in invalid_recipes {
        if display_rows.len() >= MAX_STATUS_RECORDS {
            break;
        }
        display_rows.push(StatusRow::recipe_error(recipe_name.as_str()));
    }
}

/// Filters or merges `podman ps` output with `expected` managed containers, `missing_recipes`, and `invalid_recipes`, sorting the result by `(pod, name)`.
#[must_use]
fn build_display_rows<'a>(
    ps_output: &'a str,
    expected: &'a ExpectedContainerMap,
    missing_recipes: &'a [String],
    invalid_recipes: &'a [String],
    include_all: bool,
) -> Vec<StatusRow<'a>> {
    let base_count = count_expected_containers(expected)
        .saturating_add(missing_recipes.len())
        .saturating_add(invalid_recipes.len());
    let estimated = if include_all {
        base_count.max(INITIAL_STATUS_CAPACITY)
    } else {
        base_count
    };
    let capacity = collection::bounded_capacity(estimated, MAX_COLLECTION_PREALLOC);
    let mut display_rows = Vec::with_capacity(capacity);
    let mut found_keys = BTreeSet::new();

    for raw_record in ps_output.split(RECORD_DELIMITER) {
        if display_rows.len() >= MAX_STATUS_RECORDS {
            break;
        }
        let record = raw_record.trim_start_matches(['\r', '\n']);
        let Some(mut row) = table::parse_status_record(record) else {
            continue;
        };
        if let Some(expected_container) = expected
            .get(row.pod)
            .and_then(|pod_map| pod_map.get(row.name))
        {
            row.target_name = expected_container.target_name.as_str();
            found_keys.insert((row.pod, row.name));
            display_rows.push(row);
        } else if include_all {
            display_rows.push(row);
        }
    }

    append_missing_rows(expected, &found_keys, &mut display_rows);
    append_missing_recipe_rows(missing_recipes, &mut display_rows);
    append_recipe_error_rows(invalid_recipes, &mut display_rows);
    display_rows.sort_unstable_by(|left, right| {
        left.pod
            .cmp(right.pod)
            .then_with(|| left.name.cmp(right.name))
    });
    display_rows
}

/// Inspects each target recipe in `targets` to populate expected container metadata, missing recipes, and invalid recipe names.
#[must_use]
fn collect_expected_containers(config: &Config, targets: &[Cow<'_, str>]) -> ExpectedTargets {
    let mut expected = BTreeMap::new();
    let mut expected_count = 0usize;
    let mut invalid_set = BTreeSet::new();
    let mut missing_set = BTreeSet::new();
    for target in targets {
        let total = expected_count
            .saturating_add(invalid_set.len())
            .saturating_add(missing_set.len());
        if total >= MAX_STATUS_RECORDS {
            break;
        }
        insert_recipe_containers(
            config,
            target.as_ref(),
            &mut expected,
            &mut expected_count,
            &mut missing_set,
            &mut invalid_set,
        );
    }
    ExpectedTargets {
        expected,
        invalid_recipes: invalid_set.into_iter().collect(),
        missing_recipes: missing_set.into_iter().collect(),
    }
}

/// Resolves explicit `<recipe>/<container>` CLI parameters into expected container metadata.
fn collect_targeted_containers(context: &Context) -> Result<ExpectedTargets, PodError> {
    let specs = targets::collect_container_specs(context)?;
    let (recipes, resolved) = targets::resolve_container_targets(&context.config, &specs)?;
    let mut expected = BTreeMap::new();
    let mut expected_count = 0usize;
    for (recipe_name, container_index) in resolved {
        if expected_count >= MAX_STATUS_RECORDS {
            break;
        }
        insert_targeted_container(
            &recipes,
            recipe_name,
            container_index,
            &mut expected,
            &mut expected_count,
        );
    }
    Ok(ExpectedTargets {
        expected,
        invalid_recipes: Vec::new(),
        missing_recipes: Vec::new(),
    })
}

/// Returns the total number of expected containers across all pods in `expected`.
#[must_use]
fn count_expected_containers(expected: &ExpectedContainerMap) -> usize {
    expected
        .values()
        .map(BTreeMap::len)
        .fold(0usize, usize::saturating_add)
}

/// Orchestrates `pods status` target resolution, Podman querying, recipe cross-referencing, and table rendering.
fn execute_with<W, F>(
    context: &Context,
    query_status: &mut F,
    stdout: &mut W,
) -> Result<(), PodError>
where
    W: Write,
    F: FnMut() -> Option<String>,
{
    let resolved = resolve_expected_containers(context)?;
    let ps_output = query_podman_status(query_status)?;
    let display_rows = build_display_rows(
        &ps_output,
        &resolved.expected,
        &resolved.missing_recipes,
        &resolved.invalid_recipes,
        context.flags.all(),
    );
    if display_rows.is_empty() {
        log::write_print(stdout, "No containers found.");
        return Ok(());
    }
    table::write_status_table(&display_rows, context.flags.full(), stdout);
    table::write_invalid_recipes_hint(&resolved.invalid_recipes, stdout);
    Ok(())
}

/// Inspects `target` via [`targets::inspect_recipe`] and inserts its containers into `expected`, or records `target` in `missing_recipes` or `invalid_recipes`.
fn insert_recipe_containers(
    config: &Config,
    target: &str,
    expected: &mut ExpectedContainerMap,
    expected_count: &mut usize,
    missing_recipes: &mut BTreeSet<String>,
    invalid_recipes: &mut BTreeSet<String>,
) {
    let recipe = match targets::inspect_recipe(config, target) {
        RecipeInspection::Ok(recipe) => recipe,
        RecipeInspection::NotFound => {
            missing_recipes.insert(target.to_owned());
            return;
        }
        RecipeInspection::Error => {
            invalid_recipes.insert(target.to_owned());
            return;
        }
    };
    let pod_map = expected.entry(recipe.pod.id).or_default();
    for (index, container) in recipe.containers.into_iter().enumerate() {
        if *expected_count >= MAX_STATUS_RECORDS {
            break;
        }
        let target_name = format!("{target}/{}", index + 1);
        if pod_map
            .insert(container.name, ExpectedContainer { target_name })
            .is_none()
        {
            *expected_count += 1;
        }
    }
}

/// Inserts a resolved `(recipe_name, container_index)` target into `expected` with its 1-based `<recipe>/<index>` target name.
fn insert_targeted_container(
    recipes: &BTreeMap<&str, Recipe>,
    recipe_name: &str,
    container_index: usize,
    expected: &mut ExpectedContainerMap,
    expected_count: &mut usize,
) {
    let Some(recipe) = recipes.get(recipe_name) else {
        return;
    };
    let Some(container) = recipe.containers.get(container_index) else {
        return;
    };
    let target_name = format!("{recipe_name}/{}", container_index + 1);
    let pod_map = expected.entry(recipe.pod.id.clone()).or_default();
    if pod_map
        .insert(container.name.clone(), ExpectedContainer { target_name })
        .is_none()
    {
        *expected_count += 1;
    }
}

/// Queries `podman ps -a` status output via `query_status` and returns the raw output.
fn query_podman_status<F>(query_status: &mut F) -> Result<String, PodError>
where
    F: FnMut() -> Option<String>,
{
    query_status().ok_or_else(|| PodError::from(io::Error::other("Failed to query podman status.")))
}

/// Resolves expected managed containers, missing recipes, and invalid recipes from `<recipe>/<container>` targets, `@<group>`/`<recipe>` targets, or all configured recipes.
fn resolve_expected_containers(context: &Context) -> Result<ExpectedTargets, PodError> {
    if context
        .parameters
        .iter()
        .any(|parameter| parameter.contains('/'))
    {
        return collect_targeted_containers(context);
    }
    let target_names = resolve_targets(context)?;
    Ok(collect_expected_containers(&context.config, &target_names))
}

/// Resolves target recipe names from `context.parameters` or all configured recipes in `context.config`.
fn resolve_targets(context: &Context) -> Result<Vec<Cow<'_, str>>, PodError> {
    if !context.parameters.is_empty() {
        let expanded = context.config.expand_targets(&context.parameters)?;
        let capacity = collection::bounded_capacity(expanded.len(), MAX_COLLECTION_PREALLOC);
        let mut names = Vec::with_capacity(capacity);
        for target in expanded {
            names.push(Cow::Owned(target));
        }
        return Ok(names);
    }
    let recipes = context.config.get_recipes();
    let capacity = collection::bounded_capacity(recipes.len(), MAX_COLLECTION_PREALLOC);
    let mut names = Vec::with_capacity(capacity);
    for recipe in recipes {
        names.push(Cow::Borrowed(recipe));
    }
    Ok(names)
}

#[cfg(test)]
mod tests;
