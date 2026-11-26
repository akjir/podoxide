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

//! Container image reference resolution, cached `podman pull`, and stale-container update detection.

use std::borrow::Cow;
use std::io::{self, Write};

use crate::core::log;
use crate::core::pod::Pod;
use crate::core::update::{
    self, ContainerPullRecord, ContainerUpdateResult, PullCache, PullRunner, UpdateAction,
    UpdateRunners,
};
use crate::error::PodError;
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::{self, PullOutcome};
use super::{Container, lifecycle, validation};

/// Pulls the latest image for `container` in Podman (`podman pull`) and returns its [`ContainerPullRecord`].
///
/// # Errors
///
/// Returns [`PodError`] if Podman execution fails.
pub fn pull(
    container: &Container,
    pod: &Pod,
    simulate: bool,
) -> Result<ContainerPullRecord, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    pull_with(container, pod, "", simulate, &mut stdout, &mut stderr)
}

/// Pulls the latest image for `container` in Podman (`podman pull`) with line `indent`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if container/pod validation or Podman execution fails.
pub fn pull_with<W: Write, E: Write>(
    container: &Container,
    pod: &Pod,
    indent: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ContainerPullRecord, PodError> {
    let mut cache = PullCache::new();
    let mut pull_image = podman::pull_image;
    let mut runner = PullRunner {
        cache: &mut cache,
        pull_image: &mut pull_image,
    };
    pull_with_cache(
        container,
        pod,
        indent,
        simulate,
        &mut runner,
        stdout,
        stderr,
    )
}

/// Resolves the fully qualified `<registry>/<image>` reference for `container` inside `pod`.
#[must_use]
pub fn resolve_image_reference<'a>(container: &'a Container, pod: &'a Pod) -> Cow<'a, str> {
    let registry = if container.registry.is_empty() {
        pod.registry.as_str()
    } else {
        container.registry.as_str()
    };
    if registry.is_empty() {
        return Cow::Borrowed(container.image.as_str());
    }
    Cow::Owned(format!("{registry}/{}", container.image))
}

/// Pulls the latest image for `container` and reloads the container inside `pod` when the image was updated or stale (`pull` + conditional `reload`).
///
/// # Errors
///
/// Returns [`PodError`] if pulling the image or reloading the container fails.
pub fn update(
    container: &Container,
    pod: &Pod,
    simulate: bool,
) -> Result<ContainerUpdateResult, PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    update_with(container, pod, simulate, &mut stdout, &mut stderr)
}

/// Pulls the latest image for `container` and reloads the container inside `pod` when the image was updated, stale, or simulated, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if container/pod validation or Podman execution fails.
pub fn update_with<W: Write, E: Write>(
    container: &Container,
    pod: &Pod,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ContainerUpdateResult, PodError> {
    let mut cache = PullCache::new();
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
    let mut pull_image = podman::pull_image;
    let mut runners = UpdateRunners {
        cache: &mut cache,
        pull_image: &mut pull_image,
        query_container_image: &mut query_container_image,
        query_image: &mut query_image,
    };
    update_with_cache(container, pod, simulate, &mut runners, stdout, stderr)
}

/// Upgrades [`PullOutcome::UpToDate`] to [`PullOutcome::Updated`] when `container_name` runs an older image ID than `image_reference`.
#[must_use]
pub(crate) fn detect_stale_container_image<C, I>(
    container_name: &str,
    image_reference: &str,
    outcome: PullOutcome,
    cache: &mut PullCache,
    query_container_image: &mut C,
    query_image: &mut I,
) -> PullOutcome
where
    C: FnMut(&str) -> Option<String>,
    I: FnMut(&str) -> Option<String>,
{
    if outcome != PullOutcome::UpToDate {
        return outcome;
    }
    let Some(container_image_id) = query_container_image(container_name) else {
        return PullOutcome::UpToDate;
    };
    let Some(target_image_id) = cache.image_id(image_reference, query_image) else {
        return PullOutcome::UpToDate;
    };
    if container_image_id != target_image_id {
        return PullOutcome::Updated;
    }
    PullOutcome::UpToDate
}

/// Pulls the latest image for `container` using `runner` to avoid duplicate pulls per unique image reference.
pub(crate) fn pull_with_cache<W, E, P>(
    container: &Container,
    pod: &Pod,
    indent: &str,
    simulate: bool,
    runner: &mut PullRunner<'_, P>,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ContainerPullRecord, PodError>
where
    W: Write,
    E: Write,
    P: FnMut(&str, &str, bool, &mut W, &mut E) -> Result<PullOutcome, PodError>,
{
    validation::validate_container_name(container)?;
    container.validate()?;
    validation::validate_optional_registry(&pod.registry, pod.line)?;
    let image_reference = resolve_image_reference(container, pod);
    let prefix = update::format_pull_prefix(&container.name, &image_reference, indent, simulate);
    let outcome = if let Some(cached_outcome) = runner.cache.get_outcome(&image_reference) {
        let suffix = update::pull_outcome_suffix(cached_outcome);
        log::write_print(stdout, &format!("{prefix}{suffix}"));
        cached_outcome
    } else {
        let fresh = (runner.pull_image)(&image_reference, &prefix, simulate, stdout, stderr)?;
        runner.cache.record_outcome(&image_reference, fresh);
        fresh
    };
    Ok(ContainerPullRecord {
        container_name: container.name.clone(),
        image: image_reference.into_owned(),
        outcome,
    })
}

/// Pulls the latest image for `container` via `runners`, detects stale container image IDs, and reloads when updated.
pub(crate) fn update_with_cache<W, E, P, C, I>(
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
    container.validate_create(pod)?;
    let mut pull_runner = PullRunner {
        cache: runners.cache,
        pull_image: runners.pull_image,
    };
    let mut record = pull_with_cache(
        container,
        pod,
        "",
        simulate,
        &mut pull_runner,
        stdout,
        stderr,
    )?;
    let previous_outcome = record.outcome;
    record.outcome = detect_stale_container_image(
        &container.name,
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
                "Container '{}' is running an older image; reloading.",
                container.name
            ),
        );
    }
    let action = apply_update_action(container, pod, record.outcome, simulate, stdout, stderr)?;
    Ok(ContainerUpdateResult { action, record })
}

/// Applies the post-pull lifecycle action (`reload`, up-to-date message, or failure notice) for `container`.
fn apply_update_action<W: Write, E: Write>(
    container: &Container,
    pod: &Pod,
    outcome: PullOutcome,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<UpdateAction, PodError> {
    match outcome {
        PullOutcome::Failed => {
            log::write_print(
                stdout,
                &format!("Container '{}' update failed.", container.name),
            );
            Ok(UpdateAction::Failed)
        }
        PullOutcome::Simulated => {
            let _ = lifecycle::reload_with(container, pod, true, stdout, stderr)?;
            Ok(UpdateAction::Simulated)
        }
        PullOutcome::UpToDate => {
            log::write_print(
                stdout,
                &format!("Container '{}' is up to date.", container.name),
            );
            Ok(UpdateAction::UpToDate)
        }
        PullOutcome::Updated => {
            let reload_outcome = lifecycle::reload_with(container, pod, simulate, stdout, stderr)?;
            if reload_outcome == ExecOutcome::Failed {
                log::write_print(
                    stdout,
                    &format!("Container '{}' update failed.", container.name),
                );
                return Ok(UpdateAction::Failed);
            }
            Ok(UpdateAction::Reloaded)
        }
    }
}
