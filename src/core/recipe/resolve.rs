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

//! Container target resolution (`1..=N`, `*suffix`, exact name, or suffixed name) against a [`Recipe`].

use std::borrow::Cow;

use crate::core::container::{self, Container};
use crate::error::{RecipeError, RecipeErrorKind};
use crate::utilities::string;
use super::Recipe;

/// Matched 0-based container index, reference to [`Container`], and resolved container name.
pub(super) type ResolvedContainer<'a> = (usize, &'a Container, Cow<'a, str>);

/// Resolves `container_spec` (`1..=N`, `*suffix`, or name) against `recipe.containers`, returning the 0-based index, matched [`Container`], and its resolved name.
pub(super) fn resolve_container_entry<'a>(
    recipe: &'a Recipe,
    container_spec: &str,
) -> Result<ResolvedContainer<'a>, RecipeError> {
    if !recipe.has_pod || !string::is_valid_pod_or_container_name(&recipe.pod.id) {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::ContainerNotFound,
            recipe.pod.line,
            container_spec,
        ));
    }
    if !string::is_valid_text(container_spec) {
        return Err(RecipeError::without_line(
            RecipeErrorKind::ContainerNotFound,
            container_spec,
        ));
    }
    let trimmed_spec = container_spec.trim();
    if let Some(index) =
        string::parse_ascii_u32(trimmed_spec).and_then(|value| usize::try_from(value).ok())
        && let Some(found) = resolve_by_index(recipe, index)?
    {
        return Ok(found);
    }
    let normalized = string::normalize_name(trimmed_spec);
    if let Some(suffix) = normalized.strip_prefix('*') {
        return resolve_by_suffix(recipe, suffix, container_spec);
    }
    let prefixed = format!("{}-{normalized}", recipe.pod.id);
    if let Some(found) = find_container_by_exact_or_prefixed(
        &recipe.containers,
        &recipe.pod.id,
        &normalized,
        &prefixed,
    ) {
        return Ok(found);
    }
    Err(RecipeError::without_line(
        RecipeErrorKind::ContainerNotFound,
        container_spec,
    ))
}

/// Searches `containers` in a single pass for an exact match on `exact` or a fallback match on `prefixed`.
#[must_use]
fn find_container_by_exact_or_prefixed<'a>(
    containers: &'a [Container],
    pod_id: &str,
    exact: &str,
    prefixed: &str,
) -> Option<ResolvedContainer<'a>> {
    let mut fallback = None;
    for (index, container) in containers.iter().enumerate() {
        let Ok(resolved) = resolve_stored_container_name(container, pod_id, index + 1) else {
            continue;
        };
        if resolved.as_ref() == exact {
            return Some((index, container, resolved));
        }
        if fallback.is_none() && resolved.as_ref() == prefixed {
            fallback = Some((index, container, resolved));
        }
    }
    fallback
}

/// Resolves a 1-based numeric container index (`1..=N`) against `recipe.containers`, returning `Ok(None)` when out of range.
fn resolve_by_index<'a>(
    recipe: &'a Recipe,
    index: usize,
) -> Result<Option<ResolvedContainer<'a>>, RecipeError> {
    let Some(zero_based) = index.checked_sub(1) else {
        return Ok(None);
    };
    let Some(container) = recipe.containers.get(zero_based) else {
        return Ok(None);
    };
    let resolved = resolve_stored_container_name(container, &recipe.pod.id, index)?;
    Ok(Some((zero_based, container, resolved)))
}

/// Resolves a `*suffix` container target after verifying that `suffix` is a valid non-empty suffix.
fn resolve_by_suffix<'a>(
    recipe: &'a Recipe,
    suffix: &str,
    raw_spec: &str,
) -> Result<ResolvedContainer<'a>, RecipeError> {
    if !string::is_valid_container_suffix(suffix) {
        return Err(RecipeError::without_line(
            RecipeErrorKind::ContainerNotFound,
            raw_spec,
        ));
    }
    let target = format!("{}-{suffix}", recipe.pod.id);
    if let Some(found) =
        find_container_by_exact_or_prefixed(&recipe.containers, &recipe.pod.id, &target, &target)
    {
        return Ok(found);
    }
    Err(RecipeError::without_line(
        RecipeErrorKind::ContainerNotFound,
        raw_spec,
    ))
}

/// Returns the resolved container name without heap allocation when `container.name` is already valid.
fn resolve_stored_container_name<'a>(
    container: &'a Container,
    pod_id: &str,
    index: usize,
) -> Result<Cow<'a, str>, RecipeError> {
    if !container.name.is_empty() && string::is_valid_pod_or_container_name(&container.name) {
        return Ok(Cow::Borrowed(container.name.as_str()));
    }
    container::resolve_container_name_value(&container.name, pod_id, index, container.line)
        .map(Cow::Owned)
}
