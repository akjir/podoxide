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

//! External process and Podman CLI execution modules.

pub mod command;
pub mod podman;

pub use self::command::exec_editor_with;
pub use self::podman::{
    CreateContainer, CreatePod, ExecContainer, PodLogs, SHELL_FALLBACK_COMMAND, check_version,
    container_running, create_container, create_pod, exec_container, is_running_container_status,
    is_supported_version, logs_pod, pod_exists, prune_images, pull_image, query_images,
    query_in_use_images, query_ps_status, remove_container, remove_pod, start_container,
    stop_container, stop_pod,
};
