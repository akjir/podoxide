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

//! Handler for `pods status [targets...]`.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

use crate::core::config::Config;
use crate::core::context::Context;
use crate::core::log::{self, MAX_LOG_MESSAGE_BYTES};
use crate::core::recipe::Recipe;
use crate::error::PodError;
use crate::io::exec::podman;
use crate::utilities::collection::{MAX_COLLECTION_PREALLOC, bounded_capacity};
use crate::utilities::format::format_recipe_error_hint;
use crate::utilities::string::clamp_utf8_prefix;

const COLUMN_SEPARATOR: &str = "   ";
const DEFAULT_COLUMN_COUNT: usize = 7;
const DEFAULT_COLUMN_HEADERS: [&str; DEFAULT_COLUMN_COUNT] = [
    "ID", "POD", "NAME", "STATUS", "RESTARTS", "CREATED", "IMAGE",
];
const FULL_COLUMN_COUNT: usize = 10;
const FULL_COLUMN_HEADERS: [&str; FULL_COLUMN_COUNT] = [
    "ID",
    "POD",
    "NAME",
    "TARGET NAME",
    "STATUS",
    "RESTARTS",
    "CREATED",
    "IMAGE",
    "COMMAND",
    "PORTS",
];
const INITIAL_STATUS_CAPACITY: usize = 16;
const MAX_STATUS_RECORDS: usize = 1024;
const MISSING_STATUS: &str = "Not Found";
const MISSING_VALUE: &str = "-";
const RECIPE_ERROR_STATUS: &str = "Recipe Error";

/// Expected metadata for a managed container declared in a configured `.pod` recipe.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ExpectedContainer {
    pod_id: String,
    target_name: String,
}

/// Resolved expected container metadata paired with sorted invalid recipe names.
type ExpectedTargets = (BTreeMap<String, ExpectedContainer>, Vec<String>);

/// Single container row in the rendered runtime status table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StatusRow<'a> {
    command: &'a str,
    created: &'a str,
    id: &'a str,
    image: &'a str,
    name: &'a str,
    pod: &'a str,
    ports: &'a str,
    restarts: &'a str,
    status: &'a str,
    target_name: &'a str,
}

impl<'a> StatusRow<'a> {
    /// Returns the seven default column values in display order.
    #[must_use]
    const fn default_cells(&self) -> [&'a str; DEFAULT_COLUMN_COUNT] {
        [
            self.id,
            self.pod,
            self.name,
            self.status,
            self.restarts,
            self.created,
            self.image,
        ]
    }

    /// Returns all ten `--full` column values in display order.
    #[must_use]
    const fn full_cells(&self) -> [&'a str; FULL_COLUMN_COUNT] {
        [
            self.id,
            self.pod,
            self.name,
            self.target_name,
            self.status,
            self.restarts,
            self.created,
            self.image,
            self.command,
            self.ports,
        ]
    }

    /// Creates a synthesized `"Not Found"` [`StatusRow`] for a missing managed container.
    #[must_use]
    const fn missing(name: &'a str, pod: &'a str, target_name: &'a str) -> Self {
        Self {
            command: MISSING_VALUE,
            created: MISSING_VALUE,
            id: MISSING_VALUE,
            image: MISSING_VALUE,
            name,
            pod,
            ports: MISSING_VALUE,
            restarts: MISSING_VALUE,
            status: MISSING_STATUS,
            target_name,
        }
    }

    /// Creates a synthesized `"Recipe Error"` [`StatusRow`] for a missing or invalid `.pod` recipe.
    #[must_use]
    const fn recipe_error(name: &'a str) -> Self {
        Self {
            command: MISSING_VALUE,
            created: MISSING_VALUE,
            id: MISSING_VALUE,
            image: MISSING_VALUE,
            name,
            pod: MISSING_VALUE,
            ports: MISSING_VALUE,
            restarts: MISSING_VALUE,
            status: RECIPE_ERROR_STATUS,
            target_name: name,
        }
    }
}

