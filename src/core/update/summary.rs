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

//! Grouped outcome summary accumulation and rendering for pod and container updates.

use std::fmt::Write as _;
use std::io::Write;

use crate::core::log;
use crate::io::exec::podman::PullOutcome;
use super::{ContainerPullRecord, PodUpdateResult, SummaryKind, SummaryScope, UpdateAction};

/// Maximum number of formatted entries retained per summary bucket.
pub const MAX_SUMMARY_ENTRIES: usize = 1024;
/// Estimated bytes per formatted `<container> (<image>)` segment inside a pod summary line.
const CONTAINER_DETAIL_ESTIMATE_BYTES: usize = 48;

/// Grouped outcome summary collected across pod or container update operations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateSummary {
    changed: SummaryBucket,
    failed: SummaryBucket,
    images_checked: usize,
    images_failed: usize,
    images_simulated: usize,
    images_up_to_date: usize,
    images_updated: usize,
    kind: SummaryKind,
    scope: SummaryScope,
    simulated: SummaryBucket,
    targets_checked: usize,
    up_to_date: SummaryBucket,
}

impl UpdateSummary {
    /// Creates an empty [`UpdateSummary`] configured for `kind` and `scope`.
    #[must_use]
    pub fn new(kind: SummaryKind, scope: SummaryScope) -> Self {
        Self {
            changed: SummaryBucket::default(),
            failed: SummaryBucket::default(),
            images_checked: 0,
            images_failed: 0,
            images_simulated: 0,
            images_up_to_date: 0,
            images_updated: 0,
            kind,
            scope,
            simulated: SummaryBucket::default(),
            targets_checked: 0,
            up_to_date: SummaryBucket::default(),
        }
    }

    /// Evaluates whether any target or image pull failed during this update batch.
    #[inline]
    #[must_use]
    pub const fn has_failures(&self) -> bool {
        self.failed.count > 0 || self.images_failed > 0
    }

    /// Records the outcome of a single container-targeted update or image pull.
    pub fn record_container(&mut self, action: UpdateAction, record: &ContainerPullRecord) {
        self.targets_checked = self.targets_checked.saturating_add(1);
        self.record_pull_count(record.outcome);
        match self.kind {
            SummaryKind::ImagePull => self.record_container_pull_bucket(record),
            SummaryKind::LifecycleUpdate => self.record_container_lifecycle_bucket(action, record),
        }
    }

    /// Records the outcome of a pod-targeted update or image pull.
    pub fn record_pod(&mut self, result: &PodUpdateResult) {
        self.targets_checked = self.targets_checked.saturating_add(1);
        for record in &result.containers {
            self.record_pull_count(record.outcome);
        }
        let label = format_pod_label(&result.pod_id, &result.pod_name);
        match self.kind {
            SummaryKind::ImagePull => self.record_pod_pull_buckets(&label, &result.containers),
            SummaryKind::LifecycleUpdate => {
                self.record_pod_lifecycle_bucket(result.action, &label, &result.containers);
            }
        }
    }

    /// Writes the grouped `Update Summary:` section and totals footer to `stdout`.
    pub fn write_to<W: Write>(&self, stdout: &mut W) {
        if self.targets_checked == 0 {
            return;
        }
        let changed_title = match self.kind {
            SummaryKind::ImagePull => "Updated",
            SummaryKind::LifecycleUpdate => "Reloaded",
        };
        let buckets = [
            (changed_title, &self.changed),
            ("Failed", &self.failed),
            ("Up to date", &self.up_to_date),
            ("Simulated", &self.simulated),
        ];
        log::write_print(stdout, "");
        log::write_print(stdout, "Update Summary:");
        let mut wrote_bucket = false;
        for (title, bucket) in buckets {
            if write_bucket(title, bucket, wrote_bucket, stdout) {
                wrote_bucket = true;
            }
        }
        log::write_print(stdout, "");
        log::write_print(stdout, &self.format_footer());
    }

