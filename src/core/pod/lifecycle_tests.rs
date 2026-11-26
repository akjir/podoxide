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

//! Unit tests for pod lifecycle execution and Pod equality invariance in [`super`].

use std::str::FromStr;

use crate::core::log;
use crate::core::recipe::Recipe;
use crate::core::update::UpdateAction;
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::PullOutcome;
use super::update_tests::run_update_no_inspect;
use super::{
    Pod, PodCommand, create, create_with, down, down_with, pull, pull_with, reload, reload_with,
    remove, remove_with, restart, restart_with, start, start_with, stop, stop_with, up, up_with,
    update, update_with,
};

pub(super) fn sample_multi_container_recipe() -> Recipe {
    let psl = r#"
pod order {
    name "Order Stack"
    path /pods/order
    registry obey.io
    publish "127.0.0.1:42" 21 TCP
    publish 42 21 UDP
    publish 42 21
    publish "" "127.0.0.1::42"
    option "--unknown thing"
    option --another unknown
}
container {
    image order:1
}
container *two {
    image order:2
}
container three {
    image order:3
    registry custom.io
}
"#;
    let mut recipe = Recipe::from_str(psl).unwrap();
    for (index, container) in recipe.containers.iter_mut().enumerate() {
        container.ensure_name(&recipe.pod.id, index + 1).unwrap();
    }
    recipe
}