/// Queries `podman ps -a`, cross-references managed recipes, and formats the runtime status
/// table.
///
/// # Workflow
///
/// 1. Resolve the expected managed containers to inspect:
///    - If `context.parameters` contains `<recipe>/<container>` targets, resolve only the
///      targeted containers and their `<recipe>/<index>` target names.
///    - If `context.parameters` contains `@<group>` or `<recipe>` targets, expand and
///      deduplicate them against `context.config.recipes.groups` and inspect each recipe via
///      [`Recipe::inspect`].
///    - If `context.parameters` is empty, collect and inspect all unique configured recipes from
///      `context.config.recipes.groups`.
/// 2. Query `podman ps -a --format ...` to retrieve all host container records.
/// 3. Filter or merge container records (`--all` includes unmanaged host containers; missing
///    managed containers are synthesized with status `"Not Found"`; missing or invalid recipes
///    are synthesized with status `"Recipe Error"`).
/// 4. Sort rows by `(pod, name)` and print the aligned status table (`--full` inserts
///    `TARGET NAME` after `NAME` and appends `COMMAND` and `PORTS` columns), followed by a
///    `pods recipe inspect` hint when any targeted recipe failed inspection.
///
/// # Errors
///
/// Returns [`PodError`] if target expansion or container resolution fails or `podman ps` fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let mut stdout = io::stdout().lock();
    let mut query_status = podman::query_ps_status;
    execute_with(context, &mut query_status, &mut stdout)
}

/// Appends synthesized `"Not Found"` rows for expected containers absent from `found_names`.
fn append_missing_rows<'a>(
    expected: &'a BTreeMap<String, ExpectedContainer>,
    found_names: &BTreeSet<&str>,
    display_rows: &mut Vec<StatusRow<'a>>,
) {
    for (expected_name, container) in expected {
        if display_rows.len() >= MAX_STATUS_RECORDS {
            break;
        }
        if !found_names.contains(expected_name.as_str()) {
            display_rows.push(StatusRow::missing(
                expected_name.as_str(),
                container.pod_id.as_str(),
                container.target_name.as_str(),
            ));
        }
    }
}

/// Appends synthesized `"Recipe Error"` rows for missing or invalid recipes in `invalid_recipes`.
fn append_recipe_error_rows<'a>(
    invalid_recipes: &'a [String],
    display_rows: &mut Vec<StatusRow<'a>>,
) {
    for recipe_name in invalid_recipes {
        if display_rows.len() >= MAX_STATUS_RECORDS {
            break;
        }
        display_rows.push(StatusRow::recipe_error(recipe_name.as_str()));
    }
}

/// Filters or merges `podman ps` output with `expected` managed containers and `invalid_recipes`, sorting the result by `(pod, name)`.
#[must_use]
fn build_display_rows<'a>(
    ps_output: &'a str,
    expected: &'a BTreeMap<String, ExpectedContainer>,
    invalid_recipes: &'a [String],
    include_all: bool,
) -> Vec<StatusRow<'a>> {
    let base_count = expected.len().saturating_add(invalid_recipes.len());
    let estimated = if include_all {
        base_count.max(INITIAL_STATUS_CAPACITY)
    } else {
        base_count
    };
    let capacity = bounded_capacity(estimated, MAX_COLLECTION_PREALLOC);
    let mut display_rows = Vec::with_capacity(capacity);
    let mut found_names = BTreeSet::new();

    for line in ps_output.lines().take(MAX_STATUS_RECORDS) {
        if display_rows.len() >= MAX_STATUS_RECORDS {
            break;
        }
        let Some(mut row) = parse_status_record(line) else {
            continue;
        };
        if let Some(expected_container) = expected.get(row.name) {
            row.target_name = expected_container.target_name.as_str();
            found_names.insert(row.name);
            display_rows.push(row);
        } else if include_all {
            display_rows.push(row);
        }
    }

    append_missing_rows(expected, &found_names, &mut display_rows);
    append_recipe_error_rows(invalid_recipes, &mut display_rows);
    display_rows.sort_unstable_by(|left, right| {
        left.pod
            .cmp(right.pod)
            .then_with(|| left.name.cmp(right.name))
    });
    display_rows
}

