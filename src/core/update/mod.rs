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

//! Update progress formatting, batch pull caching, indented output adaptation, and grouped summary reporting.

mod summary;
mod writer;

use std::collections::BTreeMap;

use crate::io::exec::podman::PullOutcome;
pub use self::summary::{MAX_SUMMARY_ENTRIES, UpdateSummary};
pub use self::writer::IndentedWriter;

/// Two-space indentation prefix used for container pulls and reload output under a pod header.
pub const POD_INDENT: &str = "  ";
/// Maximum number of unique image references retained in a batch [`PullCache`].
const MAX_PULL_CACHE_ENTRIES: usize = 1024;

/// Recorded image pull outcome for a single container.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerPullRecord {
    pub container_name: String,
    pub image: String,
    pub outcome: PullOutcome,
}

/// Lifecycle outcome and image pull record produced by updating a targeted container.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContainerUpdateResult {
    pub action: UpdateAction,
    pub record: ContainerPullRecord,
}

/// Aggregated lifecycle outcome and container pull records produced by updating or pulling a pod.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PodUpdateResult {
    pub action: UpdateAction,
    pub containers: Vec<ContainerPullRecord>,
    pub pod_id: String,
    pub pod_name: String,
}

/// Whether a summary covers full lifecycle updates (`pods update`) or image pulls only (`pods image update`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SummaryKind {
    ImagePull,
    LifecycleUpdate,
}

/// Granularity of top-level targets accumulated in an [`UpdateSummary`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SummaryScope {
    Container,
    Pod,
}

/// Lifecycle outcome for a single updated pod or container target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateAction {
    Failed,
    Reloaded,
    Simulated,
    UpToDate,
}

/// Batch-scoped cache of live pull outcomes and local image IDs keyed by image reference.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct PullCache {
    image_ids: BTreeMap<String, Option<String>>,
    outcomes: BTreeMap<String, PullOutcome>,
}

impl PullCache {
    /// Creates an empty [`PullCache`].
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Returns the cached live [`PullOutcome`] for `image_reference`, if already pulled in this batch.
    #[inline]
    #[must_use]
    pub(crate) fn get_outcome(&self, image_reference: &str) -> Option<PullOutcome> {
        self.outcomes.get(image_reference).copied()
    }

    /// Queries or returns the cached local image ID for `image_reference` via `query_image`.
    pub(crate) fn image_id<F>(
        &mut self,
        image_reference: &str,
        query_image: &mut F,
    ) -> Option<String>
    where
        F: FnMut(&str) -> Option<String>,
    {
        if let Some(cached) = self.image_ids.get(image_reference) {
            return cached.clone();
        }
        let queried = query_image(image_reference);
        if self.image_ids.len() < MAX_PULL_CACHE_ENTRIES {
            self.image_ids
                .insert(image_reference.to_owned(), queried.clone());
        }
        queried
    }

    /// Records a live [`PullOutcome`] for `image_reference`, skipping [`PullOutcome::Simulated`].
    pub(crate) fn record_outcome(&mut self, image_reference: &str, outcome: PullOutcome) {
        if outcome == PullOutcome::Simulated || self.outcomes.len() >= MAX_PULL_CACHE_ENTRIES {
            return;
        }
        self.outcomes.insert(image_reference.to_owned(), outcome);
    }
}

/// Injectable batch pull cache and `podman pull` callback for image pull orchestration.
pub(crate) struct PullRunner<'a, P> {
    pub(crate) cache: &'a mut PullCache,
    pub(crate) pull_image: &'a mut P,
}

/// Injectable batch pull cache and Podman image-ID lookup callbacks for update orchestration.
pub(crate) struct UpdateRunners<'a, P, C, I> {
    pub(crate) cache: &'a mut PullCache,
    pub(crate) pull_image: &'a mut P,
    pub(crate) query_container_image: &'a mut C,
    pub(crate) query_image: &'a mut I,
}

/// Formats the progress prefix for pulling `image_reference` for `container_name`.
#[must_use]
pub fn format_pull_prefix(
    container_name: &str,
    image_reference: &str,
    indent: &str,
    simulate: bool,
) -> String {
    if simulate {
        format!("{indent}Trying to pull {image_reference} ({container_name}): ")
    } else {
        format!("{indent}Trying to pull {image_reference} ({container_name})... ")
    }
}

/// Returns the single-line completion status suffix for a cached [`PullOutcome`].
#[inline]
#[must_use]
pub(crate) const fn pull_outcome_suffix(outcome: PullOutcome) -> &'static str {
    match outcome {
        PullOutcome::Failed => "Failed.",
        PullOutcome::Simulated => "Simulated.",
        PullOutcome::UpToDate => "Up to date.",
        PullOutcome::Updated => "Updated.",
    }
}

#[cfg(test)]
mod tests;
