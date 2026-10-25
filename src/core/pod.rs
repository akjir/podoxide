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

//! Pod domain models (`pod` and `command` blocks in `.pod` recipes) and Podman pod lifecycle orchestration.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::core::container;
use crate::core::log;
use crate::core::recipe::Recipe;
use crate::error::{PodError, RecipeError, RecipeErrorKind};
use crate::io::exec::command::{self, ExecOptions};
use crate::utilities::collection::{
    MAX_COLLECTION_PREALLOC, append_option_tokens, bounded_capacity,
};
use crate::utilities::string::{is_valid_pod_or_container_name, is_valid_text, is_valid_token};

const BASE_POD_CREATE_ARG_COUNT: usize = 4;

/// Pod configuration and custom command definitions from the `pod` block of a `.pod` recipe.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Pod {
    pub commands: BTreeMap<String, PodCommand>,
    pub description: String,
    pub id: String,
    pub line: usize,
    pub name: String,
    pub options: Vec<String>,
    pub path: PathBuf,
    pub publish: Vec<String>,
    pub registry: String,
}

impl Pod {
    /// Creates a new [`Pod`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the `podman pod create` argument vector for this pod.
    #[must_use]
    pub fn build_create_args(&self) -> Vec<String> {
        let capacity = bounded_capacity(
            BASE_POD_CREATE_ARG_COUNT
                .saturating_add(self.publish.len().saturating_mul(2))
                .saturating_add(self.options.len().saturating_mul(2)),
            MAX_COLLECTION_PREALLOC,
        );
        let mut arguments = Vec::with_capacity(capacity);
        arguments.push(String::from("pod"));
        arguments.push(String::from("create"));
        arguments.push(String::from("--name"));
        arguments.push(self.id.clone());
        append_publish_arguments(&mut arguments, &self.publish);
        append_option_tokens(&mut arguments, &self.options);
        arguments
    }

    /// Builds the `podman pod rm` argument vector for this pod.
    #[must_use]
    pub fn build_remove_args(&self) -> Vec<String> {
        vec![String::from("pod"), String::from("rm"), self.id.clone()]
    }

    /// Builds the `podman pod stop` argument vector for this pod.
    #[must_use]
    pub fn build_stop_args(&self) -> Vec<String> {
        vec![String::from("pod"), String::from("stop"), self.id.clone()]
    }

