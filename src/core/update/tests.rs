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

//! Unit tests for update summaries, batch pull caching, and [`super::IndentedWriter`].

use std::io::Write;

use crate::io::exec::podman::PullOutcome;
use super::summary::{format_pod_detail_entry, format_pod_label};
use super::{
    ContainerPullRecord, IndentedWriter, MAX_SUMMARY_ENTRIES, POD_INDENT, PodUpdateResult,
    PullCache, SummaryKind, SummaryScope, UpdateAction, UpdateSummary, format_pull_prefix,
    pull_outcome_suffix,
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
fn format_helpers_pull_cache_and_indented_writer_handle_edge_cases() {
    assert_eq!(
        format_pull_prefix("webapp-db", "docker.io/postgres:16", POD_INDENT, false),
        "  Trying to pull docker.io/postgres:16 (webapp-db)... "
    );
    assert_eq!(
        format_pull_prefix("webapp-db", "docker.io/postgres:16", "", true),
        "Trying to pull docker.io/postgres:16 (webapp-db): "
    );
    assert_eq!(pull_outcome_suffix(PullOutcome::Failed), "Failed.");
    assert_eq!(pull_outcome_suffix(PullOutcome::Simulated), "Simulated.");
    assert_eq!(pull_outcome_suffix(PullOutcome::UpToDate), "Up to date.");
    assert_eq!(pull_outcome_suffix(PullOutcome::Updated), "Updated.");

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

    let mut cache = PullCache::new();
    cache.record_outcome("docker.io/postgres:16", PullOutcome::Simulated);
    assert_eq!(cache.get_outcome("docker.io/postgres:16"), None);
    cache.record_outcome("docker.io/postgres:16", PullOutcome::Updated);
    assert_eq!(
        cache.get_outcome("docker.io/postgres:16"),
        Some(PullOutcome::Updated)
    );
    let mut query_calls = 0_usize;
    let mut query = |_: &str| {
        query_calls += 1;
        Some(String::from("sha256:abc"))
    };
    assert_eq!(
        cache
            .image_id("docker.io/postgres:16", &mut query)
            .as_deref(),
        Some("sha256:abc")
    );
    assert_eq!(
        cache
            .image_id("docker.io/postgres:16", &mut query)
            .as_deref(),
        Some("sha256:abc")
    );
    assert_eq!(query_calls, 1);

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

    let mut cr_buffer = Vec::new();
    {
        let mut writer = IndentedWriter::new(&mut cr_buffer, POD_INDENT);
        writer.write_all(b"step 1\rstep 2\r\n\r\nstep 3\r").unwrap();
        writer.write_all(b"step 4\n").unwrap();
        writer.flush().unwrap();
    }
    assert_eq!(
        String::from_utf8(cr_buffer).unwrap(),
        "  step 1\r  step 2\r\n\r\n  step 3\r  step 4\n"
    );
}

#[test]
fn update_summary_renders_pod_and_container_outcomes_and_reports_failures() {
    let empty_summary = UpdateSummary::new(SummaryKind::LifecycleUpdate, SummaryScope::Pod);
    assert!(!empty_summary.has_failures());
    let mut empty_out = Vec::new();
    empty_summary.write_to(&mut empty_out);
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
    assert!(!pod_summary.has_failures());
    pod_summary.record_pod(&pod_result(
        "proxy",
        "Reverse Proxy",
        UpdateAction::Failed,
        vec![nginx_failed.clone()],
    ));
    assert!(pod_summary.has_failures());
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

    let mut cnt_summary = UpdateSummary::new(SummaryKind::LifecycleUpdate, SummaryScope::Container);
    cnt_summary.record_container(UpdateAction::Reloaded, &app_updated);
    cnt_summary.record_container(UpdateAction::UpToDate, &db_same);
    assert!(!cnt_summary.has_failures());
    cnt_summary.record_container(UpdateAction::Failed, &nginx_failed);
    assert!(cnt_summary.has_failures());
    let mut cnt_out = Vec::new();
    cnt_summary.write_to(&mut cnt_out);
    let cnt_text = String::from_utf8(cnt_out).unwrap();
    assert!(cnt_text.contains(
        "  Reloaded (1):\n    - example-stack-app (docker.io/example/example-app:latest)\n"
    ));
    assert!(
        cnt_text
            .contains("  Failed (1):\n    - proxy-nginx (docker.io/library/nginx:stable-alpine)\n")
    );
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
    assert!(img_pod_summary.has_failures());
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
        img_pod_text
            .contains("  Updated (1):\n    - example-stack (Example Web Stack): example-stack-db")
    );
    assert!(
        img_pod_text
            .contains("  Failed (1):\n    - example-stack (Example Web Stack): example-stack-app")
    );
    assert!(img_pod_text.contains("  Up to date (1):\n    - cache (Redis Cache)\n"));
    assert!(
        img_pod_text.contains("Checked 2 pods (3 images): 1 updated, 1 up to date, 1 failed.\n")
    );

    let mut img_cnt_summary = UpdateSummary::new(SummaryKind::ImagePull, SummaryScope::Container);
    img_cnt_summary.record_container(UpdateAction::UpToDate, &app_updated);
    assert!(!img_cnt_summary.has_failures());
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
        assert!(!summary.has_failures());
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
    let mut bounded_out = Vec::new();
    bounded.write_to(&mut bounded_out);
    let bounded_text = String::from_utf8(bounded_out).unwrap();
    assert!(bounded_text.contains(&format!("  Up to date ({}):\n", MAX_SUMMARY_ENTRIES + 10)));
    assert!(bounded_text.contains(&format!(
        "Checked {} containers: 0 reloaded, {} up to date, 0 failed.\n",
        MAX_SUMMARY_ENTRIES + 10,
        MAX_SUMMARY_ENTRIES + 10,
    )));
    assert_eq!(
        bounded_text
            .lines()
            .filter(|line| line.starts_with("    - cnt-"))
            .count(),
        MAX_SUMMARY_ENTRIES
    );

    // Reload failure after successful pulls must include 'reload failed' in the Failed bucket.
    let mut reload_fail_summary =
        UpdateSummary::new(SummaryKind::LifecycleUpdate, SummaryScope::Pod);
    reload_fail_summary.record_pod(&pod_result(
        "webapp",
        "Web App",
        UpdateAction::Failed,
        vec![
            pull_record("webapp-db", "docker.io/postgres:16", PullOutcome::UpToDate),
            pull_record("webapp-app", "docker.io/app:latest", PullOutcome::Updated),
        ],
    ));
    assert!(reload_fail_summary.has_failures());
    let mut reload_fail_out = Vec::new();
    reload_fail_summary.write_to(&mut reload_fail_out);
    let reload_fail_text = String::from_utf8(reload_fail_out).unwrap();
    assert!(
        reload_fail_text.contains("  Failed (1):\n    - webapp (Web App): reload failed\n"),
        "expected reload failure detail in Failed bucket:\n{reload_fail_text}"
    );

    let mut container_reload_fail =
        UpdateSummary::new(SummaryKind::LifecycleUpdate, SummaryScope::Container);
    container_reload_fail.record_container(
        UpdateAction::Failed,
        &pull_record("webapp-app", "docker.io/app:latest", PullOutcome::Updated),
    );
    assert!(container_reload_fail.has_failures());
    let mut cnt_fail_out = Vec::new();
    container_reload_fail.write_to(&mut cnt_fail_out);
    let cnt_fail_text = String::from_utf8(cnt_fail_out).unwrap();
    assert!(
        cnt_fail_text.contains("  Failed (1):\n    - webapp-app: reload failed\n"),
        "expected container reload failure detail in Failed bucket:\n{cnt_fail_text}"
    );
}
