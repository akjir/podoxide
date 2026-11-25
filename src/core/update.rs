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

//! Update progress formatting, indented output adaptation, and grouped summary reporting.

use std::fmt::Write as _;
use std::io::{self, Write};

use crate::core::log;
use crate::io::exec::podman::PullOutcome;

/// Maximum number of formatted entries retained per summary bucket.
pub const MAX_SUMMARY_ENTRIES: usize = 1024;
/// Two-space indentation prefix used for container pulls and reload output under a pod header.
pub const POD_INDENT: &str = "  ";

/// Estimated bytes per formatted `<container> (<image>)` segment inside a pod summary line.
const CONTAINER_DETAIL_ESTIMATE_BYTES: usize = 48;

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

/// Writer adapter that prepends a fixed indentation prefix to every non-empty output line.
pub struct IndentedWriter<'a, W: Write> {
    at_line_start: bool,
    indent: &'a str,
    inner: &'a mut W,
}

impl<'a, W: Write> IndentedWriter<'a, W> {
    /// Creates an [`IndentedWriter`] that prefixes non-empty lines written to `inner` with `indent`.
    #[must_use]
    pub const fn new(inner: &'a mut W, indent: &'a str) -> Self {
        Self {
            at_line_start: true,
            indent,
            inner,
        }
    }
}

impl<W: Write> Write for IndentedWriter<'_, W> {
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for segment in buf.split_inclusive(|&byte| byte == b'\n') {
            if self.at_line_start && segment != b"\n" {
                self.inner.write_all(self.indent.as_bytes())?;
            }
            self.inner.write_all(segment)?;
            self.at_line_start = segment.ends_with(b"\n");
        }
        Ok(buf.len())
    }
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

