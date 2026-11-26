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

//! Unit tests for pod image pull deduplication and stale-container detection in [`super`].

use std::str::FromStr;

use crate::core::log;
use crate::core::recipe::Recipe;
use crate::core::update::{PodUpdateResult, PullCache, UpdateAction, UpdateRunners};
use crate::error::PodError;
use crate::io::exec::podman::PullOutcome;
use super::update_with_cache;

pub(super) fn run_update_no_inspect<P>(
    recipe: &Recipe,
    pull_image: &mut P,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
) -> Result<PodUpdateResult, PodError>
where
    P: FnMut(&str, &str, bool, &mut Vec<u8>, &mut Vec<u8>) -> Result<PullOutcome, PodError>,
{
    let mut cache = PullCache::new();
    let mut no_container = |_: &str| None;
    let mut no_image = |_: &str| None;
    let mut runners = UpdateRunners {
        cache: &mut cache,
        pull_image,
        query_container_image: &mut no_container,
        query_image: &mut no_image,
    };
    update_with_cache(recipe, true, &mut runners, stdout, stderr)
}

#[test]
fn pod_update_deduplicates_shared_image_pulls_and_detects_stale_containers() {
    let shared_psl = "\
pod shared {
    name \"Shared Stack\"
    path /pods/shared
    registry docker.io
}
container *a {
    image postgres:16
}
container *b {
    image postgres:16
}
";
    let mut shared_recipe = Recipe::from_str(shared_psl).unwrap();
    for (index, container) in shared_recipe.containers.iter_mut().enumerate() {
        container
            .ensure_name(&shared_recipe.pod.id, index + 1)
            .unwrap();
    }

    let mut pull_calls = 0_usize;
    let mut counting_pull = |_: &str, prefix: &str, _: bool, out: &mut Vec<u8>, _: &mut Vec<u8>| {
        pull_calls += 1;
        log::write_print(out, &format!("{prefix}Updated."));
        Ok(PullOutcome::Updated)
    };
    let mut update_out = Vec::new();
    let mut stderr = Vec::new();
    let dedup_result = run_update_no_inspect(
        &shared_recipe,
        &mut counting_pull,
        &mut update_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(dedup_result.action, UpdateAction::Reloaded);
    assert_eq!(pull_calls, 1);

    let mut stale_pull_calls = 0_usize;
    let mut up_to_date_pull =
        |_: &str, prefix: &str, _: bool, out: &mut Vec<u8>, _: &mut Vec<u8>| {
            stale_pull_calls += 1;
            log::write_print(out, &format!("{prefix}Up to date."));
            Ok(PullOutcome::UpToDate)
        };
    let mut image_id_calls = 0_usize;
    let mut query_container = |name: &str| {
        Some(String::from(if name == "shared-b" {
            "sha256:old"
        } else {
            "sha256:new"
        }))
    };
    let mut query_image = |_: &str| {
        image_id_calls += 1;
        Some(String::from("sha256:new"))
    };
    let mut cache = PullCache::new();
    let mut stale_out = Vec::new();
    let mut runners = UpdateRunners {
        cache: &mut cache,
        pull_image: &mut up_to_date_pull,
        query_container_image: &mut query_container,
        query_image: &mut query_image,
    };
    let stale_result = update_with_cache(
        &shared_recipe,
        true,
        &mut runners,
        &mut stale_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(stale_pull_calls, 1);
    assert_eq!(image_id_calls, 1);
    assert_eq!(stale_result.action, UpdateAction::Reloaded);
    assert_eq!(stale_result.containers[0].outcome, PullOutcome::UpToDate);
    assert_eq!(stale_result.containers[1].outcome, PullOutcome::Updated);
    let stale_text = String::from_utf8(stale_out).unwrap();
    let notice = "  Container 'shared-b' is running an older image; reloading.";
    let notice_pos = stale_text
        .find(notice)
        .expect("expected indented stale-container notice before pod reload");
    let stop_pos = stale_text
        .find("  Stop container 'shared-b':\n")
        .expect("expected indented Stop container output");
    assert!(
        notice_pos < stop_pos,
        "stale notice must appear before pod reload output:\n{stale_text}"
    );
    assert!(stale_text.contains("  Start container 'shared-b':\n"));

    let mut current_cache = PullCache::new();
    let mut current_out = Vec::new();
    let mut current_pull = |_: &str, prefix: &str, _: bool, out: &mut Vec<u8>, _: &mut Vec<u8>| {
        log::write_print(out, &format!("{prefix}Up to date."));
        Ok(PullOutcome::UpToDate)
    };
    let mut query_all_current = |_: &str| Some(String::from("sha256:new"));
    let mut query_same = |_: &str| Some(String::from("sha256:new"));
    let mut current_runners = UpdateRunners {
        cache: &mut current_cache,
        pull_image: &mut current_pull,
        query_container_image: &mut query_all_current,
        query_image: &mut query_same,
    };
    let current_result = update_with_cache(
        &shared_recipe,
        true,
        &mut current_runners,
        &mut current_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(current_result.action, UpdateAction::UpToDate);
    assert!(
        String::from_utf8(current_out)
            .unwrap()
            .contains("  Pod 'Shared Stack' ('shared') is up to date.")
    );

    // Failed pull in one container must skip stale-image inspection and keep other outcomes unmutated.
    let mut fail_cache = PullCache::new();
    let mut fail_out = Vec::new();
    let mut fail_calls = 0_usize;
    let mut partial_fail_pull =
        |_: &str, prefix: &str, _: bool, out: &mut Vec<u8>, _: &mut Vec<u8>| {
            fail_calls += 1;
            if fail_calls == 1 {
                log::write_print(out, &format!("{prefix}Up to date."));
                Ok(PullOutcome::UpToDate)
            } else {
                log::write_print(out, &format!("{prefix}Failed."));
                Ok(PullOutcome::Failed)
            }
        };
    let mut container_inspect_called = false;
    let mut image_inspect_called = false;
    let mut query_never = |_: &str| {
        container_inspect_called = true;
        Some(String::from("sha256:old"))
    };
    let mut query_image_never = |_: &str| {
        image_inspect_called = true;
        Some(String::from("sha256:new"))
    };
    let mut fail_runners = UpdateRunners {
        cache: &mut fail_cache,
        pull_image: &mut partial_fail_pull,
        query_container_image: &mut query_never,
        query_image: &mut query_image_never,
    };
    let mut distinct_recipe = shared_recipe;
    distinct_recipe.containers[1].image = String::from("shared:other");
    let fail_result = update_with_cache(
        &distinct_recipe,
        true,
        &mut fail_runners,
        &mut fail_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(fail_result.action, UpdateAction::Failed);
    assert!(
        !container_inspect_called && !image_inspect_called,
        "stale-image inspection must not run when any container pull failed"
    );
    assert_eq!(fail_result.containers[0].outcome, PullOutcome::UpToDate);
}
