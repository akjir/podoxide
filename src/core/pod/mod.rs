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

mod lifecycle;
mod update;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use crate::core::recipe::validation as recipe_validation;
use crate::error::{PodError, RecipeError};
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::{self, CreatePod};
pub use self::lifecycle::{
    create, create_with, down, down_with, reload, reload_with, remove, remove_with, restart,
    restart_with, start, start_with, stop, stop_with, up, up_with,
};
pub use self::update::{pull, pull_with, update, update_with};
pub(crate) use self::update::{pull_with_cache, update_with_cache};

/// Pod configuration and custom command definitions from the `pod` block of a `.pod` recipe.
#[derive(Clone, Debug, Default)]
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
    created: Cell<bool>,
}

impl Pod {
    /// Creates a new [`Pod`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
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
    ) -> Result<ExecOutcome, PodError> {
        self.validate_create()?;
        self.create_validated(simulate, stdout, stderr)
    }

    /// Ensures this pod exists in Podman by checking `pod_exists` and calling [`Self::create_with`] when missing.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if pod validation, stream writing, or Podman execution fails.
    pub fn ensure_created_with<W, E, F>(
        &self,
        simulate: bool,
        pod_exists: F,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<ExecOutcome, PodError>
    where
        W: Write,
        E: Write,
        F: FnOnce(&str) -> bool,
    {
        self.validate_create()?;
        self.ensure_created_validated(simulate, pod_exists, stdout, stderr)
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
    ) -> Result<ExecOutcome, PodError> {
        lifecycle::validate_pod_id(self)?;
        let prefix = format!("Remove pod '{}' ('{}'): ", self.name, self.id);
        let outcome = podman::remove_pod(&self.id, &prefix, simulate, stdout, stderr)?;
        self.created.set(false);
        Ok(outcome)
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
    ) -> Result<ExecOutcome, PodError> {
        lifecycle::validate_pod_id(self)?;
        let prefix = format!("Stop pod '{}' ('{}'): ", self.name, self.id);
        podman::stop_pod(&self.id, &prefix, simulate, stdout, stderr)
    }

    /// Validates `self.id`, `self.publish`, and `self.options` before executing `podman pod create`.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] when `self.id`, any `publish` entry, or any `option` value is invalid.
    pub fn validate_create(&self) -> Result<(), RecipeError> {
        lifecycle::validate_pod_id(self)?;
        recipe_validation::validate_pod_publish(&self.publish, self.line)?;
        recipe_validation::validate_pod_options(&self.options, self.line)
    }

    /// Executes `podman pod create` assuming [`Self::validate_create`] has already succeeded.
    pub(crate) fn create_validated<W: Write, E: Write>(
        &self,
        simulate: bool,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<ExecOutcome, PodError> {
        let prefix = format!("Create pod '{}' ('{}'): ", self.name, self.id);
        let outcome = podman::create_pod(
            &CreatePod {
                options: &self.options,
                pod_name: &self.id,
                publish: &self.publish,
            },
            &prefix,
            simulate,
            stdout,
            stderr,
        )?;
        if outcome == ExecOutcome::Succeeded {
            self.created.set(true);
        }
        Ok(outcome)
    }

    /// Ensures this pod exists via `pod_exists`, skipping redundant validation when already checked.
    pub(crate) fn ensure_created_validated<W, E, F>(
        &self,
        simulate: bool,
        pod_exists: F,
        stdout: &mut W,
        stderr: &mut E,
    ) -> Result<ExecOutcome, PodError>
    where
        W: Write,
        E: Write,
        F: FnOnce(&str) -> bool,
    {
        if self.created.get() {
            return Ok(ExecOutcome::Succeeded);
        }
        if pod_exists(&self.id) {
            self.created.set(true);
            return Ok(ExecOutcome::Succeeded);
        }
        self.create_validated(simulate, stdout, stderr)
    }
}

impl Eq for Pod {}

impl PartialEq for Pod {
    fn eq(&self, other: &Self) -> bool {
        self.commands == other.commands
            && self.description == other.description
            && self.id == other.id
            && self.line == other.line
            && self.name == other.name
            && self.options == other.options
            && self.path == other.path
            && self.publish == other.publish
            && self.registry == other.registry
    }
}

/// Custom command definition from a `command <name>` sub-block inside a `pod` block.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PodCommand {
    pub container: String,
    pub description: String,
    pub execute: String,
    pub line: usize,
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

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod update_tests;
#[cfg(test)]
mod validation_tests;