    /// Formats the concluding checked or simulated totals line for this summary.
    #[must_use]
    fn format_footer(&self) -> String {
        if self.simulated.count > 0 {
            return self.format_simulated_footer();
        }
        match (self.kind, self.scope) {
            (SummaryKind::ImagePull, SummaryScope::Container) => format!(
                "Checked {} {}: {} updated, {} up to date, {} failed.",
                self.images_checked,
                pluralize(self.images_checked, "image", "images"),
                self.images_updated,
                self.images_up_to_date,
                self.images_failed,
            ),
            (SummaryKind::ImagePull, SummaryScope::Pod) => format!(
                "Checked {} {} ({} {}): {} updated, {} up to date, {} failed.",
                self.targets_checked,
                pluralize(self.targets_checked, "pod", "pods"),
                self.images_checked,
                pluralize(self.images_checked, "image", "images"),
                self.images_updated,
                self.images_up_to_date,
                self.images_failed,
            ),
            (SummaryKind::LifecycleUpdate, SummaryScope::Container) => format!(
                "Checked {} {}: {} reloaded, {} up to date, {} failed.",
                self.targets_checked,
                pluralize(self.targets_checked, "container", "containers"),
                self.changed.count,
                self.up_to_date.count,
                self.failed.count,
            ),
            (SummaryKind::LifecycleUpdate, SummaryScope::Pod) => format!(
                "Checked {} {} ({} {}): {} reloaded, {} up to date, {} failed.",
                self.targets_checked,
                pluralize(self.targets_checked, "pod", "pods"),
                self.images_checked,
                pluralize(self.images_checked, "container", "containers"),
                self.changed.count,
                self.up_to_date.count,
                self.failed.count,
            ),
        }
    }

    /// Formats the concluding totals line when `--simulate` mode is active.
    #[must_use]
    fn format_simulated_footer(&self) -> String {
        match (self.kind, self.scope) {
            (SummaryKind::ImagePull, SummaryScope::Container) => format!(
                "Simulated {} {}.",
                self.images_simulated,
                pluralize(self.images_simulated, "image", "images"),
            ),
            (SummaryKind::ImagePull, SummaryScope::Pod) => format!(
                "Simulated {} {} ({} {}).",
                self.targets_checked,
                pluralize(self.targets_checked, "pod", "pods"),
                self.images_simulated,
                pluralize(self.images_simulated, "image", "images"),
            ),
            (SummaryKind::LifecycleUpdate, SummaryScope::Container) => format!(
                "Simulated {} {}.",
                self.targets_checked,
                pluralize(self.targets_checked, "container", "containers"),
            ),
            (SummaryKind::LifecycleUpdate, SummaryScope::Pod) => format!(
                "Simulated {} {} ({} {}).",
                self.targets_checked,
                pluralize(self.targets_checked, "pod", "pods"),
                self.images_checked,
                pluralize(self.images_checked, "container", "containers"),
            ),
        }
    }

    /// Routes a container lifecycle update record into its corresponding outcome bucket.
    fn record_container_lifecycle_bucket(
        &mut self,
        action: UpdateAction,
        record: &ContainerPullRecord,
    ) {
        match action {
            UpdateAction::Failed => self
                .failed
                .push(format!("{} ({})", record.container_name, record.image)),
            UpdateAction::Reloaded => self
                .changed
                .push(format!("{} ({})", record.container_name, record.image)),
            UpdateAction::Simulated => self
                .simulated
                .push(format!("{} ({})", record.container_name, record.image)),
            UpdateAction::UpToDate => self.up_to_date.push(record.container_name.clone()),
        }
    }

    /// Routes a container image pull record into its corresponding outcome bucket.
    fn record_container_pull_bucket(&mut self, record: &ContainerPullRecord) {
        match record.outcome {
            PullOutcome::Failed => self
                .failed
                .push(format!("{} ({})", record.container_name, record.image)),
            PullOutcome::Simulated => self
                .simulated
                .push(format!("{} ({})", record.container_name, record.image)),
            PullOutcome::UpToDate => self.up_to_date.push(record.container_name.clone()),
            PullOutcome::Updated => self
                .changed
                .push(format!("{} ({})", record.container_name, record.image)),
        }
    }

    /// Routes a pod lifecycle update result into its single matching outcome bucket.
    fn record_pod_lifecycle_bucket(
        &mut self,
        action: UpdateAction,
        label: &str,
        containers: &[ContainerPullRecord],
    ) {
        match action {
            UpdateAction::Failed => self.failed.push(format_pod_detail_entry(
                label,
                containers,
                PullOutcome::Failed,
            )),
            UpdateAction::Reloaded => self.changed.push(format_pod_detail_entry(
                label,
                containers,
                PullOutcome::Updated,
            )),
            UpdateAction::Simulated => self.simulated.push(label.to_owned()),
            UpdateAction::UpToDate => self.up_to_date.push(label.to_owned()),
        }
    }

