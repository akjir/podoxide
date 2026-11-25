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

//! Pod-level Podman lifecycle orchestration (`create`, `start`, `stop`, `remove`, `up`, `down`, `restart`, `reload`).

use std::io::{self, Write};

use crate::core::container::{self, validation as container_validation};
use crate::core::recipe::Recipe;
use crate::error::{PodError, RecipeError, RecipeErrorKind};
use crate::io::exec::command::ExecOutcome;
use crate::utilities::string;
use super::Pod;

/// Creates `recipe.pod` (`podman pod create`) and all `recipe.containers` (`podman create`) in `1..N` order.
///
/// # Errors
///
/// Returns [`PodError`] if pod or container creation fails.
pub fn create(recipe: &Recipe, simulate: bool) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    create_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Creates `recipe.pod` and all `recipe.containers` in `1..N` order, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn create_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    validate_recipe_create(recipe)?;
    let mut outcome = recipe.pod.create_validated(simulate, stdout, stderr)?;
    for item in &recipe.containers {
        let step = container::create_validated(item, &recipe.pod, simulate, stdout, stderr)?;
        outcome = outcome.combine(step);
    }
    Ok(outcome)
}

/// Stops and removes `recipe.containers` (in reverse `N..1` order) and `recipe.pod` (`stop` + `remove`).
///
/// # Errors
///
/// Returns [`PodError`] if stopping or removing the pod or its containers fails.
pub fn down(recipe: &Recipe, simulate: bool) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    down_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Stops and removes `recipe.containers` and `recipe.pod`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn down_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    let stop_outcome = stop_with(recipe, simulate, stdout, stderr)?;
    let remove_outcome = remove_with(recipe, simulate, stdout, stderr)?;
    Ok(stop_outcome.combine(remove_outcome))
}

/// Stops and removes `recipe.containers` and `recipe.pod`, then recreates `recipe.pod` and creates and starts `recipe.containers` (`down` + `up`).
///
/// # Errors
///
/// Returns [`PodError`] if tearing down or bringing up the pod fails.
pub fn reload(recipe: &Recipe, simulate: bool) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    reload_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Stops and removes `recipe.containers` and `recipe.pod`, then recreates `recipe.pod` and creates and starts `recipe.containers`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn reload_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    validate_recipe_create(recipe)?;
    let down_outcome = down_with(recipe, simulate, stdout, stderr)?;
    let up_outcome = up_with(recipe, simulate, stdout, stderr)?;
    Ok(down_outcome.combine(up_outcome))
}

/// Removes `recipe.containers` in reverse `N..1` order (`podman rm`) followed by `recipe.pod` (`podman pod rm`).
///
/// # Errors
///
/// Returns [`PodError`] if container or pod removal fails.
pub fn remove(recipe: &Recipe, simulate: bool) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    remove_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Removes `recipe.containers` in reverse `N..1` order followed by `recipe.pod`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn remove_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    validate_recipe_containers(recipe)?;
    let mut outcome = ExecOutcome::Succeeded;
    for item in recipe.containers.iter().rev() {
        let step = container::remove_with(item, simulate, stdout, stderr)?;
        outcome = outcome.combine(step);
    }
    let pod_outcome = recipe.pod.remove_with(simulate, stdout, stderr)?;
    Ok(outcome.combine(pod_outcome))
}

/// Stops `recipe.containers` and `recipe.pod` and starts `recipe.containers` in place (`stop` + `start`).
///
/// # Errors
///
/// Returns [`PodError`] if stopping the containers or pod or starting the containers fails.
pub fn restart(recipe: &Recipe, simulate: bool) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    restart_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Stops `recipe.containers` and `recipe.pod` and starts `recipe.containers` in place, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn restart_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    let stop_outcome = stop_with(recipe, simulate, stdout, stderr)?;
    let start_outcome = start_with(recipe, simulate, stdout, stderr)?;
    Ok(stop_outcome.combine(start_outcome))
}

/// Starts all `recipe.containers` (`podman start`) in `1..N` order.
///
/// # Errors
///
/// Returns [`PodError`] if pod validation or starting any container fails.
pub fn start(recipe: &Recipe, simulate: bool) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    start_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Starts all `recipe.containers` in `1..N` order, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if pod validation, stream writing, or Podman execution fails.
pub fn start_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    validate_recipe_containers(recipe)?;
    let mut outcome = ExecOutcome::Succeeded;
    for item in &recipe.containers {
        let step = container::start_with(item, simulate, stdout, stderr)?;
        outcome = outcome.combine(step);
    }
    Ok(outcome)
}

/// Stops `recipe.containers` in reverse `N..1` order (`podman stop`) followed by `recipe.pod` (`podman pod stop`).
///
/// # Errors
///
/// Returns [`PodError`] if stopping any container or the pod fails.
pub fn stop(recipe: &Recipe, simulate: bool) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    stop_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Stops `recipe.containers` in reverse `N..1` order followed by `recipe.pod`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn stop_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    validate_recipe_containers(recipe)?;
    let mut outcome = ExecOutcome::Succeeded;
    for item in recipe.containers.iter().rev() {
        let step = container::stop_with(item, simulate, stdout, stderr)?;
        outcome = outcome.combine(step);
    }
    let pod_outcome = recipe.pod.stop_with(simulate, stdout, stderr)?;
    Ok(outcome.combine(pod_outcome))
}

/// Creates `recipe.pod` and all `recipe.containers` and starts `recipe.containers` in `1..N` order (`create` + `start`).
///
/// # Errors
///
/// Returns [`PodError`] if creating the pod or its containers or starting the containers fails.
pub fn up(recipe: &Recipe, simulate: bool) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    up_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Creates `recipe.pod` and all `recipe.containers` and starts `recipe.containers` in `1..N` order, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn up_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    let create_outcome = create_with(recipe, simulate, stdout, stderr)?;
    let start_outcome = start_with(recipe, simulate, stdout, stderr)?;
    Ok(create_outcome.combine(start_outcome))
}

/// Validates that `pod.id` is a well-formed non-empty Podman pod name.
pub(super) fn validate_pod_id(pod: &Pod) -> Result<(), RecipeError> {
    if !string::is_valid_pod_or_container_name(&pod.id) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidPodId,
            pod.line,
            &pod.id,
        ));
    }
    Ok(())
}

/// Validates `recipe.pod` once and each container once before executing pod creation lifecycles.
pub(super) fn validate_recipe_create(recipe: &Recipe) -> Result<(), RecipeError> {
    recipe.pod.validate_create()?;
    container_validation::validate_optional_registry(&recipe.pod.registry, recipe.pod.line)?;
    for item in &recipe.containers {
        item.validate_create_for_validated_pod(&recipe.pod)?;
    }
    Ok(())
}

/// Validates `recipe.pod.id` and all container names in `recipe.containers` before executing container lifecycle commands.
fn validate_recipe_containers(recipe: &Recipe) -> Result<(), RecipeError> {
    validate_pod_id(&recipe.pod)?;
    for item in &recipe.containers {
        if !string::is_valid_pod_or_container_name(&item.name) {
            return Err(RecipeError::at_line_or_none(
                RecipeErrorKind::InvalidContainerName,
                item.line,
                &item.name,
            ));
        }
    }
    Ok(())
}