/// Inspects each target recipe in `targets` to map expected container names to [`ExpectedContainer`] metadata and collect invalid recipe names.
#[must_use]
fn collect_expected_containers(config: &Config, targets: &[Cow<'_, str>]) -> ExpectedTargets {
    let mut expected = BTreeMap::new();
    let mut invalid_set = BTreeSet::new();
    for target in targets {
        if expected.len().saturating_add(invalid_set.len()) >= MAX_STATUS_RECORDS {
            break;
        }
        insert_recipe_containers(config, target.as_ref(), &mut expected, &mut invalid_set);
    }
    let invalid_recipes = invalid_set.into_iter().collect();
    (expected, invalid_recipes)
}

/// Resolves explicit `<recipe>/<container>` CLI parameters into a map of expected container metadata.
fn collect_targeted_containers(context: &Context) -> Result<ExpectedTargets, PodError> {
    let specs = super::collect_container_specs(context)?;
    let (recipes, resolved) = super::resolve_container_targets(&context.config, &specs)?;
    let mut expected = BTreeMap::new();
    for (recipe_name, container_name) in resolved {
        if expected.len() >= MAX_STATUS_RECORDS {
            break;
        }
        insert_targeted_container(&recipes, recipe_name, container_name, &mut expected);
    }
    Ok((expected, Vec::new()))
}

/// Computes the maximum visible character width for each active column across headers and `rows`.
#[must_use]
fn compute_column_widths(rows: &[StatusRow<'_>], is_full: bool) -> [usize; FULL_COLUMN_COUNT] {
    let mut widths = [0usize; FULL_COLUMN_COUNT];
    if is_full {
        update_column_widths(&mut widths, &FULL_COLUMN_HEADERS);
        for row in rows {
            update_column_widths(&mut widths, &row.full_cells());
        }
        return widths;
    }
    let active = &mut widths[..DEFAULT_COLUMN_COUNT];
    update_column_widths(active, &DEFAULT_COLUMN_HEADERS);
    for row in rows {
        update_column_widths(active, &row.default_cells());
    }
    widths
}

/// Orchestrates `pods status` target resolution, Podman querying, recipe cross-referencing, and table rendering.
fn execute_with<W, F>(
    context: &Context,
    query_status: &mut F,
    stdout: &mut W,
) -> Result<(), PodError>
where
    W: Write,
    F: FnMut() -> Option<String>,
{
    let (expected, invalid_recipes) = resolve_expected_containers(context)?;
    let ps_output = query_podman_status(query_status)?;
    let display_rows = build_display_rows(
        &ps_output,
        &expected,
        &invalid_recipes,
        context.flags.all,
    );
    if display_rows.is_empty() {
        log::write_print(stdout, "No containers found.");
        return Ok(());
    }
    write_status_table(&display_rows, context.flags.full, stdout);
    write_invalid_recipes_hint(&invalid_recipes, stdout);
    Ok(())
}

/// Formats a slice of table `cells` padded to `widths` and separated by [`COLUMN_SEPARATOR`].
#[must_use]
fn format_table_row(cells: &[&str], widths: &[usize]) -> String {
    let separator_bytes = cells
        .len()
        .saturating_sub(1)
        .saturating_mul(COLUMN_SEPARATOR.len());
    let total_width = widths.iter().copied().fold(0usize, usize::saturating_add);
    let capacity = total_width
        .saturating_add(separator_bytes)
        .min(MAX_LOG_MESSAGE_BYTES);
    let mut line = String::with_capacity(capacity);
    for (i, (&cell, &width)) in cells.iter().zip(widths.iter()).enumerate() {
        if i > 0 {
            line.push_str(COLUMN_SEPARATOR);
        }
        push_padded_cell(&mut line, cell, width);
    }
    line
}

/// Inspects `target` via [`Recipe::inspect`] and inserts its container names into `expected`, or records `target` in `invalid_recipes` on failure.
fn insert_recipe_containers(
    config: &Config,
    target: &str,
    expected: &mut BTreeMap<String, ExpectedContainer>,
    invalid_recipes: &mut BTreeSet<String>,
) {
    let Ok(recipe) = Recipe::inspect(config, target) else {
        invalid_recipes.insert(target.to_owned());
        return;
    };
    let pod_id = &recipe.pod.id;
    for (index, container) in recipe.containers.into_iter().enumerate() {
        if expected.len() >= MAX_STATUS_RECORDS {
            break;
        }
        let target_name = format!("{target}/{}", index + 1);
        expected.insert(
            container.name,
            ExpectedContainer {
                pod_id: pod_id.clone(),
                target_name,
            },
        );
    }
}

/// Inserts a resolved `(recipe_name, container_name)` target into `expected` with its 1-based `<recipe>/<index>` target name.
fn insert_targeted_container(
    recipes: &BTreeMap<&str, Recipe>,
    recipe_name: &str,
    container_name: String,
    expected: &mut BTreeMap<String, ExpectedContainer>,
) {
    let Some(recipe) = recipes.get(recipe_name) else {
        return;
    };
    let Some(index) = recipe
        .containers
        .iter()
        .position(|container| container.name == container_name)
    else {
        return;
    };
    let target_name = format!("{recipe_name}/{}", index + 1);
    expected.insert(
        container_name,
        ExpectedContainer {
            pod_id: recipe.pod.id.clone(),
            target_name,
        },
    );
}

/// Parses a `;;;`-delimited `podman ps -a --format` output line into a [`StatusRow`].
#[must_use]
fn parse_status_record(line: &str) -> Option<StatusRow<'_>> {
    if line.is_empty() {
        return None;
    }
    let (id, rest) = line.split_once(";;;")?;
    let (image, rest) = rest.split_once(";;;")?;
    let (rest, restarts) = rest.rsplit_once(";;;")?;
    let (rest, pod) = rest.rsplit_once(";;;")?;
    let (rest, name) = rest.rsplit_once(";;;")?;
    let (rest, ports) = rest.rsplit_once(";;;")?;
    let (rest, status) = rest.rsplit_once(";;;")?;
    let (command, created) = rest.rsplit_once(";;;")?;
    Some(StatusRow {
        command,
        created,
        id,
        image,
        name,
        pod,
        ports,
        restarts,
        status,
        target_name: MISSING_VALUE,
    })
}

/// Appends `value` right-padded with ASCII spaces to `width` visible characters into `buffer`.
fn push_padded_cell(buffer: &mut String, value: &str, width: usize) {
    let clamped = clamp_utf8_prefix(value, MAX_LOG_MESSAGE_BYTES);
    buffer.push_str(clamped);
    let char_len = clamped.chars().count();
    let clamped_width = width.min(MAX_LOG_MESSAGE_BYTES);
    let pad = clamped_width.saturating_sub(char_len);
    if pad > 0 {
        buffer.push_str(&" ".repeat(pad));
    }
}

/// Queries `podman ps -a` status output via `query_status` and returns the raw output.
fn query_podman_status<F>(query_status: &mut F) -> Result<String, PodError>
where
    F: FnMut() -> Option<String>,
{
    query_status().ok_or_else(|| PodError::from(io::Error::other("Failed to query podman status.")))
}

/// Resolves expected managed containers and invalid recipes from `<recipe>/<container>` targets, `@<group>`/`<recipe>` targets, or all configured recipes.
fn resolve_expected_containers(context: &Context) -> Result<ExpectedTargets, PodError> {
    if context
        .parameters
        .iter()
        .any(|parameter| parameter.contains('/'))
    {
        return collect_targeted_containers(context);
    }
    let targets = resolve_targets(context)?;
    Ok(collect_expected_containers(&context.config, &targets))
}

/// Resolves target recipe names from `context.parameters` or all configured recipes in `context.config`.
fn resolve_targets(context: &Context) -> Result<Vec<Cow<'_, str>>, PodError> {
    if !context.parameters.is_empty() {
        let expanded = context.config.expand_targets(&context.parameters)?;
        let capacity = bounded_capacity(expanded.len(), MAX_COLLECTION_PREALLOC);
        let mut targets = Vec::with_capacity(capacity);
        for target in expanded {
            targets.push(Cow::Owned(target));
        }
        return Ok(targets);
    }
    let recipes = context.config.get_recipes();
    let capacity = bounded_capacity(recipes.len(), MAX_COLLECTION_PREALLOC);
    let mut targets = Vec::with_capacity(capacity);
    for recipe in recipes {
        targets.push(Cow::Borrowed(recipe));
    }
    Ok(targets)
}

