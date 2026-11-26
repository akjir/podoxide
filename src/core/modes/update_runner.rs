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

//! Shared pod and container image-update orchestration with grouped summaries for `pods update` and `pods image update`.

use std::io::Write;

use crate::core::container::{self, Container};
use crate::core::context::Context;
use crate::core::log;
use crate::core::modes::targets;
use crate::core::pod::{self, Pod};
use crate::core::update::{
    ContainerUpdateResult, PullCache, PullRunner, SummaryKind, SummaryScope, UpdateAction,
    UpdateRunners, UpdateSummary,
};
use crate::error::{CliError, CliErrorKind, PodError};
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::PullOutcome;

/// Resolves targets from `context`, runs pod or container updates/pulls via `pull_image` with shared pull deduplication, and renders the grouped `Update Summary:` to `stdout`.
///
/// # Errors
///
/// Returns [`PodError`] if target resolution, recipe validation, or command spawning fails.
pub(crate) fn execute_update_with<W, E, P>(
    context: &Context,
    kind: SummaryKind,
    pull_image: &mut P,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
{
    let mut cache = PullCache::new();
    let mut no_container_inspect = |_: &str| None;
    let mut no_image_inspect = |_: &str| None;
    let mut runners = UpdateRunners {
        cache: &mut cache,
        pull_image,
        query_container_image: &mut no_container_inspect,
        query_image: &mut no_image_inspect,
    };
    execute_update_with_inspectors(context, kind, &mut runners, stdout, stderr)
}

/// Resolves targets from `context`, runs pod or container updates/pulls via `runners`, and renders the grouped `Update Summary:` to `stdout`.
///
/// # Errors
///
/// Returns [`PodError`] if target resolution, recipe validation, or command spawning fails.
pub(crate) fn execute_update_with_inspectors<W, E, P, C, I>(
    context: &Context,
    kind: SummaryKind,
    runners: &mut UpdateRunners<'_, P, C, I>,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
    C: FnMut(&str) -> Option<String>,
    I: FnMut(&str) -> Option<String>,
{
    let Some(first) = context.parameters.first() else {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    };
    if first.contains('/') {
        return execute_container_updates(context, kind, runners, stdout, stderr);
    }
    execute_pod_updates(context, kind, runners, stdout, stderr)
}

/// Resolves `<recipe>/<container>` targets, executes pulls or lifecycle updates, and writes the container summary.
fn execute_container_updates<W, E, P, C, I>(
    context: &Context,
    kind: SummaryKind,
    runners: &mut UpdateRunners<'_, P, C, I>,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
    C: FnMut(&str) -> Option<String>,
    I: FnMut(&str) -> Option<String>,
{
    if log::is_debug_enabled() {
        log::debug(&format!("Targets   - {}", context.parameters.join(" ")));
    }
    let specs = targets::collect_container_specs(context)?;
    let (recipes, resolved) = targets::resolve_container_targets(&context.config, &specs)?;
    let mut summary = UpdateSummary::new(kind, SummaryScope::Container);
    for (index, &(recipe_name, container_index)) in resolved.iter().enumerate() {
        let Some(recipe) = recipes.get(recipe_name) else {
            continue;
        };
        let Some(container) = recipe.containers.get(container_index) else {
            continue;
        };
        if index > 0 && kind == SummaryKind::LifecycleUpdate {
            log::write_print(stdout, "");
        }
        let result = run_single_container_update(
            kind,
            container,
            &recipe.pod,
            context.config.simulate,
            runners,
            stdout,
            stderr,
        )?;
        summary.record_container(result.action, &result.record);
    }
    summary.write_to(stdout);
    Ok(summary_outcome(&summary))
}

/// Expands `@<group>` and `<recipe>` targets, executes pod pulls or lifecycle updates, and writes the pod summary.
fn execute_pod_updates<W, E, P, C, I>(
    context: &Context,
    kind: SummaryKind,
    runners: &mut UpdateRunners<'_, P, C, I>,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
    C: FnMut(&str) -> Option<String>,
    I: FnMut(&str) -> Option<String>,
{
    let expanded = targets::expand_pod_targets(context)?;
    let recipes = targets::load_target_recipes(&context.config, &expanded)?;
    let mut summary = UpdateSummary::new(kind, SummaryScope::Pod);
    for (index, recipe) in recipes.iter().enumerate() {
        if index > 0 {
            log::write_print(stdout, "");
        }
        let result = match kind {
            SummaryKind::ImagePull => pod::pull_with_cache(
                recipe,
                context.config.simulate,
                runners.cache,
                runners.pull_image,
                stdout,
                stderr,
            )?,
            SummaryKind::LifecycleUpdate => {
                pod::update_with_cache(recipe, context.config.simulate, runners, stdout, stderr)?
            }
        };
        summary.record_pod(&result);
    }
    summary.write_to(stdout);
    Ok(summary_outcome(&summary))
}

/// Executes a single container pull or lifecycle update and returns its [`ContainerUpdateResult`].
fn run_single_container_update<W, E, P, C, I>(
    kind: SummaryKind,
    container: &Container,
    pod: &Pod,
    simulate: bool,
    runners: &mut UpdateRunners<'_, P, C, I>,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ContainerUpdateResult, PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
    C: FnMut(&str) -> Option<String>,
    I: FnMut(&str) -> Option<String>,
{
    match kind {
        SummaryKind::ImagePull => {
            let mut pull_runner = PullRunner {
                cache: runners.cache,
                pull_image: runners.pull_image,
            };
            let record = container::pull_with_cache(
                container,
                pod,
                "",
                simulate,
                &mut pull_runner,
                stdout,
                stderr,
            )?;
            Ok(ContainerUpdateResult {
                action: UpdateAction::UpToDate,
                record,
            })
        }
        SummaryKind::LifecycleUpdate => {
            container::update_with_cache(container, pod, simulate, runners, stdout, stderr)
        }
    }
}

/// Converts [`UpdateSummary::has_failures`] into an [`ExecOutcome`].
#[must_use]
const fn summary_outcome(summary: &UpdateSummary) -> ExecOutcome {
    if summary.has_failures() {
        ExecOutcome::Failed
    } else {
        ExecOutcome::Succeeded
    }
}

#[cfg(test)]
#[path = "update_runner_tests.rs"]
mod tests;