/// Formats `<pod_label>: <container> (<image>), ...` for all records matching `filter`.
#[must_use]
fn format_pod_detail_entry(
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
fn format_pod_label(pod_id: &str, pod_name: &str) -> String {
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

#[cfg(test)]
mod tests {
    use std::io::Write;

    use crate::io::exec::podman::PullOutcome;
    use super::{
        ContainerPullRecord, IndentedWriter, MAX_SUMMARY_ENTRIES, POD_INDENT, PodUpdateResult,
        SummaryKind, SummaryScope, UpdateAction, UpdateSummary, format_pod_detail_entry,
        format_pod_label, format_pull_prefix,
    };

    fn pod_result(
        pod_id: &str,
        pod_name: &str,
        action: UpdateAction,
        containers: Vec<ContainerPullRecord>,
    ) -> PodUpdateResult {
        PodUpdateResult {
            action,
            containers,
            pod_id: String::from(pod_id),
            pod_name: String::from(pod_name),
        }
    }

    fn pull_record(container_name: &str, image: &str, outcome: PullOutcome) -> ContainerPullRecord {
        ContainerPullRecord {
            container_name: String::from(container_name),
            image: String::from(image),
            outcome,
        }
    }

    #[test]
    fn format_helpers_and_indented_writer_handle_edge_cases() {
        assert_eq!(
            format_pull_prefix("webapp-db", "docker.io/postgres:16", POD_INDENT, false),
            "  Trying to pull docker.io/postgres:16 (webapp-db)... "
        );
        assert_eq!(
            format_pull_prefix("webapp-db", "docker.io/postgres:16", "", true),
            "Trying to pull docker.io/postgres:16 (webapp-db): "
        );
        assert_eq!(
            format_pod_label("example-stack", "Example Web Stack"),
            "example-stack (Example Web Stack)"
        );
        assert_eq!(format_pod_label("cache", ""), "cache");
        assert_eq!(format_pod_label("cache", "cache"), "cache");

        assert_eq!(
            format_pod_detail_entry("cache (Redis Cache)", &[], PullOutcome::Updated),
            "cache (Redis Cache)"
        );
        let multi_detail = format_pod_detail_entry(
            "stack (Web Stack)",
            &[
                pull_record("stack-db", "docker.io/db:1", PullOutcome::Updated),
                pull_record("stack-app", "docker.io/app:2", PullOutcome::Updated),
            ],
            PullOutcome::Updated,
        );
        assert_eq!(
            multi_detail,
            "stack (Web Stack): stack-db (docker.io/db:1), stack-app (docker.io/app:2)"
        );

        let mut buffer = Vec::new();
        {
            let mut writer = IndentedWriter::new(&mut buffer, POD_INDENT);
            assert_eq!(writer.write(&[]).unwrap(), 0);
            writer.write_all(b"first line\n\nsecond ").unwrap();
            writer.write_all(b"part\nthird\n").unwrap();
            writer.flush().unwrap();
        }
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "  first line\n\n  second part\n  third\n"
        );
    }

    #[test]
    fn update_summary_renders_pod_and_container_outcomes_for_update_and_image_pull() {
        let mut empty_out = Vec::new();
        UpdateSummary::new(SummaryKind::LifecycleUpdate, SummaryScope::Pod)
            .write_to(&mut empty_out);
        assert!(empty_out.is_empty());

        let db_same = pull_record(
            "example-stack-db",
            "docker.io/example/db-server:latest",
            PullOutcome::UpToDate,
        );
        let app_updated = pull_record(
            "example-stack-app",
            "docker.io/example/example-app:latest",
            PullOutcome::Updated,
        );
        let redis_same = pull_record(
            "cache-redis",
            "docker.io/library/redis:7-alpine",
            PullOutcome::UpToDate,
        );
        let nginx_failed = pull_record(
            "proxy-nginx",
            "docker.io/library/nginx:stable-alpine",
            PullOutcome::Failed,
        );

        let mut pod_summary = UpdateSummary::new(SummaryKind::LifecycleUpdate, SummaryScope::Pod);
        pod_summary.record_pod(&pod_result(
            "example-stack",
            "Example Web Stack",
            UpdateAction::Reloaded,
            vec![db_same.clone(), app_updated.clone()],
        ));
        pod_summary.record_pod(&pod_result(
            "cache",
            "Redis Cache",
            UpdateAction::UpToDate,
            vec![redis_same.clone()],
        ));
        pod_summary.record_pod(&pod_result(
            "proxy",
            "Reverse Proxy",
            UpdateAction::Failed,
            vec![nginx_failed.clone()],
        ));
        let mut pod_out = Vec::new();
        pod_summary.write_to(&mut pod_out);
        assert_eq!(
            String::from_utf8(pod_out).unwrap(),
            concat!(
                "\n",
                "Update Summary:\n",
                "  Reloaded (1):\n",
                "    - example-stack (Example Web Stack): example-stack-app (docker.io/example/example-app:latest)\n",
                "\n",
                "  Failed (1):\n",
                "    - proxy (Reverse Proxy): proxy-nginx (docker.io/library/nginx:stable-alpine)\n",
                "\n",
                "  Up to date (1):\n",
                "    - cache (Redis Cache)\n",
                "\n",
                "Checked 3 pods (4 containers): 1 reloaded, 1 up to date, 1 failed.\n",
            )
        );

        let mut cnt_summary =
            UpdateSummary::new(SummaryKind::LifecycleUpdate, SummaryScope::Container);
        cnt_summary.record_container(UpdateAction::Reloaded, &app_updated);
        cnt_summary.record_container(UpdateAction::UpToDate, &db_same);
        cnt_summary.record_container(UpdateAction::Failed, &nginx_failed);
        let mut cnt_out = Vec::new();
        cnt_summary.write_to(&mut cnt_out);
        let cnt_text = String::from_utf8(cnt_out).unwrap();
        assert!(cnt_text.contains(
            "  Reloaded (1):\n    - example-stack-app (docker.io/example/example-app:latest)\n"
        ));
        assert!(cnt_text.contains(
            "  Failed (1):\n    - proxy-nginx (docker.io/library/nginx:stable-alpine)\n"
        ));
        assert!(cnt_text.contains("  Up to date (1):\n    - example-stack-db\n"));
        assert!(cnt_text.contains("Checked 3 containers: 1 reloaded, 1 up to date, 1 failed.\n"));

        let mut img_pod_summary = UpdateSummary::new(SummaryKind::ImagePull, SummaryScope::Pod);
        img_pod_summary.record_pod(&pod_result(
            "example-stack",
            "Example Web Stack",
            UpdateAction::Failed,
            vec![
                pull_record(
                    "example-stack-db",
                    "docker.io/example/db-server:latest",
                    PullOutcome::Updated,
                ),
                pull_record(
                    "example-stack-app",
                    "docker.io/example/example-app:latest",
                    PullOutcome::Failed,
                ),
            ],
        ));
        img_pod_summary.record_pod(&pod_result(
            "cache",
            "Redis Cache",
            UpdateAction::UpToDate,
            vec![redis_same],
        ));
        let mut img_pod_out = Vec::new();
        img_pod_summary.write_to(&mut img_pod_out);
        let img_pod_text = String::from_utf8(img_pod_out).unwrap();
        assert!(
            img_pod_text.contains(
                "  Updated (1):\n    - example-stack (Example Web Stack): example-stack-db"
            )
        );
        assert!(
            img_pod_text.contains(
                "  Failed (1):\n    - example-stack (Example Web Stack): example-stack-app"
            )
        );
        assert!(img_pod_text.contains("  Up to date (1):\n    - cache (Redis Cache)\n"));
        assert!(
            img_pod_text
                .contains("Checked 2 pods (3 images): 1 updated, 1 up to date, 1 failed.\n")
        );

        let mut img_cnt_summary =
            UpdateSummary::new(SummaryKind::ImagePull, SummaryScope::Container);
        img_cnt_summary.record_container(UpdateAction::UpToDate, &app_updated);
        let mut img_cnt_out = Vec::new();
        img_cnt_summary.write_to(&mut img_cnt_out);
        let img_cnt_text = String::from_utf8(img_cnt_out).unwrap();
        assert!(img_cnt_text.contains("  Updated (1):\n"));
        assert!(img_cnt_text.contains("Checked 1 image: 1 updated, 0 up to date, 0 failed.\n"));
    }

    #[test]
    fn update_summary_renders_simulated_footers_and_enforces_bucket_bounds() {
        let sim_record = pull_record(
            "example-stack-db",
            "docker.io/example/db-server:latest",
            PullOutcome::Simulated,
        );
        let sim_pod = pod_result(
            "example-stack",
            "Example Web Stack",
            UpdateAction::Simulated,
            vec![sim_record.clone()],
        );

        for (kind, scope, expected_footer) in [
            (
                SummaryKind::LifecycleUpdate,
                SummaryScope::Pod,
                "Simulated 1 pod (1 container).\n",
            ),
            (
                SummaryKind::ImagePull,
                SummaryScope::Pod,
                "Simulated 1 pod (1 image).\n",
            ),
            (
                SummaryKind::LifecycleUpdate,
                SummaryScope::Container,
                "Simulated 1 container.\n",
            ),
            (
                SummaryKind::ImagePull,
                SummaryScope::Container,
                "Simulated 1 image.\n",
            ),
        ] {
            let mut summary = UpdateSummary::new(kind, scope);
            match scope {
                SummaryScope::Pod => summary.record_pod(&sim_pod),
                SummaryScope::Container => {
                    summary.record_container(UpdateAction::Simulated, &sim_record);
                }
            }
            let mut out = Vec::new();
            summary.write_to(&mut out);
            let text = String::from_utf8(out).unwrap();
            assert!(
                text.contains("  Simulated (1):\n"),
                "missing Simulated bucket for ({kind:?}, {scope:?}): {text}"
            );
            assert!(
                text.ends_with(expected_footer),
                "unexpected footer for ({kind:?}, {scope:?}): {text}"
            );
        }

        let mut bounded = UpdateSummary::new(SummaryKind::LifecycleUpdate, SummaryScope::Container);
        for index in 0..(MAX_SUMMARY_ENTRIES + 10) {
            bounded.record_container(
                UpdateAction::UpToDate,
                &pull_record(
                    &format!("cnt-{index}"),
                    "docker.io/alpine:latest",
                    PullOutcome::UpToDate,
                ),
            );
        }
        assert_eq!(bounded.up_to_date.count, MAX_SUMMARY_ENTRIES + 10);
        assert_eq!(bounded.up_to_date.items.len(), MAX_SUMMARY_ENTRIES);
        let mut bounded_out = Vec::new();
        bounded.write_to(&mut bounded_out);
        let bounded_text = String::from_utf8(bounded_out).unwrap();
        assert!(bounded_text.contains(&format!("  Up to date ({}):\n", MAX_SUMMARY_ENTRIES + 10)));
        assert!(bounded_text.contains(&format!(
            "Checked {} containers: 0 reloaded, {} up to date, 0 failed.\n",
            MAX_SUMMARY_ENTRIES + 10,
            MAX_SUMMARY_ENTRIES + 10,
        )));
    }
}