    /// Routes a pod image pull result into its matching pull buckets (`Updated`, `Failed`, `Up to date`, or `Simulated`).
    fn record_pod_pull_buckets(&mut self, label: &str, containers: &[ContainerPullRecord]) {
        let mut any_simulated = false;
        let mut any_updated = false;
        let mut any_failed = false;
        for record in containers {
            match record.outcome {
                PullOutcome::Failed => any_failed = true,
                PullOutcome::Simulated => any_simulated = true,
                PullOutcome::UpToDate => {}
                PullOutcome::Updated => any_updated = true,
            }
        }
        if any_simulated {
            self.simulated.push(label.to_owned());
            return;
        }
        if any_updated {
            self.changed.push(format_pod_detail_entry(
                label,
                containers,
                PullOutcome::Updated,
            ));
        }
        if any_failed {
            self.failed.push(format_pod_detail_entry(
                label,
                containers,
                PullOutcome::Failed,
            ));
        }
        if !any_updated && !any_failed {
            self.up_to_date.push(label.to_owned());
        }
    }

    /// Increments the total and per-outcome image pull counters for `outcome`.
    fn record_pull_count(&mut self, outcome: PullOutcome) {
        self.images_checked = self.images_checked.saturating_add(1);
        match outcome {
            PullOutcome::Failed => self.images_failed = self.images_failed.saturating_add(1),
            PullOutcome::Simulated => {
                self.images_simulated = self.images_simulated.saturating_add(1);
            }
            PullOutcome::UpToDate => {
                self.images_up_to_date = self.images_up_to_date.saturating_add(1);
            }
            PullOutcome::Updated => self.images_updated = self.images_updated.saturating_add(1),
        }
    }
}

/// Bounded collection of formatted summary lines paired with the exact recorded item count.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct SummaryBucket {
    count: usize,
    items: Vec<String>,
}

impl SummaryBucket {
    /// Increments the recorded count and stores `item` when below [`MAX_SUMMARY_ENTRIES`].
    fn push(&mut self, item: String) {
        self.count = self.count.saturating_add(1);
        if self.items.len() < MAX_SUMMARY_ENTRIES {
            self.items.push(item);
        }
    }
}

/// Formats `<pod_label>: <container> (<image>), ...` for all records matching `filter`.
#[must_use]
pub(super) fn format_pod_detail_entry(
    pod_label: &str,
    containers: &[ContainerPullRecord],
    filter: PullOutcome,
) -> String {
    let estimated = pod_label
        .len()
        .saturating_add(
            containers
                .len()
                .saturating_mul(CONTAINER_DETAIL_ESTIMATE_BYTES),
        )
        .min(log::MAX_LOG_MESSAGE_BYTES);
    let mut line = String::with_capacity(estimated);
    line.push_str(pod_label);
    let mut matched = false;
    for record in containers {
        if record.outcome != filter {
            continue;
        }
        if matched {
            line.push_str(", ");
        } else {
            line.push_str(": ");
            matched = true;
        }
        let _ = write!(line, "{} ({})", record.container_name, record.image);
    }
    line
}

/// Formats `<pod_id> (<pod_name>)` when `pod_name` is non-empty and differs from `pod_id`, or `<pod_id>` otherwise.
#[must_use]
pub(super) fn format_pod_label(pod_id: &str, pod_name: &str) -> String {
    let trimmed = pod_name.trim();
    if trimmed.is_empty() || trimmed == pod_id {
        pod_id.to_owned()
    } else {
        format!("{pod_id} ({trimmed})")
    }
}

/// Returns `singular` when `count == 1`, or `plural` otherwise.
#[must_use]
const fn pluralize(count: usize, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 { singular } else { plural }
}

/// Writes a non-empty summary `bucket` under `title` to `stdout`, returning `true` when output was emitted.
fn write_bucket<W: Write>(
    title: &str,
    bucket: &SummaryBucket,
    prepend_blank: bool,
    stdout: &mut W,
) -> bool {
    if bucket.count == 0 {
        return false;
    }
    if prepend_blank {
        log::write_print(stdout, "");
    }
    log::write_print(stdout, &format!("  {title} ({}):", bucket.count));
    for item in &bucket.items {
        log::write_print(stdout, &format!("    - {item}"));
    }
    true
}