#[test]
fn pod_and_pod_command_defaults_equality_and_args() {
    let mut pod = Pod::new();
    assert!(pod.commands.is_empty());
    assert!(pod.description.is_empty());
    assert!(pod.id.is_empty());
    assert_eq!(pod.line, 0);
    assert!(pod.name.is_empty());
    assert!(pod.options.is_empty());
    assert!(pod.path.as_os_str().is_empty());
    assert!(pod.publish.is_empty());
    assert!(pod.registry.is_empty());

    let command = PodCommand::new();
    assert!(command.container.is_empty());
    assert!(command.description.is_empty());
    assert!(command.execute.is_empty());
    assert_eq!(command.line, 0);
    assert!(command.name.is_empty());
    assert_eq!(command.user, None);

    pod.id = String::from("si_po");
    pod.name = String::from("Simple Pod");
    pod.publish = vec![
        String::from("127.0.0.1:42:21/TCP"),
        String::from("127.0.0.1::42"),
    ];
    pod.options = vec![
        String::from("--unknown thing"),
        String::from("--another"),
        String::from("unknown"),
    ];
    let baseline = pod.clone();
    assert_eq!(pod, baseline);

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        pod.create_with(true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        pod, baseline,
        "Pod equality must remain invariant after create_with flips created"
    );
    assert_eq!(
        pod.stop_with(true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        pod.remove_with(true, &mut stdout, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    let simulated = String::from_utf8(stdout.clone()).unwrap();
    assert!(simulated.contains(
        "podman pod create --name si_po --publish 127.0.0.1:42:21/TCP --publish 127.0.0.1::42 --unknown thing --another unknown"
    ));
    assert!(simulated.contains("podman pod stop si_po"));
    assert!(simulated.contains("podman pod rm si_po"));

    let mut ensure_out = Vec::new();
    pod.ensure_created_with(true, |_| false, &mut ensure_out, &mut stderr)
        .unwrap();
    assert_eq!(pod, baseline);
    assert_eq!(std::str::from_utf8(&ensure_out).unwrap().lines().count(), 2);
    pod.ensure_created_with(
        true,
        |_| panic!("pod_exists should not be called once pod is marked created"),
        &mut ensure_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(std::str::from_utf8(&ensure_out).unwrap().lines().count(), 2);

    pod.remove_with(true, &mut stdout, &mut stderr).unwrap();
    ensure_out.clear();
    pod.ensure_created_with(true, |_| true, &mut ensure_out, &mut stderr)
        .unwrap();
    assert_eq!(
        pod, baseline,
        "Pod equality must remain invariant after ensure_created_with flips created"
    );
    assert!(ensure_out.is_empty());
    pod.ensure_created_with(
        true,
        |_| panic!("pod_exists should not be called after pod_exists returned true"),
        &mut ensure_out,
        &mut stderr,
    )
    .unwrap();
    assert!(ensure_out.is_empty());
}

#[test]
fn pod_composite_lifecycles_up_down_restart_reload_pull_update() {
    let recipe = sample_multi_container_recipe();

    let mut up_down_out = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        up_with(&recipe, true, &mut up_down_out, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    assert_eq!(
        down_with(&recipe, true, &mut up_down_out, &mut stderr).unwrap(),
        ExecOutcome::Succeeded
    );
    let up_down_lines: Vec<&str> = std::str::from_utf8(&up_down_out).unwrap().lines().collect();
    assert_eq!(up_down_lines.len(), 30);
    assert_eq!(up_down_lines[0], "Create pod 'Order Stack' ('order'):");
    assert_eq!(up_down_lines[8], "Start container 'order-1':");
    assert_eq!(up_down_lines[14], "Stop container 'three':");
    assert_eq!(up_down_lines[28], "Remove pod 'Order Stack' ('order'):");

    let mut restart_out = Vec::new();
    restart_with(&recipe, true, &mut restart_out, &mut stderr).unwrap();
    let restart_lines: Vec<&str> = std::str::from_utf8(&restart_out).unwrap().lines().collect();
    assert_eq!(
        restart_lines,
        vec![
            "Stop container 'three':",
            "podman stop three",
            "Stop container 'order-two':",
            "podman stop order-two",
            "Stop container 'order-1':",
            "podman stop order-1",
            "Stop pod 'Order Stack' ('order'):",
            "podman pod stop order",
            "Start container 'order-1':",
            "podman start order-1",
            "Start container 'order-two':",
            "podman start order-two",
            "Start container 'three':",
            "podman start three",
        ]
    );

    let mut pull_out = Vec::new();
    let pull_result = pull_with(&recipe, true, &mut pull_out, &mut stderr).unwrap();
    assert_eq!(pull_result.action, UpdateAction::Simulated);
    assert_eq!(pull_result.containers.len(), 3);

    let mut reload_out = Vec::new();
    reload_with(&recipe, true, &mut reload_out, &mut stderr).unwrap();
    let reload_lines: Vec<&str> = std::str::from_utf8(&reload_out).unwrap().lines().collect();
    assert_eq!(reload_lines.len(), 30);

    let mut update_out = Vec::new();
    let update_result = update_with(&recipe, true, &mut update_out, &mut stderr).unwrap();
    assert_eq!(update_result.action, UpdateAction::Simulated);
    let update_lines: Vec<&str> = std::str::from_utf8(&update_out).unwrap().lines().collect();
    assert_eq!(update_lines.len(), 37);
    assert!(stderr.is_empty());

    let mut up_to_date_out = Vec::new();
    let mut unchanged_pull =
        |_: &str, prefix: &str, _: bool, out: &mut Vec<u8>, _: &mut Vec<u8>| {
            log::write_print(out, &format!("{prefix}Up to date."));
            Ok(PullOutcome::UpToDate)
        };
    let up_to_date_result = run_update_no_inspect(
        &recipe,
        &mut unchanged_pull,
        &mut up_to_date_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(up_to_date_result.action, UpdateAction::UpToDate);
    assert_eq!(
        std::str::from_utf8(&up_to_date_out)
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        vec![
            "Update pod 'Order Stack' ('order') ...",
            "  Trying to pull obey.io/order:1 (order-1): Up to date.",
            "  Trying to pull obey.io/order:2 (order-two): Up to date.",
            "  Trying to pull custom.io/order:3 (three): Up to date.",
            "  Pod 'Order Stack' ('order') is up to date.",
        ]
    );

    let mut partial_update_out = Vec::new();
    let mut partial_pull =
        |image_ref: &str, prefix: &str, _: bool, out: &mut Vec<u8>, _: &mut Vec<u8>| {
            if image_ref == "obey.io/order:2" {
                log::write_print(out, &format!("{prefix}Updated."));
                Ok(PullOutcome::Updated)
            } else {
                log::write_print(out, &format!("{prefix}Up to date."));
                Ok(PullOutcome::UpToDate)
            }
        };
    let partial_result = run_update_no_inspect(
        &recipe,
        &mut partial_pull,
        &mut partial_update_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(partial_result.action, UpdateAction::Reloaded);
    let partial_lines: Vec<&str> = std::str::from_utf8(&partial_update_out)
        .unwrap()
        .lines()
        .collect();
    assert_eq!(partial_lines.len(), 34);

    let mut failed_update_out = Vec::new();
    let mut mixed_failure_pull =
        |image_ref: &str, prefix: &str, _: bool, out: &mut Vec<u8>, _: &mut Vec<u8>| {
            if image_ref == "obey.io/order:2" {
                log::write_print(out, &format!("{prefix}Updated."));
                Ok(PullOutcome::Updated)
            } else if image_ref == "custom.io/order:3" {
                log::write_print(out, &format!("{prefix}Failed."));
                Ok(PullOutcome::Failed)
            } else {
                log::write_print(out, &format!("{prefix}Up to date."));
                Ok(PullOutcome::UpToDate)
            }
        };
    let failed_result = run_update_no_inspect(
        &recipe,
        &mut mixed_failure_pull,
        &mut failed_update_out,
        &mut stderr,
    )
    .unwrap();
    assert_eq!(failed_result.action, UpdateAction::Failed);

    assert_eq!(create(&recipe, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(start(&recipe, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(stop(&recipe, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(remove(&recipe, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(up(&recipe, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(down(&recipe, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(restart(&recipe, true).unwrap(), ExecOutcome::Succeeded);
    assert_eq!(reload(&recipe, true).unwrap(), ExecOutcome::Succeeded);
    assert!(pull(&recipe, true).is_ok());
    assert!(update(&recipe, true).is_ok());
}

#[test]
fn pod_create_start_stop_remove_ordering() {
    let recipe = sample_multi_container_recipe();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    create_with(&recipe, true, &mut stdout, &mut stderr).unwrap();
    start_with(&recipe, true, &mut stdout, &mut stderr).unwrap();
    stop_with(&recipe, true, &mut stdout, &mut stderr).unwrap();
    remove_with(&recipe, true, &mut stdout, &mut stderr).unwrap();

    let output = String::from_utf8(stdout).unwrap();
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines.len(), 30);
    assert_eq!(lines[0], "Create pod 'Order Stack' ('order'):");
    assert_eq!(lines[12], "Start container 'three':");
    assert_eq!(lines[14], "Stop container 'three':");
    assert_eq!(lines[28], "Remove pod 'Order Stack' ('order'):");
    assert!(stderr.is_empty());

    // Simulating pod creation must not satisfy subsequent live ensure_created_with calls.
    let sim_then_live_pod = sample_multi_container_recipe().pod;
    let mut sim_out = Vec::new();
    let mut sim_err = Vec::new();
    assert_eq!(
        sim_then_live_pod
            .ensure_created_with(true, |_| false, &mut sim_out, &mut sim_err)
            .unwrap(),
        ExecOutcome::Succeeded
    );
    let mut exists_checked = false;
    let mut live_out = Vec::new();
    let mut live_err = Vec::new();
    let _ = sim_then_live_pod.ensure_created_with(
        false,
        |_| {
            exists_checked = true;
            true
        },
        &mut live_out,
        &mut live_err,
    );
    assert!(
        exists_checked,
        "simulated pod creation must not short-circuit live pod_exists checks"
    );
}

#[test]
fn pod_down_and_start_continue_when_containers_do_not_exist_and_return_failed_outcome() {
    let mut recipe = sample_multi_container_recipe();
    recipe.pod.id = String::from("podoxide_missing_pod_999999");
    recipe.containers.truncate(2);
    recipe.containers[0].name = String::from("podoxide_missing_pod_999999-1");
    recipe.containers[1].name = String::from("podoxide_missing_pod_999999-2");

    let mut down_stdout = Vec::new();
    let mut down_stderr = Vec::new();
    let down_outcome = down_with(&recipe, false, &mut down_stdout, &mut down_stderr).unwrap();
    assert_eq!(down_outcome, ExecOutcome::Failed);
    assert!(down_stdout.is_empty());

    let down_err_text = String::from_utf8(down_stderr).unwrap();
    assert_eq!(
        down_err_text
            .lines()
            .filter(|line| line.starts_with("WARNING: Command exited with code '"))
            .count(),
        6
    );

    let mut start_stdout = Vec::new();
    let mut start_stderr = Vec::new();
    let start_outcome = start_with(&recipe, false, &mut start_stdout, &mut start_stderr).unwrap();
    assert_eq!(start_outcome, ExecOutcome::Failed);
    assert!(start_stdout.is_empty());

    let start_err_text = String::from_utf8(start_stderr).unwrap();
    assert_eq!(
        start_err_text
            .lines()
            .filter(|line| line.starts_with("WARNING: Command exited with code '"))
            .count(),
        2
    );
}

#[test]
fn pod_create_with_invokes_pod_create_only_once_when_pod_create_fails() {
    let mut recipe = sample_multi_container_recipe();
    recipe.pod.id = String::from("podoxide_create_fail_999999");
    recipe.pod.options = vec![String::from("--podoxide-nonexistent-flag")];
    for (index, container) in recipe.containers.iter_mut().enumerate() {
        container.name = format!("podoxide_create_fail_999999-{}", index + 1);
        container.options = vec![String::from("--pull=never")];
    }

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let outcome = create_with(&recipe, false, &mut stdout, &mut stderr).unwrap();
    assert_eq!(outcome, ExecOutcome::Failed);

    let combined = format!(
        "{}{}",
        String::from_utf8(stdout).unwrap(),
        String::from_utf8(stderr).unwrap()
    );
    let pod_create_invocations = combined.matches("Create pod 'Order Stack'").count();
    assert_eq!(
        pod_create_invocations, 1,
        "pod::create_with must invoke podman pod create only once on failure:\n{combined}"
    );
}