    /// Creates this pod in Podman (`podman pod create`), writing command output to `stdout` and `stderr`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if pod validation, stream writing, or Podman execution fails.
    pub fn create_with<W: Write, E: Write>(
        &self,
        simulate: bool,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<(), PodError> {
        validate_pod_create(self)?;
        let arguments = self.build_create_args();
        let prefix = format!("Create pod '{}' ('{}'): ", self.name, self.id);
        command::exec_with(
            "podman",
            &arguments,
            &ExecOptions {
                interactive: false,
                prefix: &prefix,
                simulate,
            },
            stdout,
            stderr,
        )
    }

    /// Removes this pod in Podman (`podman pod rm`), writing command output to `stdout` and `stderr`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if pod validation, stream writing, or Podman execution fails.
    pub fn remove_with<W: Write, E: Write>(
        &self,
        simulate: bool,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<(), PodError> {
        validate_pod_id(self)?;
        let arguments = self.build_remove_args();
        let prefix = format!("Remove pod '{}' ('{}'): ", self.name, self.id);
        command::exec_with(
            "podman",
            &arguments,
            &ExecOptions {
                interactive: false,
                prefix: &prefix,
                simulate,
            },
            stdout,
            stderr,
        )
    }

    /// Stops this pod in Podman (`podman pod stop`), writing command output to `stdout` and `stderr`.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if pod validation, stream writing, or Podman execution fails.
    pub fn stop_with<W: Write, E: Write>(
        &self,
        simulate: bool,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<(), PodError> {
        validate_pod_id(self)?;
        let arguments = self.build_stop_args();
        let prefix = format!("Stop pod '{}' ('{}'): ", self.name, self.id);
        command::exec_with(
            "podman",
            &arguments,
            &ExecOptions {
                interactive: false,
                prefix: &prefix,
                simulate,
            },
            stdout,
            stderr,
        )
    }
}

/// Custom command definition from a `command <name>` sub-block inside a `pod` block.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodCommand {
    pub container: String,
    pub description: String,
    pub execute: String,
    pub name: String,
    pub user: Option<String>,
}

impl PodCommand {
    /// Creates a new [`PodCommand`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Creates `recipe.pod` (`podman pod create`) and all `recipe.containers` (`podman create`) in `1..N` order.
///
/// # Errors
///
/// Returns [`PodError`] if pod or container creation fails.
pub fn create(recipe: &Recipe, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    create_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Creates `recipe.pod` and all `recipe.containers` in `1..N` order, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn create_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_recipe_create(recipe)?;
    recipe.pod.create_with(simulate, stdout, stderr)?;
    for item in &recipe.containers {
        container::create_with(item, &recipe.pod, simulate, stdout, stderr)?;
    }
    Ok(())
}

/// Stops and removes `recipe.containers` (in reverse `N..1` order) and `recipe.pod` (`stop` + `remove`).
///
/// # Errors
///
/// Returns [`PodError`] if stopping or removing the pod or its containers fails.
pub fn down(recipe: &Recipe, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    down_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Stops and removes `recipe.containers` and `recipe.pod`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn down_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    stop_with(recipe, simulate, stdout, stderr)?;
    remove_with(recipe, simulate, stdout, stderr)
}

/// Pulls the latest images (`podman pull`) for all `recipe.containers` in `1..N` order without restarting the pod.
///
/// # Errors
///
/// Returns [`PodError`] if pulling any container image fails.
pub fn pull(recipe: &Recipe, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    pull_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Pulls the latest images for all `recipe.containers` in `1..N` order, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if pod/container validation or Podman execution fails.
pub fn pull_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_recipe_pull(recipe)?;
    log::write_print(
        stdout,
        &format!("Update pod '{}' ('{}') ...", recipe.pod.name, recipe.pod.id),
    );
    for item in &recipe.containers {
        container::pull_with(item, &recipe.pod, simulate, stdout, stderr)?;
    }
    Ok(())
}

/// Stops and removes `recipe.containers` and `recipe.pod`, then recreates `recipe.pod` and creates and starts `recipe.containers` (`down` + `up`).
///
/// # Errors
///
/// Returns [`PodError`] if tearing down or bringing up the pod fails.
pub fn reload(recipe: &Recipe, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    reload_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Stops and removes `recipe.containers` and `recipe.pod`, then recreates `recipe.pod` and creates and starts `recipe.containers`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn reload_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_recipe_create(recipe)?;
    down_with(recipe, simulate, stdout, stderr)?;
    up_with(recipe, simulate, stdout, stderr)
}

/// Removes `recipe.containers` in reverse `N..1` order (`podman rm`) followed by `recipe.pod` (`podman pod rm`).
///
/// # Errors
///
/// Returns [`PodError`] if container or pod removal fails.
pub fn remove(recipe: &Recipe, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    remove_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Removes `recipe.containers` in reverse `N..1` order followed by `recipe.pod`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn remove_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_recipe_containers(recipe)?;
    for item in recipe.containers.iter().rev() {
        container::remove_with(item, simulate, stdout, stderr)?;
    }
    recipe.pod.remove_with(simulate, stdout, stderr)
}

/// Stops `recipe.containers` and `recipe.pod` and starts `recipe.containers` in place (`stop` + `start`).
///
/// # Errors
///
/// Returns [`PodError`] if stopping the containers or pod or starting the containers fails.
pub fn restart(recipe: &Recipe, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    restart_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Stops `recipe.containers` and `recipe.pod` and starts `recipe.containers` in place, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn restart_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    stop_with(recipe, simulate, stdout, stderr)?;
    start_with(recipe, simulate, stdout, stderr)
}

/// Starts all `recipe.containers` (`podman start`) in `1..N` order.
///
/// # Errors
///
/// Returns [`PodError`] if pod validation or starting any container fails.
pub fn start(recipe: &Recipe, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    start_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Starts all `recipe.containers` in `1..N` order, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if pod validation, stream writing, or Podman execution fails.
pub fn start_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_recipe_containers(recipe)?;
    for item in &recipe.containers {
        container::start_with(item, simulate, stdout, stderr)?;
    }
    Ok(())
}

/// Stops `recipe.containers` in reverse `N..1` order (`podman stop`) followed by `recipe.pod` (`podman pod stop`).
///
/// # Errors
///
/// Returns [`PodError`] if stopping any container or the pod fails.
pub fn stop(recipe: &Recipe, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    stop_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Stops `recipe.containers` in reverse `N..1` order followed by `recipe.pod`, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn stop_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_recipe_containers(recipe)?;
    for item in recipe.containers.iter().rev() {
        container::stop_with(item, simulate, stdout, stderr)?;
    }
    recipe.pod.stop_with(simulate, stdout, stderr)
}

/// Creates `recipe.pod` and all `recipe.containers` and starts `recipe.containers` in `1..N` order (`create` + `start`).
///
/// # Errors
///
/// Returns [`PodError`] if creating the pod or its containers or starting the containers fails.
pub fn up(recipe: &Recipe, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    up_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Creates `recipe.pod` and all `recipe.containers` and starts `recipe.containers` in `1..N` order, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn up_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    create_with(recipe, simulate, stdout, stderr)?;
    start_with(recipe, simulate, stdout, stderr)
}

/// Pulls the latest images for `recipe.containers` and reloads the pod (`pull` + `reload`).
///
/// # Errors
///
/// Returns [`PodError`] if pulling images or reloading the pod fails.
pub fn update(recipe: &Recipe, simulate: bool) -> Result<(), PodError> {
    let mut stdout = io::stdout();
    let mut stderr = io::stderr();
    update_with(recipe, simulate, &mut stdout, &mut stderr)
}

/// Pulls the latest images for `recipe.containers` and reloads the pod, writing output to `stdout` and `stderr`.
///
/// # Errors
///
/// Returns [`PodError`] if stream writing or Podman execution fails.
pub fn update_with<W: Write, E: Write>(
    recipe: &Recipe,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError> {
    validate_recipe_pull(recipe)?;
    validate_recipe_create(recipe)?;
    pull_with(recipe, simulate, stdout, stderr)?;
    reload_with(recipe, simulate, stdout, stderr)
}

/// Appends `--publish <entry>` pairs from `publish_entries` into `arguments`.
fn append_publish_arguments(arguments: &mut Vec<String>, publish_entries: &[String]) {
    for entry in publish_entries {
        arguments.push(String::from("--publish"));
        arguments.push(entry.clone());
    }
}

/// Validates `pod.id`, `pod.publish`, and `pod.options` before executing `podman pod create`.
fn validate_pod_create(pod: &Pod) -> Result<(), RecipeError> {
    validate_pod_id(pod)?;
    for entry in &pod.publish {
        if !is_valid_token(entry) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidPublish,
                pod.line,
                entry,
            ));
        }
    }
    for option in &pod.options {
        if !is_valid_text(option) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidOption,
                pod.line,
                option,
            ));
        }
    }
    Ok(())
}

