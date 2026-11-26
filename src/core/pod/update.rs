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

//! Pod-level image pulling, shared pull caching, stale-container detection, and conditional pod reload.

use std::io::{self, Write};

use crate::core::container::{self, validation as container_validation};
use crate::core::log;
use crate::core::recipe::Recipe;
use crate::core::update::{
    ContainerPullRecord, IndentedWriter, POD_INDENT, PodUpdateResult, PullCache, PullRunner,
    UpdateAction, UpdateRunners,
};
use crate::error::{PodError, RecipeError, RecipeErrorKind};
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::{self, PullOutcome};
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::string;
use super::lifecycle;

/// Pulls the latest images (`podman pull`) for all `recipe.containers` in `1..N` order without restarting the pod.
///
/// # Errors
///
/// Returns [`PodError`] if pulling any container image fails.
pub fn pull(recipe: &Recipe, simulate: bool) -> Result<PodUpdateResult, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    pull_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Pulls the latest images for all `recipe.containers` in `1..N` order, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if pod/container validation or Podman execution fails.
pub fn pull_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<PodUpdateResult, PodError> {
    let mut cache = PullCache::new();
    pull_with_cache(
        recipe,
        simulate,
        &mut cache,
        &mut podman::pull_image,
        stdout,
        stderr,
    )
}

/// Pulls the latest images for `recipe.containers` and reloads the pod when at least one image was updated or stale (`pull` + conditional `reload`).
///
/// # Errors
///
/// Returns [`PodError`] if pulling images or reloading the pod fails.
pub fn update(recipe: &Recipe, simulate: bool) -> Result<PodUpdateResult, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    update_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Pulls the latest images for `recipe.containers` and reloads the pod when at least one image was updated, stale, or simulated, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn update_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<PodUpdateResult, PodError> {
    let mut cache = PullCache::new();
    let mut pull_image = podman::pull_image;
    let mut query_container_image = |name: &str| {
        if simulate {
            None
        } else {
            podman::query_container_image_id(name)
        }
    };
    let mut query_image = |reference: &str| {
        if simulate {
            None
        } else {
            podman::query_image_id(reference)
        }
    };
    let mut runners = UpdateRunners {
        cache: &mut cache,
        pull_image: &mut pull_image,
        query_container_image: &mut query_container_image,
        query_image: &mut query_image,
    };
    update_with_cache(recipe, simulate, &mut runners, stdout, stderr)
}

