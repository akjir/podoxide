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

//! Handler for `pods connect [shell] <target>`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::modes::targets;
use crate::core::recipe::Recipe;
use crate::error::{
    CliError, CliErrorKind, ConfigError, ConfigErrorKind, PodError, RecipeError, RecipeErrorKind,
};
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::{self, ExecContainer, SHELL_FALLBACK_COMMAND};
use crate::utilities::string;

/// Output prefix printed before simulating or executing the interactive shell command.
const CONNECT_PREFIX: &str = "Execute connect command: ";
/// Default 1-based container index used when a recipe defines a single container.
const DEFAULT_CONTAINER_INDEX: &str = "1";

/// Resolves the target container and opens an interactive shell session via `podman exec -it`.
///
/// # Workflow
///
/// 1. Extract `<target>` (`context.parameters[0]`) and split an optional `/<container>`
///    specification (`<recipe>/<container>` vs `<target>`).
/// 2. Check whether `<recipe>` is registered in `context.config.recipes.groups`:
///    - If `<recipe>` is registered: read and validate only that single `.pod` file. If no
///      `/<container>` suffix was provided, require that the recipe defines exactly 1
///      container (defaulting to index `1`); otherwise resolve `<container>` by 1-based
///      index, relative name (`*db` / `db`), or full name.
///    - If `<recipe>` is not registered and no `/<container>` suffix was given: treat
///      `<target>` directly as an unmanaged Podman container name without reading any
///      `.pod` file (unreferenced `.pod` files on disk never hijack direct container names).
///    - If `<recipe>` is not registered and a `/<container>` suffix was given: return
///      [`ConfigErrorKind::RecipeNotFound`].
/// 3. When `context.config.simulate` is `false`, verify via `podman` that the resolved
///    container is running, distinguishing stopped unmanaged containers
///    ([`CliErrorKind::ContainerNotRunning`]) from nonexistent ones
///    ([`CliErrorKind::TargetNotFound`]).
/// 4. Execute (or simulate) `podman exec -it <container_name> ...` interactively, preferring
///    `bash` and falling back to `sh`.
///
/// # Errors
///
/// Returns [`PodError`] if the recipe or container cannot be resolved, the target container
/// is not running or does not exist, or the interactive `podman exec` process fails.
pub fn handle(context: &Context) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    execute_with(
        context,
        podman::container_running,
        podman::container_exists,
        &mut stdout,
        &mut stderr,
    )
}

/// Resolves the target container from `context`, checks runtime status via `is_running` and `container_exists`, and runs `podman exec -it`.
///
/// # Errors
///
/// Returns [`PodError`] if the CLI target is missing or invalid, recipe loading or container
/// resolution fails, the container is not running or does not exist, or `podman exec` fails.
fn execute_with<W, E, R, X>(
    context: &Context,
    is_running: R,
    container_exists: X,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError>
where
    W: Write,
    E: Write,
    R: FnOnce(&str) -> bool,
    X: FnOnce(&str) -> bool,
{
    let target = extract_target_parameter(context)?;
    let (recipe_name, container_spec) = targets::split_recipe_container_spec(target)?;
    let is_registered_recipe = context.config.has_recipe(recipe_name);
    let container_name =
        resolve_target_container(context, recipe_name, container_spec, is_registered_recipe)?;
    verify_running_container(
        &container_name,
        target,
        is_registered_recipe,
        context.config.simulate,
        is_running,
        container_exists,
    )?;
    podman::exec_container(
        &ExecContainer {
            command: &SHELL_FALLBACK_COMMAND,
            container_name: &container_name,
            interactive: true,
            user: None,
        },
        CONNECT_PREFIX,
        context.config.simulate,
        stdout,
        stderr,
    )
}

/// Extracts and validates the single positional `<target>` parameter from `context.parameters`.
///
/// # Errors
///
/// Returns [`PodError`] with [`CliErrorKind::MissingTarget`] when no parameter is present,
/// [`CliErrorKind::TooManyArguments`] when extra parameters are present, or
/// [`CliErrorKind::InvalidTarget`] when the target text is empty or contains control characters.
fn extract_target_parameter(context: &Context) -> Result<&str, PodError> {
    let Some(target) = context.parameters.first() else {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    };
    if let Some(extra) = context.parameters.get(1) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::TooManyArguments,
            extra,
        )));
    }
    if !string::is_valid_text(target) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidTarget,
            target,
        )));
    }
    Ok(target)
}

