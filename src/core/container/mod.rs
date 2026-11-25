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

//! Container domain models (`container` blocks and `volume` mounts in `.pod` recipes) and Podman container lifecycle execution.

mod lifecycle;
mod update;
pub(crate) mod validation;
mod volume;

use crate::core::pod::Pod;
use crate::error::RecipeError;
pub(crate) use self::lifecycle::create_validated;
pub use self::lifecycle::{
    create, create_with, down, down_with, reload, reload_with, remove, remove_with, restart,
    restart_with, start, start_with, stop, stop_with, up, up_with,
};
pub use self::update::{pull, pull_with, resolve_image_reference, update, update_with};
pub(crate) use self::update::{detect_stale_container_image, pull_with_cache, update_with_cache};
pub use self::validation::resolve_container_name_value;
pub use self::volume::{format_volume_mount, is_valid_volume_mount};

/// Container specification and runtime arguments from a `container` block in a `.pod` recipe.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Container {
    pub commands: Vec<String>,
    pub image: String,
    pub line: usize,
    pub name: String,
    pub options: Vec<String>,
    pub registry: String,
    pub restart: String,
    pub volumes: Vec<VolumeMount>,
}

impl Container {
    /// Creates a new [`Container`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves and normalizes the container name using `pod_id` and its 1-based `index`.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] when the configured container name is invalid.
    pub fn ensure_name(&mut self, pod_id: &str, index: usize) -> Result<(), RecipeError> {
        self.name = resolve_container_name_value(&self.name, pod_id, index, self.line)?;
        Ok(())
    }

    /// Validates that container fields (`name`, `image`, `registry`, `restart`, `options`, `commands`, and `volumes`) are well-formed.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] when `image` is missing or any container field is invalid.
    pub fn validate(&self) -> Result<(), RecipeError> {
        validation::validate_container(self)
    }

    /// Validates that this container and its parent `pod` satisfy all preconditions for `podman create`.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] when the container name, pod ID, pod publish/option entries, pod registry, container fields, or relative volume base directory are invalid.
    pub fn validate_create(&self, pod: &Pod) -> Result<(), RecipeError> {
        validation::validate_container_create(self, pod)
    }

    /// Validates this container against a parent `pod` whose [`Pod::validate_create`] has already passed.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeError`] when container fields, `pod.registry`, or relative volume host directory are invalid.
    pub(crate) fn validate_create_for_validated_pod(&self, pod: &Pod) -> Result<(), RecipeError> {
        validation::validate_container_create_for_validated_pod(self, pod)
    }
}

/// Host-to-container volume mount specification from a `volume` directive inside a `container` block.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VolumeMount {
    pub container_dir: String,
    pub host_dir: String,
    pub options: String,
}

impl VolumeMount {
    /// Creates a new [`VolumeMount`] with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod lifecycle_tests;
#[cfg(test)]
mod validation_tests;
#[cfg(test)]
mod volume_tests;