/// Updates each entry in `widths` with the maximum of its current value and the visible character count of the corresponding cell in `cells`.
fn update_column_widths(widths: &mut [usize], cells: &[&str]) {
    for (width, cell) in widths.iter_mut().zip(cells.iter()) {
        let char_len = clamp_utf8_prefix(cell, MAX_LOG_MESSAGE_BYTES)
            .chars()
            .count();
        if char_len > *width {
            *width = char_len;
        }
    }
}

/// Writes a blank line and the `pods recipe inspect` hint when `invalid_recipes` is non-empty.
fn write_invalid_recipes_hint<W: Write>(invalid_recipes: &[String], stdout: &mut W) {
    if invalid_recipes.is_empty() {
        return;
    }
    let capacity = bounded_capacity(invalid_recipes.len(), MAX_COLLECTION_PREALLOC);
    let mut refs = Vec::with_capacity(capacity);
    for name in invalid_recipes {
        refs.push(name.as_str());
    }
    log::write_print(stdout, "");
    log::write_print(stdout, &format_recipe_error_hint(&refs));
}

/// Formats and writes the aligned status header and container rows to `stdout`.
fn write_status_table<W: Write>(rows: &[StatusRow<'_>], is_full: bool, stdout: &mut W) {
    let widths = compute_column_widths(rows, is_full);
    if is_full {
        let header_line = format_table_row(&FULL_COLUMN_HEADERS, &widths);
        log::write_print(stdout, &header_line);
        for row in rows {
            let row_line = format_table_row(&row.full_cells(), &widths);
            log::write_print(stdout, &row_line);
        }
        return;
    }
    let active_widths = &widths[..DEFAULT_COLUMN_COUNT];
    let header_line = format_table_row(&DEFAULT_COLUMN_HEADERS, active_widths);
    log::write_print(stdout, &header_line);
    for row in rows {
        let row_line = format_table_row(&row.default_cells(), active_widths);
        log::write_print(stdout, &row_line);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::env;
    use std::fs as std_fs;
    use std::path::PathBuf;
    use std::process;

    use crate::core::config::Config;
    use crate::core::context::Context;
    use crate::core::log::MAX_LOG_MESSAGE_BYTES;
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError, RecipeErrorKind};
    use crate::io::system::fs;
    use super::{
        ExpectedContainer, MAX_STATUS_RECORDS, build_display_rows, collect_expected_containers,
        execute_with, format_table_row, handle, parse_status_record, update_column_widths,
    };

    const MOCK_PS_OUTPUT: &str = "\
a1b2c3d4e5f6;;;nginx:alpine;;;nginx -g 'daemon off;';;;2 hours ago;;;Up 2 hours (healthy);;;0.0.0.0:8080->80/tcp;;;test_simple_cnt_pod-app-1;;;test_simple_cnt_pod;;;0\n\
b2c3d4e5f6a1;;;alpine:latest;;;/bin/sh;;;5 mins ago;;;Up 5 mins;;;;;;test_pod-1;;;test_pod;;;1\n\
c3d4e5f6a1b2;;;busybox;;;sh;;;1 day ago;;;Exited (0) 1 day ago;;;;;;unmanaged_container;;;;;;0\n";

    const NAMING_RECIPE_PSL: &str = "\
pod test_pod {
    name     \"Test Pod\"
    registry docker.io
}
container {
    image \"alpine:latest\"
}
container \"*my-db\" {
    image \"postgres:15\"
}
container totally_custom {
    image \"redis:alpine\"
}
";

    const SIMPLE_RECIPE_PSL: &str = "\
pod test_simple_cnt_pod {
    name     \"Simple Pod\"
    registry docker.io
}
container \"*app-1\" {
    image \"nginx:alpine\"
}
";

    fn create_status_workspace(suffix: &str) -> (PathBuf, Config) {
        let temp_dir =
            env::temp_dir().join(format!("podoxide_status_test_{}_{suffix}", process::id()));
        let _ = std_fs::remove_dir_all(&temp_dir);
        let pods_dir = temp_dir.join("pods");
        let recipes_dir = temp_dir.join("recipes");
        std_fs::create_dir_all(&pods_dir).unwrap();
        std_fs::create_dir_all(&recipes_dir).unwrap();

        fs::write_file(
            &recipes_dir.join("recipe_011_simple_container.pod"),
            SIMPLE_RECIPE_PSL,
        )
        .unwrap();
        fs::write_file(
            &recipes_dir.join("recipe_017_container_naming.pod"),
            NAMING_RECIPE_PSL,
        )
        .unwrap();
        fs::write_file(&recipes_dir.join("invalid_recipe.pod"), "invalid {{{ psl").unwrap();

        let mut config = Config::new();
        config.pods.path = pods_dir;
        config.recipes.path = recipes_dir;
        config.recipes.groups.insert(
            String::from("all"),
            vec![
                String::from("recipe_011_simple_container"),
                String::from("recipe_017_container_naming"),
                String::from("missing_recipe"),
                String::from("invalid_recipe"),
            ],
        );
        (temp_dir, config)
    }

    #[test]
    fn execute_with_filters_explicit_container_targets_and_rejects_invalid_specs() {
        let (temp_dir, config) = create_status_workspace("container_targets");
        let mut context = Context::new();
        context.config = config;
        context.flags.full = true;
        context.parameters = vec![
            String::from("recipe_017_container_naming/*my-db"),
            String::from("recipe_017_container_naming/1"),
            String::from("recipe_017_container_naming/my-db"),
            String::from("recipe_011_simple_container/app-1"),
        ];

        let mut capture = || Some(String::from(MOCK_PS_OUTPUT));
        let mut stdout = Vec::new();
        execute_with(&context, &mut capture, &mut stdout).unwrap();
        let output = String::from_utf8(stdout).unwrap();

        // Only targeted containers are rendered, with their 1-based `<recipe>/<index>` TARGET NAME.
        assert!(output.contains("TARGET NAME"));
        assert!(output.contains("recipe_017_container_naming/1"));
        assert!(output.contains("recipe_017_container_naming/2"));
        assert!(output.contains("recipe_011_simple_container/1"));
        assert!(!output.contains("totally_custom"));
        assert!(!output.contains("recipe_017_container_naming/3"));

        // Unknown container in a configured recipe fails before querying podman.
        context.parameters = vec![String::from("recipe_017_container_naming/nonexistent")];
        let mut never_capture =
            || -> Option<String> { panic!("podman ps should not be called") };
        let error = execute_with(&context, &mut never_capture, &mut Vec::new()).unwrap_err();
        match error {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotFound);
            }
            other => panic!("expected PodError::Recipe(ContainerNotFound), got {other:?}"),
        }

        // Mixed container and pod targets fail before querying podman.
        context.parameters = vec![
            String::from("recipe_017_container_naming/1"),
            String::from("recipe_011_simple_container"),
        ];
        let mixed_error = execute_with(&context, &mut never_capture, &mut Vec::new()).unwrap_err();
        match mixed_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::InvalidTarget),
            other => panic!("expected PodError::Cli(InvalidTarget), got {other:?}"),
        }

        context.parameters = vec![
            String::from("recipe_011_simple_container"),
            String::from("recipe_017_container_naming/1"),
        ];
        let pod_first_mixed_error =
            execute_with(&context, &mut never_capture, &mut Vec::new()).unwrap_err();
        match pod_first_mixed_error {
            PodError::Cli(error) => assert_eq!(error.kind, CliErrorKind::InvalidTarget),
            other => panic!("expected PodError::Cli(InvalidTarget), got {other:?}"),
        }

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn execute_with_handles_groups_empty_results_bounds_and_errors() {
        let (temp_dir, config) = create_status_workspace("bounds_and_errors");
        let mut context = Context::new();
        context.config = config;

        // 1. Unknown recipe or group target fails before querying podman.
        context.parameters = vec![String::from("unknown_recipe")];
        assert!(handle(&context).is_err());
        let mut called = false;
        let mut never_capture = || -> Option<String> {
            called = true;
            None
        };
        let target_error = execute_with(&context, &mut never_capture, &mut Vec::new()).unwrap_err();
        assert!(!called);
        match target_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }

        // 2. Group target (@all) resolves and renders expected containers and Recipe Error rows.
        context.parameters = vec![String::from("@all")];
        let mut group_stdout = Vec::new();
        let mut mock_capture = || Some(String::from(MOCK_PS_OUTPUT));
        execute_with(&context, &mut mock_capture, &mut group_stdout).unwrap();
        let group_output = String::from_utf8(group_stdout).unwrap();
        assert!(group_output.contains("test_simple_cnt_pod-app-1"));
        assert!(group_output.contains("test_pod-my-db"));
        assert!(group_output.contains("invalid_recipe"));
        assert!(group_output.contains("Recipe Error"));

        // 2b. Single invalid recipe target renders Recipe Error row and singular hint.
        context.parameters = vec![String::from("invalid_recipe")];
        let mut invalid_stdout = Vec::new();
        execute_with(&context, &mut mock_capture, &mut invalid_stdout).unwrap();
        let invalid_output = String::from_utf8(invalid_stdout).unwrap();
        assert!(invalid_output.contains("invalid_recipe   Recipe Error"));
        assert!(invalid_output.contains(
            "\nRecipe 'invalid_recipe' has errors! Run 'pods recipe inspect invalid_recipe' for details.\n"
        ));

        // 3. Failed `podman ps` returns PodError::Io containing "Failed to query podman status."
        let mut fail_capture = || -> Option<String> { None };
        let ps_error = execute_with(&context, &mut fail_capture, &mut Vec::new()).unwrap_err();
        assert!(
            ps_error
                .to_string()
                .contains("Failed to query podman status.")
        );

        // 4. Empty display rows prints "No containers found."
        let mut empty_context = Context::new();
        empty_context.config.pods.path = temp_dir.join("pods");
        empty_context.config.recipes.path = temp_dir.join("recipes");
        let mut unmanaged_only_capture = || {
            Some(String::from(
                "c3d4e5f6a1b2;;;busybox;;;sh;;;1 day ago;;;Exited (0) 1 day ago;;;;;;unmanaged;;;;;;0\n",
            ))
        };
        let mut empty_stdout = Vec::new();
        execute_with(
            &empty_context,
            &mut unmanaged_only_capture,
            &mut empty_stdout,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(empty_stdout).unwrap(),
            "No containers found.\n"
        );

        // 5. Malformed lines and delimiter injection inside command in parse_status_record
        assert!(parse_status_record("").is_none());
        assert!(parse_status_record("too;;;few;;;fields").is_none());
        let injected_row = parse_status_record(
            "id;;;img;;;sh -c 'a;;;b;';;;created;;;status;;;ports;;;name;;;pod;;;3",
        )
        .unwrap();
        assert_eq!(injected_row.command, "sh -c 'a;;;b;'");
        assert_eq!(injected_row.created, "created");
        assert_eq!(injected_row.restarts, "3");

        // 6. Extreme capacity bounds (MAX_STATUS_RECORDS and MAX_LOG_MESSAGE_BYTES)
        let mut oversized_ps = String::new();
        let mut oversized_expected = BTreeMap::new();
        for index in 0..(MAX_STATUS_RECORDS + 10) {
            oversized_ps.push_str(&format!(
                "id_{index};;;img;;;cmd;;;now;;;Up;;;80;;;cnt_{index};;;pod;;;0\n"
            ));
            oversized_expected.insert(
                format!("missing_{index}"),
                ExpectedContainer {
                    pod_id: String::from("pod"),
                    target_name: format!("recipe/{}", index + 1),
                },
            );
        }
        let empty_expected = BTreeMap::new();
        let bounded_ps_rows = build_display_rows(&oversized_ps, &empty_expected, &[], true);
        assert_eq!(bounded_ps_rows.len(), MAX_STATUS_RECORDS);
        let bounded_missing_rows = build_display_rows("", &oversized_expected, &[], false);
        assert_eq!(bounded_missing_rows.len(), MAX_STATUS_RECORDS);

        let mut single_expected = BTreeMap::new();
        single_expected.insert(
            format!("cnt_{MAX_STATUS_RECORDS}"),
            ExpectedContainer {
                pod_id: String::from("pod"),
                target_name: String::from("recipe/1"),
            },
        );
        let filtered_after_limit =
            build_display_rows(&oversized_ps, &single_expected, &[], false);
        assert_eq!(filtered_after_limit.len(), 1);
        assert_eq!(filtered_after_limit[0].status, "Not Found");

        let oversized_cell = "x".repeat(MAX_LOG_MESSAGE_BYTES + 100);
        let mut widths = [0usize; 1];
        update_column_widths(&mut widths, &[&oversized_cell]);
        assert_eq!(widths[0], MAX_LOG_MESSAGE_BYTES);
        let clamped_cell_row = format_table_row(&[&oversized_cell], &widths);
        assert_eq!(clamped_cell_row.len(), MAX_LOG_MESSAGE_BYTES);
        let clamped_row = format_table_row(&["a"], &[usize::MAX]);
        assert_eq!(clamped_row.len(), MAX_LOG_MESSAGE_BYTES);
        let (empty_map, empty_invalid) = collect_expected_containers(&context.config, &[]);
        assert!(empty_map.is_empty());
        assert!(empty_invalid.is_empty());

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn execute_with_renders_filtered_and_full_status_tables_for_explicit_target() {
        let (temp_dir, config) = create_status_workspace("explicit_target");
        let mut context = Context::new();
        context.config = config;
        context.parameters = vec![String::from("recipe_011_simple_container")];

        let mut query_count = 0usize;
        let mut capture = || -> Option<String> {
            query_count += 1;
            Some(String::from(MOCK_PS_OUTPUT))
        };

        let mut stdout = Vec::new();
        execute_with(&context, &mut capture, &mut stdout).unwrap();

        let output = String::from_utf8(stdout).unwrap();
        let expected_header = "ID             POD                   NAME                        STATUS                 RESTARTS   CREATED       IMAGE       \n";
        let expected_row = "a1b2c3d4e5f6   test_simple_cnt_pod   test_simple_cnt_pod-app-1   Up 2 hours (healthy)   0          2 hours ago   nginx:alpine\n";
        assert_eq!(output, format!("{expected_header}{expected_row}"));

        // With --full, TARGET NAME is inserted after NAME, and COMMAND and PORTS columns are appended.
        context.flags.full = true;
        let mut full_stdout = Vec::new();
        execute_with(&context, &mut capture, &mut full_stdout).unwrap();
        assert_eq!(query_count, 2);
        let full_output = String::from_utf8(full_stdout).unwrap();
        let expected_full_header = "ID             POD                   NAME                        TARGET NAME                     STATUS                 RESTARTS   CREATED       IMAGE          COMMAND                  PORTS               \n";
        let expected_full_row = "a1b2c3d4e5f6   test_simple_cnt_pod   test_simple_cnt_pod-app-1   recipe_011_simple_container/1   Up 2 hours (healthy)   0          2 hours ago   nginx:alpine   nginx -g 'daemon off;'   0.0.0.0:8080->80/tcp\n";
        assert_eq!(
            full_output,
            format!("{expected_full_header}{expected_full_row}")
        );

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn execute_with_synthesizes_not_found_rows_sorts_by_pod_and_name_and_supports_all_flag() {
        let (temp_dir, config) = create_status_workspace("missing_and_all");
        let mut context = Context::new();
        context.config = config;
        context.parameters = vec![String::from("recipe_017_container_naming")];

        let mut capture = || Some(String::from(MOCK_PS_OUTPUT));
        let mut stdout = Vec::new();
        execute_with(&context, &mut capture, &mut stdout).unwrap();
        let output = String::from_utf8(stdout).unwrap();

        let expected_table = "\
ID             POD        NAME             STATUS      RESTARTS   CREATED      IMAGE        \n\
b2c3d4e5f6a1   test_pod   test_pod-1       Up 5 mins   1          5 mins ago   alpine:latest\n\
-              test_pod   test_pod-my-db   Not Found   -          -            -            \n\
-              test_pod   totally_custom   Not Found   -          -            -            \n";
        assert_eq!(output, expected_table);

        // Default target resolution (empty parameters) with --all includes unmanaged containers, Recipe Error rows, and all configured recipes.
        context.parameters.clear();
        context.flags.all = true;
        let mut all_stdout = Vec::new();
        execute_with(&context, &mut capture, &mut all_stdout).unwrap();
        let all_output = String::from_utf8(all_stdout).unwrap();

        let expected_all_table = "\
ID             POD                   NAME                        STATUS                 RESTARTS   CREATED       IMAGE        \n\
c3d4e5f6a1b2                         unmanaged_container         Exited (0) 1 day ago   0          1 day ago     busybox      \n\
-              -                     invalid_recipe              Recipe Error           -          -             -            \n\
-              -                     missing_recipe              Recipe Error           -          -             -            \n\
b2c3d4e5f6a1   test_pod              test_pod-1                  Up 5 mins              1          5 mins ago    alpine:latest\n\
-              test_pod              test_pod-my-db              Not Found              -          -             -            \n\
-              test_pod              totally_custom              Not Found              -          -             -            \n\
a1b2c3d4e5f6   test_simple_cnt_pod   test_simple_cnt_pod-app-1   Up 2 hours (healthy)   0          2 hours ago   nginx:alpine \n\
\n\
Recipes 'invalid_recipe' and 'missing_recipe' have errors! Run 'pods recipe inspect <recipe>' for details.\n";
        assert_eq!(all_output, expected_all_table);

        // With --all --full, unmanaged containers display `-` in TARGET NAME, Recipe Error rows display `<recipe>`, and managed containers display `<recipe>/<index>`.
        context.flags.full = true;
        let mut all_full_stdout = Vec::new();
        execute_with(&context, &mut capture, &mut all_full_stdout).unwrap();
        let all_full_output = String::from_utf8(all_full_stdout).unwrap();
        assert!(all_full_output.contains("unmanaged_container         -"));
        assert!(all_full_output.contains("invalid_recipe              invalid_recipe"));
        assert!(all_full_output.contains("missing_recipe              missing_recipe"));
        assert!(all_full_output.contains("test_pod-1                  recipe_017_container_naming/1"));
        assert!(all_full_output.contains("test_pod-my-db              recipe_017_container_naming/2"));
        assert!(all_full_output.contains("totally_custom              recipe_017_container_naming/3"));
        assert!(all_full_output.contains("test_simple_cnt_pod-app-1   recipe_011_simple_container/1"));

        let _ = std_fs::remove_dir_all(&temp_dir);
    }
}