/// Validates that `pod.id` is a well-formed non-empty Podman pod name.
fn validate_pod_id(pod: &Pod) -> Result<(), RecipeError> {
    if !is_valid_pod_or_container_name(&pod.id) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidPodId,
            pod.line,
            &pod.id,
        ));
    }
    Ok(())
}

/// Validates `recipe.pod.id` and all container names in `recipe.containers` before executing container lifecycle commands.
fn validate_recipe_containers(recipe: &Recipe) -> Result<(), RecipeError> {
    validate_pod_id(&recipe.pod)?;
    for item in &recipe.containers {
        if !is_valid_pod_or_container_name(&item.name) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidContainerName,
                item.line,
                &item.name,
            ));
        }
    }
    Ok(())
}

/// Validates `recipe.pod` and all `recipe.containers` before executing pod creation lifecycles.
fn validate_recipe_create(recipe: &Recipe) -> Result<(), RecipeError> {
    validate_pod_create(&recipe.pod)?;
    for item in &recipe.containers {
        item.validate_create(&recipe.pod)?;
    }
    Ok(())
}

/// Validates `recipe.pod.id`, `recipe.pod.registry`, and all `recipe.containers` before pulling container images.
fn validate_recipe_pull(recipe: &Recipe) -> Result<(), RecipeError> {
    validate_pod_id(&recipe.pod)?;
    if !recipe.pod.registry.is_empty() && !is_valid_token(&recipe.pod.registry) {
        return Err(RecipeError::with_token(
            RecipeErrorKind::InvalidRegistry,
            recipe.pod.line,
            &recipe.pod.registry,
        ));
    }
    for item in &recipe.containers {
        if !is_valid_pod_or_container_name(&item.name) {
            return Err(RecipeError::with_token(
                RecipeErrorKind::InvalidContainerName,
                item.line,
                &item.name,
            ));
        }
        item.validate()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::str::FromStr;

    use crate::core::container::VolumeMount;
    use crate::core::log::MAX_LOG_MESSAGE_BYTES;
    use crate::core::recipe::Recipe;
    use crate::error::{PodError, RecipeErrorKind};
    use super::{
        Pod, PodCommand, create, create_with, down, down_with, pull, pull_with, reload,
        reload_with, remove, remove_with, restart, restart_with, start, start_with, stop,
        stop_with, up, up_with, update, update_with,
    };

    struct BrokenPipeWriter;

    impl Write for BrokenPipeWriter {
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
        }

        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
        }
    }

    fn assert_create_lifecycles_reject(
        recipe: &Recipe,
        expected_kind: RecipeErrorKind,
        expected_line: usize,
    ) {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert!(matches!(
            create_with(recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == expected_kind && error.line == Some(expected_line)
        ));
        assert!(matches!(
            up_with(recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == expected_kind && error.line == Some(expected_line)
        ));
        assert!(matches!(
            reload_with(recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == expected_kind && error.line == Some(expected_line)
        ));
        assert!(matches!(
            update_with(recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == expected_kind && error.line == Some(expected_line)
        ));
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
    }

    fn assert_invalid_pod_id_rejected(recipe: &Recipe, invalid_id: &str) {
        let mut bad_recipe = recipe.clone();
        bad_recipe.pod.id = String::from(invalid_id);
        bad_recipe.pod.line = 7;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert!(matches!(
            bad_recipe.pod.create_with(true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
        ));
        assert!(matches!(
            bad_recipe.pod.remove_with(true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
        ));
        assert!(matches!(
            start_with(&bad_recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
        ));
        assert!(matches!(
            bad_recipe.pod.stop_with(true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
        ));
        assert!(matches!(
            pull_with(&bad_recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
        ));
        assert!(matches!(
            remove_with(&bad_recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
        ));
        assert!(matches!(
            stop_with(&bad_recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
        ));
        assert!(matches!(
            down_with(&bad_recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
        ));
        assert!(matches!(
            restart_with(&bad_recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
        ));
        assert!(matches!(
            reload_with(&bad_recipe, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidPodId && error.line == Some(7)
        ));
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
    }

    #[test]
    fn pod_and_pod_command_defaults_and_args() {
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

        assert_eq!(
            pod.build_create_args(),
            vec![
                "pod",
                "create",
                "--name",
                "si_po",
                "--publish",
                "127.0.0.1:42:21/TCP",
                "--publish",
                "127.0.0.1::42",
                "--unknown",
                "thing",
                "--another",
                "unknown",
            ]
        );
        assert_eq!(pod.build_stop_args(), vec!["pod", "stop", "si_po"]);
        assert_eq!(pod.build_remove_args(), vec!["pod", "rm", "si_po"]);

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert!(pod.create_with(true, &mut stdout, &mut stderr).is_ok());
        assert!(pod.stop_with(true, &mut stdout, &mut stderr).is_ok());
        assert!(pod.remove_with(true, &mut stdout, &mut stderr).is_ok());
    }

    #[test]
    fn pod_composite_lifecycles_up_down_restart_reload_pull_update() {
        let recipe = sample_multi_container_recipe();

        let mut up_down_out = Vec::new();
        let mut stderr = Vec::new();
        up_with(&recipe, true, &mut up_down_out, &mut stderr).unwrap();
        down_with(&recipe, true, &mut up_down_out, &mut stderr).unwrap();
        let up_down_lines: Vec<&str> = std::str::from_utf8(&up_down_out).unwrap().lines().collect();
        assert_eq!(up_down_lines.len(), 30);
        assert_eq!(up_down_lines[0], "Create pod 'Order Stack' ('order'): ");
        assert_eq!(up_down_lines[8], "Start container 'order-1': ");
        assert_eq!(up_down_lines[14], "Stop container 'three': ");
        assert_eq!(up_down_lines[28], "Remove pod 'Order Stack' ('order'): ");

        let mut restart_out = Vec::new();
        restart_with(&recipe, true, &mut restart_out, &mut stderr).unwrap();
        let restart_lines: Vec<&str> = std::str::from_utf8(&restart_out).unwrap().lines().collect();
        assert_eq!(
            restart_lines,
            vec![
                "Stop container 'three': ",
                "podman stop three",
                "Stop container 'order-two': ",
                "podman stop order-two",
                "Stop container 'order-1': ",
                "podman stop order-1",
                "Stop pod 'Order Stack' ('order'): ",
                "podman pod stop order",
                "Start container 'order-1': ",
                "podman start order-1",
                "Start container 'order-two': ",
                "podman start order-two",
                "Start container 'three': ",
                "podman start three",
            ]
        );

        let mut pull_out = Vec::new();
        pull_with(&recipe, true, &mut pull_out, &mut stderr).unwrap();
        let pull_lines: Vec<&str> = std::str::from_utf8(&pull_out).unwrap().lines().collect();
        assert_eq!(
            pull_lines,
            vec![
                "Update pod 'Order Stack' ('order') ...",
                "Update container 'order-1' ...",
                "podman pull obey.io/order:1",
                "Update container 'order-two' ...",
                "podman pull obey.io/order:2",
                "Update container 'three' ...",
                "podman pull custom.io/order:3",
            ]
        );

        let mut reload_out = Vec::new();
        reload_with(&recipe, true, &mut reload_out, &mut stderr).unwrap();
        let reload_lines: Vec<&str> = std::str::from_utf8(&reload_out).unwrap().lines().collect();
        assert_eq!(reload_lines.len(), 30);
        assert_eq!(reload_lines[0], "Stop container 'three': ");
        assert_eq!(reload_lines[14], "Remove pod 'Order Stack' ('order'): ");
        assert_eq!(reload_lines[16], "Create pod 'Order Stack' ('order'): ");
        assert_eq!(reload_lines[28], "Start container 'three': ");

        let mut update_out = Vec::new();
        update_with(&recipe, true, &mut update_out, &mut stderr).unwrap();
        let update_lines: Vec<&str> = std::str::from_utf8(&update_out).unwrap().lines().collect();
        assert_eq!(update_lines.len(), 37);
        assert_eq!(update_lines[0], "Update pod 'Order Stack' ('order') ...");
        assert_eq!(update_lines[7], "Stop container 'three': ");
        assert_eq!(update_lines[35], "Start container 'three': ");
        assert!(stderr.is_empty());

        // Exercise standard stream wrappers in simulate mode.
        assert!(create(&recipe, true).is_ok());
        assert!(start(&recipe, true).is_ok());
        assert!(stop(&recipe, true).is_ok());
        assert!(remove(&recipe, true).is_ok());
        assert!(up(&recipe, true).is_ok());
        assert!(down(&recipe, true).is_ok());
        assert!(restart(&recipe, true).is_ok());
        assert!(reload(&recipe, true).is_ok());
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
        assert_eq!(
            lines,
            vec![
                "Create pod 'Order Stack' ('order'): ",
                "podman pod create --name order --publish 127.0.0.1:42:21/TCP --publish 42:21/UDP --publish 42:21 --publish 127.0.0.1::42 --unknown thing --another unknown",
                "Create container 'order-1': ",
                "podman create --name order-1 --pod order obey.io/order:1",
                "Create container 'order-two': ",
                "podman create --name order-two --pod order obey.io/order:2",
                "Create container 'three': ",
                "podman create --name three --pod order custom.io/order:3",
                "Start container 'order-1': ",
                "podman start order-1",
                "Start container 'order-two': ",
                "podman start order-two",
                "Start container 'three': ",
                "podman start three",
                "Stop container 'three': ",
                "podman stop three",
                "Stop container 'order-two': ",
                "podman stop order-two",
                "Stop container 'order-1': ",
                "podman stop order-1",
                "Stop pod 'Order Stack' ('order'): ",
                "podman pod stop order",
                "Remove container 'three': ",
                "podman rm three",
                "Remove container 'order-two': ",
                "podman rm order-two",
                "Remove container 'order-1': ",
                "podman rm order-1",
                "Remove pod 'Order Stack' ('order'): ",
                "podman pod rm order",
            ]
        );
        assert!(stderr.is_empty());
    }

    #[test]
    fn pod_down_and_start_continue_when_containers_do_not_exist() {
        let mut recipe = sample_multi_container_recipe();
        recipe.pod.id = String::from("podoxide_missing_pod_999999");
        recipe.containers.truncate(2);
        recipe.containers[0].name = String::from("podoxide_missing_pod_999999-1");
        recipe.containers[1].name = String::from("podoxide_missing_pod_999999-2");

        let mut down_stdout = Vec::new();
        let mut down_stderr = Vec::new();
        down_with(&recipe, false, &mut down_stdout, &mut down_stderr).unwrap();

        let down_out_text = String::from_utf8(down_stdout).unwrap();
        assert!(down_out_text.contains("Stop container 'podoxide_missing_pod_999999-2': "));
        assert!(down_out_text.contains("Stop container 'podoxide_missing_pod_999999-1': "));
        assert!(
            down_out_text.contains("Stop pod 'Order Stack' ('podoxide_missing_pod_999999'): ")
        );
        assert!(down_out_text.contains("Remove container 'podoxide_missing_pod_999999-2': "));
        assert!(down_out_text.contains("Remove container 'podoxide_missing_pod_999999-1': "));
        assert!(
            down_out_text.contains("Remove pod 'Order Stack' ('podoxide_missing_pod_999999'): ")
        );

        let down_err_text = String::from_utf8(down_stderr).unwrap();
        assert_eq!(down_err_text.lines().count(), 6);
        assert!(
            down_err_text
                .lines()
                .all(|line| line.starts_with("WARNING: Command exited with code '"))
        );

        let mut start_stdout = Vec::new();
        let mut start_stderr = Vec::new();
        start_with(&recipe, false, &mut start_stdout, &mut start_stderr).unwrap();

        let start_out_text = String::from_utf8(start_stdout).unwrap();
        assert!(start_out_text.contains("Start container 'podoxide_missing_pod_999999-1': "));
        assert!(start_out_text.contains("Start container 'podoxide_missing_pod_999999-2': "));

        let start_err_text = String::from_utf8(start_stderr).unwrap();
        assert_eq!(start_err_text.lines().count(), 2);
        assert!(
            start_err_text
                .lines()
                .all(|line| line.starts_with("WARNING: Command exited with code '"))
        );
    }

    #[test]
    fn pod_lifecycles_reject_invalid_fields_up_front_without_partial_execution() {
        let base_recipe = sample_multi_container_recipe();
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();

        let mut bad_publish = base_recipe.clone();
        bad_publish.pod.line = 11;
        bad_publish.pod.publish = vec![String::from("--privileged")];
        assert_create_lifecycles_reject(&bad_publish, RecipeErrorKind::InvalidPublish, 11);

        let mut bad_option = base_recipe.clone();
        bad_option.pod.line = 13;
        bad_option.pod.options = vec![String::from("--flag\nspoof")];
        assert_create_lifecycles_reject(&bad_option, RecipeErrorKind::InvalidOption, 13);

        let mut bad_image = base_recipe.clone();
        bad_image.containers[1].line = 24;
        bad_image.containers[1].image = String::from("--help");
        assert_create_lifecycles_reject(&bad_image, RecipeErrorKind::InvalidImage, 24);
        assert!(matches!(
            pull_with(&bad_image, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidImage && error.line == Some(24)
        ));
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());

        let mut bad_name = base_recipe.clone();
        bad_name.containers[1].line = 25;
        bad_name.containers[1].name = String::from("--all");
        assert_create_lifecycles_reject(&bad_name, RecipeErrorKind::InvalidContainerName, 25);
        assert!(matches!(
            pull_with(&bad_name, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
        ));
        assert!(matches!(
            start_with(&bad_name, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
        ));
        assert!(matches!(
            stop_with(&bad_name, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
        ));
        assert!(matches!(
            remove_with(&bad_name, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
        ));
        assert!(matches!(
            down_with(&bad_name, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
        ));
        assert!(matches!(
            restart_with(&bad_name, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(25)
        ));
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());

        let mut empty_name = base_recipe.clone();
        empty_name.containers[1].line = 26;
        empty_name.containers[1].name.clear();
        assert_create_lifecycles_reject(&empty_name, RecipeErrorKind::InvalidContainerName, 26);
        assert!(matches!(
            pull_with(&empty_name, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidContainerName && error.line == Some(26)
        ));
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());

        let mut bad_registry = base_recipe.clone();
        bad_registry.pod.line = 12;
        bad_registry.pod.registry = String::from("--tls-verify=false");
        assert_create_lifecycles_reject(&bad_registry, RecipeErrorKind::InvalidRegistry, 12);
        assert!(matches!(
            pull_with(&bad_registry, true, &mut stdout, &mut stderr),
            Err(PodError::Recipe(error))
                if error.kind == RecipeErrorKind::InvalidRegistry && error.line == Some(12)
        ));
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());

        let mut bad_volume_path = base_recipe;
        bad_volume_path.pod.path = PathBuf::new();
        bad_volume_path.containers[1].line = 27;
        bad_volume_path.containers[1].volumes.push(VolumeMount {
            container_dir: String::from("/data"),
            host_dir: String::from("./data"),
            options: String::new(),
        });
        assert_create_lifecycles_reject(&bad_volume_path, RecipeErrorKind::InvalidVolume, 27);
    }

    #[test]
    fn pod_pull_with_sanitizes_name_and_tolerates_broken_pipe() {
        let recipe = sample_multi_container_recipe();
        let mut broken_stdout = BrokenPipeWriter;
        let mut stderr = Vec::new();

        assert!(recipe.pod.create_with(true, &mut broken_stdout, &mut stderr).is_ok());
        assert!(start_with(&recipe, true, &mut broken_stdout, &mut stderr).is_ok());
        assert!(recipe.pod.stop_with(true, &mut broken_stdout, &mut stderr).is_ok());
        assert!(recipe.pod.remove_with(true, &mut broken_stdout, &mut stderr).is_ok());
        assert!(pull_with(&recipe, true, &mut broken_stdout, &mut stderr).is_ok());
        assert!(update_with(&recipe, true, &mut broken_stdout, &mut stderr).is_ok());

        let mut injected = recipe;
        injected.pod.name = format!(
            "Bad\nINFO: Spoof\r\x1b[31m\0\u{009b}\u{202E}{}",
            "ä".repeat(MAX_LOG_MESSAGE_BYTES)
        );
        let mut stdout = Vec::new();
        pull_with(&injected, true, &mut stdout, &mut stderr).unwrap();
        let output = String::from_utf8(stdout).unwrap();
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 7);
        let first_line = lines[0];
        assert!(first_line.starts_with("Update pod 'Bad INFO: Spoof"));
        assert!(first_line.len() <= MAX_LOG_MESSAGE_BYTES);
        assert!(!first_line.contains('\r'));
        assert!(!first_line.contains('\x1b'));
        assert!(!first_line.contains('\0'));
        assert!(!first_line.contains('\u{009b}'));
        assert!(!first_line.contains('\u{202E}'));
    }

    #[test]
    fn pod_validation_rejects_malformed_id_publish_and_options() {
        let recipe = sample_multi_container_recipe();
        let base_pod = recipe.pod.clone();

        for invalid_id in ["", "--all", "-f", "bad\nname", "bad\u{202E}name"] {
            assert_invalid_pod_id_rejected(&recipe, invalid_id);
        }

        for invalid_publish in ["", "--privileged", "-p", "80:80\nspoof", "80:80\u{202E}"] {
            let mut bad_pod = base_pod.clone();
            bad_pod.line = 11;
            bad_pod.publish = vec![String::from(invalid_publish)];
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            assert!(matches!(
                bad_pod.create_with(true, &mut stdout, &mut stderr),
                Err(PodError::Recipe(error))
                    if error.kind == RecipeErrorKind::InvalidPublish && error.line == Some(11)
            ));
            assert!(stdout.is_empty());
        }

        for invalid_option in ["", "   ", "--flag\nspoof", "--flag\u{202E}spoof", "--flag\0spoof"] {
            let mut bad_pod = base_pod.clone();
            bad_pod.line = 13;
            bad_pod.options = vec![String::from(invalid_option)];
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            assert!(matches!(
                bad_pod.create_with(true, &mut stdout, &mut stderr),
                Err(PodError::Recipe(error))
                    if error.kind == RecipeErrorKind::InvalidOption && error.line == Some(13)
            ));
            assert!(stdout.is_empty());
        }
    }

    fn sample_multi_container_recipe() -> Recipe {
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
}