/// Loads and validates `<recipe_name>.pod` and resolves the target container name.
///
/// # Errors
///
/// Returns [`PodError`] if the recipe fails to load or validate, has multiple containers
/// when `container_spec` is `None`, or does not contain a matching container.
fn resolve_recipe_container(
    context: &Context,
    recipe_name: &str,
    container_spec: Option<&str>,
) -> Result<String, PodError> {
    let recipe = Recipe::inspect(&context.config, recipe_name)?;
    let effective_spec = match container_spec {
        Some(spec) => spec,
        None if recipe.containers.len() == 1 => DEFAULT_CONTAINER_INDEX,
        None => {
            return Err(PodError::from(RecipeError::without_line(
                RecipeErrorKind::MultipleContainers,
                recipe_name,
            )));
        }
    };
    let container_name = recipe.resolve_container_name(effective_spec)?;
    Ok(container_name)
}

/// Resolves the absolute Podman container name from either a registered `.pod` recipe or a direct container target.
///
/// # Errors
///
/// Returns [`PodError`] if recipe or unmanaged container resolution fails.
fn resolve_target_container(
    context: &Context,
    recipe_name: &str,
    container_spec: Option<&str>,
    is_registered_recipe: bool,
) -> Result<String, PodError> {
    if is_registered_recipe {
        return resolve_recipe_container(context, recipe_name, container_spec);
    }
    resolve_unmanaged_container(recipe_name, container_spec)
}

/// Validates and returns `recipe_name` as a direct container name when `recipe_name` is not registered in `pods.conf`.
///
/// # Errors
///
/// Returns [`PodError`] with [`ConfigErrorKind::RecipeNotFound`] when a `/<container>` suffix was
/// specified for an unregistered recipe, or [`CliErrorKind::InvalidTarget`] when `recipe_name`
/// is not a valid Podman container name.
fn resolve_unmanaged_container(
    recipe_name: &str,
    container_spec: Option<&str>,
) -> Result<String, PodError> {
    if container_spec.is_some() {
        return Err(PodError::from(ConfigError::without_line(
            ConfigErrorKind::RecipeNotFound,
            recipe_name,
        )));
    }
    if !string::is_valid_unmanaged_container_name(recipe_name) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidTarget,
            recipe_name,
        )));
    }
    Ok(String::from(recipe_name))
}

/// Verifies that `container_name` is running in Podman when `simulate` is `false`.
///
/// # Errors
///
/// Returns [`PodError`] with [`RecipeErrorKind::ContainerNotRunning`] when `is_registered_recipe`
/// is `true` and the container is not running, [`CliErrorKind::ContainerNotRunning`] when an
/// unmanaged container exists in Podman but is stopped, or [`CliErrorKind::TargetNotFound`] when
/// an unmanaged container does not exist.
fn verify_running_container<R, X>(
    container_name: &str,
    target: &str,
    is_registered_recipe: bool,
    simulate: bool,
    is_running: R,
    container_exists: X,
) -> Result<(), PodError>
where
    R: FnOnce(&str) -> bool,
    X: FnOnce(&str) -> bool,
{
    if simulate || is_running(container_name) {
        return Ok(());
    }
    if is_registered_recipe {
        return Err(PodError::from(RecipeError::without_line(
            RecipeErrorKind::ContainerNotRunning,
            container_name,
        )));
    }
    if container_exists(container_name) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::ContainerNotRunning,
            target,
        )));
    }
    Err(PodError::from(CliError::with_argument(
        CliErrorKind::TargetNotFound,
        target,
    )))
}

#[cfg(test)]
#[path = "shell_tests.rs"]
mod tests;