/// Pulls the latest images for all `recipe.containers` in `1..N` order using `cache` and `pull_image` without reloading the pod.
pub(crate) fn pull_with_cache<W, E, P>(
    recipe: &Recipe,
    simulate: bool,
    cache: &mut PullCache,
    pull_image: &mut P,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<PodUpdateResult, PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
{
    validate_recipe_pull(recipe)?;
    let containers =
        pull_containers_with_cache(recipe, simulate, cache, pull_image, stdout, stderr)?;
    let action = derive_pull_action(&containers);
    Ok(PodUpdateResult {
        action,
        containers,
        pod_id: recipe.pod.id.clone(),
        pod_name: recipe.pod.name.clone(),
    })
}

/// Pulls the latest images for `recipe.containers` via `runners`, detects stale container image IDs, and reloads the pod when needed.
pub(crate) fn update_with_cache<W, E, P, C, I>(
    recipe: &Recipe,
    simulate: bool,
    runners: &mut UpdateRunners<'_, P, C, I>,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<PodUpdateResult, PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
    C: FnMut(&str) -> Option<String>,
    I: FnMut(&str) -> Option<String>,
{
    lifecycle::validate_recipe_create(recipe)?;
    let mut containers = pull_containers_with_cache(
        recipe,
        simulate,
        runners.cache,
        runners.pull_image,
        stdout,
        stderr,
    )?;
    let has_failed_pull = containers
        .iter()
        .any(|record| record.outcome == PullOutcome::Failed);
    if !has_failed_pull {
        for record in &mut containers {
            let previous_outcome = record.outcome;
            record.outcome = container::detect_stale_container_image(
                &record.container_name,
                &record.image,
                record.outcome,
                runners.cache,
                runners.query_container_image,
                runners.query_image,
            );
            if previous_outcome == PullOutcome::UpToDate && record.outcome == PullOutcome::Updated {
                log::write_print(
                    stdout,
                    &format!(
                        "{POD_INDENT}Container '{}' is running an older image; reloading.",
                        record.container_name
                    ),
                );
            }
        }
    }
    let action = apply_pod_update_action(recipe, &containers, simulate, stdout, stderr)?;
    Ok(PodUpdateResult {
        action,
        containers,
        pod_id: recipe.pod.id.clone(),
        pod_name: recipe.pod.name.clone(),
    })
}

/// Executes the post-pull pod lifecycle action (indented reload, up-to-date message, or failure notice).
fn apply_pod_update_action<W: Write, E: Write>(
    recipe: &Recipe,
    containers: &[ContainerPullRecord],
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<UpdateAction, PodError> {
    match derive_pull_action(containers) {
        UpdateAction::Failed => {
            log::write_print(
                stdout,
                &format!(
                    "{POD_INDENT}Pod '{}' ('{}') update failed.",
                    recipe.pod.name, recipe.pod.id
                ),
            );
            Ok(UpdateAction::Failed)
        }
        UpdateAction::Reloaded => {
            let mut indented = IndentedWriter::new(stdout, POD_INDENT);
            let reload_outcome = lifecycle::reload_with(recipe, simulate, &mut indented, stderr)?;
            if reload_outcome == ExecOutcome::Failed {
                log::write_print(
                    stdout,
                    &format!(
                        "{POD_INDENT}Pod '{}' ('{}') update failed.",
                        recipe.pod.name, recipe.pod.id
                    ),
                );
                return Ok(UpdateAction::Failed);
            }
            Ok(UpdateAction::Reloaded)
        }
        UpdateAction::Simulated => {
            let mut indented = IndentedWriter::new(stdout, POD_INDENT);
            let _ = lifecycle::reload_with(recipe, true, &mut indented, stderr)?;
            Ok(UpdateAction::Simulated)
        }
        UpdateAction::UpToDate => {
            log::write_print(
                stdout,
                &format!(
                    "{POD_INDENT}Pod '{}' ('{}') is up to date.",
                    recipe.pod.name, recipe.pod.id
                ),
            );
            Ok(UpdateAction::UpToDate)
        }
    }
}

/// Derives the pod-level [`UpdateAction`] from per-container pull outcomes.
#[must_use]
fn derive_pull_action(containers: &[ContainerPullRecord]) -> UpdateAction {
    let mut any_simulated = false;
    let mut any_failed = false;
    let mut any_updated = false;
    for record in containers {
        match record.outcome {
            PullOutcome::Failed => any_failed = true,
            PullOutcome::Simulated => any_simulated = true,
            PullOutcome::UpToDate => {}
            PullOutcome::Updated => any_updated = true,
        }
    }
    if any_simulated {
        return UpdateAction::Simulated;
    }
    if any_failed {
        return UpdateAction::Failed;
    }
    if any_updated {
        return UpdateAction::Reloaded;
    }
    UpdateAction::UpToDate
}

/// Prints the pod header and pulls all `recipe.containers` in `1..N` order using `cache` and `pull_image`.
fn pull_containers_with_cache<W, E, P>(
    recipe: &Recipe,
    simulate: bool,
    cache: &mut PullCache,
    pull_image: &mut P,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<Vec<ContainerPullRecord>, PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
{
    log::write_print(
        stdout,
        &format!("Update pod '{}' ('{}') ...", recipe.pod.name, recipe.pod.id),
    );
    let capacity = collection::bounded_capacity(recipe.containers.len(), MAX_COLLECTION_PREALLOC);
    let mut records = Vec::with_capacity(capacity);
    let mut runner = PullRunner { cache, pull_image };
    for item in &recipe.containers {
        let record = container::pull_with_cache(
            item,
            &recipe.pod,
            POD_INDENT,
            simulate,
            &mut runner,
            stdout,
            stderr,
        )?;
        records.push(record);
    }
    Ok(records)
}

/// Validates `recipe.pod.id`, `recipe.pod.registry`, and all `recipe.containers` before pulling container images.
fn validate_recipe_pull(recipe: &Recipe) -> Result<(), RecipeError> {
    lifecycle::validate_pod_id(&recipe.pod)?;
    container_validation::validate_optional_registry(&recipe.pod.registry, recipe.pod.line)?;
    for item in &recipe.containers {
        if !string::is_valid_pod_or_container_name(&item.name) {
            return Err(RecipeError::at_line_or_none(
                RecipeErrorKind::InvalidContainerName,
                item.line,
                &item.name,
            ));
        }
        item.validate()?;
        container_validation::validate_resolved_image(item, &recipe.pod)?;
    }
    Ok(())
}
