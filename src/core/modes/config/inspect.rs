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

//! Handler for `pods config` (`pods config inspect`).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};
use std::path::Path;

use crate::core::config::Config;
use crate::core::context::Context;
use crate::core::log;
use crate::core::recipe::{RECIPE_EXTENSION, Recipe};
use crate::error::PodError;
use crate::io::system::fs;
use crate::utilities::collection::{MAX_COLLECTION_PREALLOC, bounded_capacity};
use crate::utilities::format::{
    DEFAULT_TARGET_COLUMN, format_line, format_quoted_conjunction, format_recipe_error_hint,
};

const MAX_GROUP_OUTPUT_LINES: usize = 4096;
const UNREFERENCED_PREFIX_CHARS: usize = 4;

/// Entry in the rendered group hierarchy output queue.
enum GroupOutputLine {
    Status {
        line: String,
        status: &'static str,
    },
    Text(String),
}

/// Mutable state accumulated while traversing configured recipe groups for display and validation.
#[derive(Default)]
struct GroupValidationState<'a> {
    invalid_recipes: BTreeSet<&'a str>,
    max_line_length: usize,
    missing_recipes: BTreeSet<&'a str>,
    output_lines: Vec<GroupOutputLine>,
    referenced_recipes: BTreeSet<&'a str>,
    validation_cache: BTreeMap<&'a str, RecipeStatus>,
}

impl GroupValidationState<'_> {
    /// Creates a new empty [`GroupValidationState`].
    #[must_use]
    fn new() -> Self {
        Self::default()
    }
}

/// Inspection outcome for a referenced `.pod` recipe file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RecipeStatus {
    Error,
    NotFound,
    Ok,
}

impl RecipeStatus {
    /// Returns the bracketed status badge for this recipe inspection outcome.
    #[must_use]
    const fn badge(self) -> &'static str {
        match self {
            Self::Error => "[ERROR]",
            Self::NotFound => "[NOT FOUND]",
            Self::Ok => "[OK]",
        }
    }
}

/// Displays the active configuration settings, directory status, group hierarchy tree,
/// unreferenced recipes, and validation summary.
///
/// # Workflow
///
/// 1. Use `context.config` already loaded from `context.config.path`.
/// 2. Print the resolved absolute path of `context.config.path` and `Settings:` (`Editor`,
///    `Simulate`).
/// 3. Check `context.config.pods.path` and `context.config.recipes.path` on disk, count
///    known and unreferenced `.pod` files in `recipes.path`, and print the aligned
///    `Directories:` status lines (`[OK]`, `[OK, K known recipe(s) found]`,
///    `[OK, K known and U unreferenced recipe(s) found]`, or `[NOT FOUND]`).
/// 4. Iterate through sorted `context.config.recipes.groups`, expand `@<group>` references
///    for display, inspect `<recipe>.pod` files using [`Recipe::inspect`] with a local lookup
///    cache, and render the aligned `Groups:` tree (`[OK]`, `[ERROR]`, `[NOT FOUND]`,
///    `[MISSING GROUP]`).
/// 5. When unreferenced `.pod` files exist in `recipes.path`, list their recipe stems under
///    `Unreferenced Recipes:` with status `[UNREFERENCED]`.
/// 6. Print the `Validation Summary:` for missing, invalid, or unreferenced recipes.
///
/// # Errors
///
/// Returns [`PodError`] if a required filesystem query fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    inspect_with(&context.config, &mut io::stdout().lock())
}

/// Appends a single group element (`@<subgroup>` or `<recipe>`) to `state`.
fn append_group_element<'a>(
    state: &mut GroupValidationState<'a>,
    config: &'a Config,
    element: &'a str,
    prefix: &str,
    is_last: bool,
    visiting: &mut BTreeSet<&'a str>,
) {
    if state.output_lines.len() >= MAX_GROUP_OUTPUT_LINES {
        return;
    }
    let branch = if is_last { "└── " } else { "├── " };
    if let Some(subgroup_name) = element.strip_prefix('@') {
        state
            .output_lines
            .push(GroupOutputLine::Text(format!("{prefix}{branch}{element}")));
        append_subgroup(state, config, subgroup_name, prefix, is_last, visiting);
        return;
    }
    let line = format!("{prefix}{branch}{element}");
    append_recipe_status_line(state, config, element, line);
}

/// Appends all direct elements of a single recipe group to `state`.
fn append_group_elements<'a>(
    state: &mut GroupValidationState<'a>,
    config: &'a Config,
    elements: &'a [String],
    prefix: &str,
    visiting: &mut BTreeSet<&'a str>,
) {
    let element_count = elements.len();
    for (j, element) in elements.iter().enumerate() {
        if state.output_lines.len() >= MAX_GROUP_OUTPUT_LINES {
            return;
        }
        let is_last = j + 1 == element_count;
        append_group_element(state, config, element.as_str(), prefix, is_last, visiting);
    }
}

/// Evaluates `<recipe>.pod` status (using `state.validation_cache`) and appends a status line to `state`.
fn append_recipe_status_line<'a>(
    state: &mut GroupValidationState<'a>,
    config: &'a Config,
    recipe_name: &'a str,
    line: String,
) {
    let status = record_referenced_recipe(state, config, recipe_name).badge();
    let visible_length = line.chars().count();
    if visible_length > state.max_line_length {
        state.max_line_length = visible_length;
    }
    state
        .output_lines
        .push(GroupOutputLine::Status { line, status });
}

/// Expands a `@<subgroup>` reference and appends its entries (or `[MISSING GROUP]`) to `state`.
fn append_subgroup<'a>(
    state: &mut GroupValidationState<'a>,
    config: &'a Config,
    subgroup_name: &'a str,
    parent_prefix: &str,
    parent_is_last: bool,
    visiting: &mut BTreeSet<&'a str>,
) {
    if state.output_lines.len() >= MAX_GROUP_OUTPUT_LINES {
        return;
    }
    let indent = if parent_is_last { "    " } else { "│   " };
    let child_prefix = format!("{parent_prefix}{indent}");
    let Some(sub_elements) = config
        .recipes
        .groups
        .get(subgroup_name)
        .filter(|_| visiting.insert(subgroup_name))
    else {
        state.output_lines.push(GroupOutputLine::Text(format!(
            "{child_prefix}└── [MISSING GROUP]"
        )));
        return;
    };
    append_group_elements(state, config, sub_elements, &child_prefix, visiting);
    visiting.remove(subgroup_name);
}

/// Builds the group hierarchy output lines and recipe validation sets from `config`.
#[must_use]
fn build_groups_state(config: &Config) -> GroupValidationState<'_> {
    let mut state = GroupValidationState::new();
    let group_count = config.recipes.groups.len();
    let estimated_lines = bounded_capacity(group_count.saturating_mul(4), MAX_COLLECTION_PREALLOC);
    state.output_lines = Vec::with_capacity(estimated_lines);
    let mut visiting = BTreeSet::new();

    for (i, (group_name, elements)) in config.recipes.groups.iter().enumerate() {
        if state.output_lines.len() >= MAX_GROUP_OUTPUT_LINES {
            break;
        }
        state
            .output_lines
            .push(GroupOutputLine::Text(format!("  • {group_name}")));
        visiting.insert(group_name.as_str());
        append_group_elements(&mut state, config, elements, "    ", &mut visiting);
        visiting.remove(group_name.as_str());
        if i + 1 < group_count && state.output_lines.len() < MAX_GROUP_OUTPUT_LINES {
            state
                .output_lines
                .push(GroupOutputLine::Text(String::from("    ")));
        }
    }
    for recipe_name in config.get_recipes() {
        record_referenced_recipe(&mut state, config, recipe_name);
    }
    state
}

/// Filters discovered `.pod` filenames in `recipes_files` and returns the recipe stems not in `referenced_recipes`.
#[must_use]
fn collect_unreferenced_recipes<'a>(
    recipes_files: Option<&'a [String]>,
    referenced_recipes: &BTreeSet<&str>,
) -> Vec<&'a str> {
    let Some(files) = recipes_files else {
        return Vec::new();
    };
    let capacity = bounded_capacity(files.len(), MAX_COLLECTION_PREALLOC);
    let mut unreferenced = Vec::with_capacity(capacity);
    for file in files {
        let stem = file.strip_suffix(RECIPE_EXTENSION).unwrap_or(file.as_str());
        if !referenced_recipes.contains(stem) {
            unreferenced.push(stem);
        }
    }
    unreferenced
}

/// Checks whether `<recipe>.pod` exists on disk and validates its PSL contents via [`Recipe::inspect`].
#[must_use]
fn evaluate_recipe_status(config: &Config, recipe_name: &str) -> RecipeStatus {
    if !Recipe::file_exists(&config.recipes.path, recipe_name) {
        return RecipeStatus::NotFound;
    }
    if Recipe::inspect(config, recipe_name).is_ok() {
        return RecipeStatus::Ok;
    }
    RecipeStatus::Error
}

/// Formats the status badge for the `Recipes:` line in the `Directories:` section.
#[must_use]
fn format_recipes_directory_status(
    recipes_files: Option<&[String]>,
    unreferenced_count: usize,
) -> Cow<'static, str> {
    let Some(files) = recipes_files else {
        return Cow::Borrowed("[NOT FOUND]");
    };
    let known_count = files.len().saturating_sub(unreferenced_count);
    if unreferenced_count == 0 {
        let noun = if known_count == 1 {
            "recipe"
        } else {
            "recipes"
        };
        return Cow::Owned(format!("[OK, {known_count} known {noun} found]"));
    }
    let noun = if unreferenced_count == 1 {
        "recipe"
    } else {
        "recipes"
    };
    Cow::Owned(format!(
        "[OK, {known_count} known and {unreferenced_count} unreferenced {noun} found]"
    ))
}

/// Renders the full configuration inspection report for `config` to `stdout`.
fn inspect_with<W: Write>(config: &Config, stdout: &mut W) -> Result<(), PodError> {
    write_header_and_settings(config, stdout);
    let state = build_groups_state(config);
    let recipes_files = list_recipes_directory(&config.recipes.path)?;
    let unreferenced =
        collect_unreferenced_recipes(recipes_files.as_deref(), &state.referenced_recipes);
    write_directories_section(config, recipes_files.as_deref(), unreferenced.len(), stdout);
    write_groups_section(&state, stdout);
    write_unreferenced_section(&unreferenced, stdout);
    write_validation_summary(
        &state.missing_recipes,
        &state.invalid_recipes,
        unreferenced.len(),
        stdout,
    );
    Ok(())
}

/// Lists `.pod` files in `recipes_path` when the directory exists on disk.
fn list_recipes_directory(recipes_path: &Path) -> Result<Option<Vec<String>>, PodError> {
    if !fs::directory_exists(recipes_path) {
        return Ok(None);
    }
    let files = fs::list_directory(recipes_path, Some(RECIPE_EXTENSION))?;
    Ok(Some(files))
}

/// Records `recipe_name` in `state.referenced_recipes`, inspects `<recipe>.pod` via `state.validation_cache`, and updates `state.missing_recipes` and `state.invalid_recipes`.
fn record_referenced_recipe<'a>(
    state: &mut GroupValidationState<'a>,
    config: &'a Config,
    recipe_name: &'a str,
) -> RecipeStatus {
    state.referenced_recipes.insert(recipe_name);
    let status = *state
        .validation_cache
        .entry(recipe_name)
        .or_insert_with(|| evaluate_recipe_status(config, recipe_name));
    match status {
        RecipeStatus::Error => {
            state.invalid_recipes.insert(recipe_name);
        }
        RecipeStatus::NotFound => {
            state.missing_recipes.insert(recipe_name);
        }
        RecipeStatus::Ok => {}
    }
    status
}

/// Calculates the alignment target column for the `Unreferenced Recipes:` section.
#[must_use]
fn unreferenced_target_column(unreferenced: &[&str]) -> usize {
    let max_stem_chars = unreferenced
        .iter()
        .map(|stem| stem.chars().count())
        .max()
        .unwrap_or(0);
    let max_line_length = max_stem_chars.saturating_add(UNREFERENCED_PREFIX_CHARS);
    DEFAULT_TARGET_COLUMN.max(max_line_length.saturating_add(1))
}

/// Checks `config.pods.path` and `config.recipes.path` and writes the `Directories:` section to `stdout`.
fn write_directories_section<W: Write>(
    config: &Config,
    recipes_files: Option<&[String]>,
    unreferenced_count: usize,
    stdout: &mut W,
) {
    log::write_print(stdout, "Directories:");

    let pods_status = if fs::directory_exists(&config.pods.path) {
        "[OK]"
    } else {
        "[NOT FOUND]"
    };
    let absolute_pods_path = fs::get_absolute_path(&config.pods.path);
    let pods_line = format!("  {:<14}{}", "Pods:", absolute_pods_path.display());

    let recipes_status = format_recipes_directory_status(recipes_files, unreferenced_count);
    let absolute_recipes_path = fs::get_absolute_path(&config.recipes.path);
    let recipes_line = format!("  {:<14}{}", "Recipes:", absolute_recipes_path.display());

    let max_directory_line_length = pods_line
        .chars()
        .count()
        .max(recipes_line.chars().count());
    let directory_target_column =
        DEFAULT_TARGET_COLUMN.max(max_directory_line_length.saturating_add(1));

    log::write_print(
        stdout,
        &format_line(&pods_line, pods_status, directory_target_column),
    );
    log::write_print(
        stdout,
        &format_line(&recipes_line, &recipes_status, directory_target_column),
    );
    log::write_print(stdout, "");
}

/// Writes the `Groups:` header and rendered group hierarchy lines in `state` to `stdout`.
fn write_groups_section<W: Write>(state: &GroupValidationState<'_>, stdout: &mut W) {
    log::write_print(stdout, "Groups:");
    let target_column = DEFAULT_TARGET_COLUMN.max(state.max_line_length.saturating_add(1));
    for item in &state.output_lines {
        match item {
            GroupOutputLine::Status { line, status } => {
                log::write_print(stdout, &format_line(line, status, target_column));
            }
            GroupOutputLine::Text(text) => {
                log::write_print(stdout, text);
            }
        }
    }
}

/// Writes the resolved configuration file path and general settings to `stdout`.
fn write_header_and_settings<W: Write>(config: &Config, stdout: &mut W) {
    let config_path = fs::get_absolute_path(&config.path);
    log::write_print(stdout, &format!("Configuration: {}", config_path.display()));
    log::write_print(stdout, "");
    log::write_print(stdout, "Settings:");
    log::write_print(stdout, &format!("  Editor:       {}", config.editor));
    log::write_print(stdout, &format!("  Simulate:     {}", config.simulate));
    log::write_print(stdout, "");
}

/// Writes the invalid recipe files bullet line to `stdout` when `invalid_recipes` is non-empty.
fn write_invalid_recipes_summary<W: Write>(invalid_recipes: &BTreeSet<&str>, stdout: &mut W) {
    if invalid_recipes.is_empty() {
        return;
    }
    let capacity = bounded_capacity(invalid_recipes.len(), MAX_COLLECTION_PREALLOC);
    let mut invalid_list = Vec::with_capacity(capacity);
    invalid_list.extend(invalid_recipes.iter().copied());
    let hint = format_recipe_error_hint(&invalid_list);
    log::write_print(stdout, &format!("- {hint}"));
}

/// Writes the missing recipe files bullet line to `stdout` when `missing_recipes` is non-empty.
fn write_missing_recipes_summary<W: Write>(missing_recipes: &BTreeSet<&str>, stdout: &mut W) {
    if missing_recipes.is_empty() {
        return;
    }
    if missing_recipes.len() == 1
        && let Some(&single) = missing_recipes.iter().next()
    {
        log::write_print(
            stdout,
            &format!("- Recipe file for '{single}' not found!"),
        );
        return;
    }
    let capacity = bounded_capacity(missing_recipes.len(), MAX_COLLECTION_PREALLOC);
    let mut missing_list = Vec::with_capacity(capacity);
    missing_list.extend(missing_recipes.iter().copied());
    let joined = format_quoted_conjunction(&missing_list);
    log::write_print(stdout, &format!("- Recipe files for {joined} not found!"));
}

/// Writes the unreferenced recipes count bullet line to `stdout` when `unreferenced_count` is non-zero.
fn write_unreferenced_recipes_summary<W: Write>(unreferenced_count: usize, stdout: &mut W) {
    if unreferenced_count == 0 {
        return;
    }
    let noun = if unreferenced_count == 1 {
        "recipe"
    } else {
        "recipes"
    };
    log::write_print(
        stdout,
        &format!("- {unreferenced_count} unreferenced {noun} found!"),
    );
}

/// Writes the `Unreferenced Recipes:` section to `stdout` when `unreferenced` is non-empty.
fn write_unreferenced_section<W: Write>(unreferenced: &[&str], stdout: &mut W) {
    if unreferenced.is_empty() {
        return;
    }
    log::write_print(stdout, "");
    log::write_print(stdout, "Unreferenced Recipes:");
    let target_column = unreferenced_target_column(unreferenced);
    for stem in unreferenced {
        let line = format!("  • {stem}");
        log::write_print(stdout, &format_line(&line, "[UNREFERENCED]", target_column));
    }
}

/// Writes the `Validation Summary:` section when missing, invalid, or unreferenced recipe files are detected.
fn write_validation_summary<W: Write>(
    missing_recipes: &BTreeSet<&str>,
    invalid_recipes: &BTreeSet<&str>,
    unreferenced_count: usize,
    stdout: &mut W,
) {
    if missing_recipes.is_empty() && invalid_recipes.is_empty() && unreferenced_count == 0 {
        return;
    }
    log::write_print(stdout, "");
    log::write_print(stdout, "Validation Summary:");
    write_missing_recipes_summary(missing_recipes, stdout);
    write_invalid_recipes_summary(invalid_recipes, stdout);
    write_unreferenced_recipes_summary(unreferenced_count, stdout);
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs as std_fs;
    use std::path::PathBuf;
    use std::process;
    use std::str::FromStr;

    use crate::core::config::Config;
    use crate::core::context::Context;
    use crate::io::system::fs;
    use super::{MAX_GROUP_OUTPUT_LINES, build_groups_state, handle, inspect_with};

    const VALID_RECIPE: &str = "\
pod test-pod {
    name \"Test Pod\"
    registry docker.io
    path /pods/test-pod
}
container *app {
    image alpine:latest
}
";

    fn create_temp_test_dir(suffix: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "podoxide_config_inspect_{}_{suffix}",
            process::id()
        ));
        let _ = std_fs::remove_dir_all(&dir);
        std_fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn inspect_with_clamps_diamond_group_dag_output_and_preserves_recipe_validation() {
        let temp_dir = create_temp_test_dir("diamond_dag");
        let recipes_dir = temp_dir.join("recipes");
        std_fs::create_dir_all(&recipes_dir).unwrap();

        let mut config = Config::new();
        config.recipes.path = recipes_dir;
        for i in 0..30 {
            let group_name = format!("g{i:02}");
            let next_target = format!("@g{:02}", i + 1);
            config
                .recipes
                .groups
                .insert(group_name, vec![next_target.clone(), next_target]);
        }
        config
            .recipes
            .groups
            .insert(String::from("g30"), vec![String::from("leaf_recipe")]);

        let state = build_groups_state(&config);
        assert_eq!(state.output_lines.len(), MAX_GROUP_OUTPUT_LINES);
        assert!(state.referenced_recipes.contains("leaf_recipe"));
        assert!(state.missing_recipes.contains("leaf_recipe"));

        let mut stdout = Vec::new();
        inspect_with(&config, &mut stdout).unwrap();
        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains("[OK, 0 known recipes found]\n"));
        assert!(!output.contains("Unreferenced Recipes:"));
        assert!(output.contains("Validation Summary:\n"));
        assert!(output.contains("- Recipe file for 'leaf_recipe' not found!\n"));

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn inspect_with_distinguishes_subgroup_from_same_named_recipe_leaf() {
        let temp_dir = create_temp_test_dir("same_named_leaf");
        let recipes_dir = temp_dir.join("recipes");
        std_fs::create_dir_all(&recipes_dir).unwrap();

        fs::write_file(&recipes_dir.join("alpha.pod"), VALID_RECIPE).unwrap();
        fs::write_file(&recipes_dir.join("beta.pod"), VALID_RECIPE).unwrap();
        fs::write_file(&recipes_dir.join("stack.pod"), VALID_RECIPE).unwrap();

        let config_input = "\
general {
    editor vim
    simulate false
}

pods {
    path /pods
}

recipes {
    path ./recipes

    groups {
        all @alpha @stack
        stack media cloud proxy
        alpha beta alpha
    }
}
";
        let mut config = Config::from_str(config_input).unwrap();
        config.recipes.path = recipes_dir;

        let mut stdout = Vec::new();
        inspect_with(&config, &mut stdout).unwrap();
        let output = String::from_utf8(stdout).unwrap();

        assert!(output.contains("Settings:\n  Editor:       vim\n  Simulate:     false\n"));
        assert!(output.contains("[OK, 2 known and 1 unreferenced recipe found]\n"));
        assert!(output.contains(
            "  • all\n    ├── @alpha\n    │   ├── beta                            [OK]\n    │   └── alpha                           [OK]\n    └── @stack\n        ├── media                           [NOT FOUND]\n        ├── cloud                           [NOT FOUND]\n        └── proxy                           [NOT FOUND]\n"
        ));
        assert!(output.contains(
            "  • alpha\n    ├── beta                                [OK]\n    └── alpha                               [OK]\n"
        ));
        assert!(output.contains(
            "  • stack\n    ├── media                               [NOT FOUND]\n    ├── cloud                               [NOT FOUND]\n    └── proxy                               [NOT FOUND]\n"
        ));
        assert!(!output.contains("[MISSING GROUP]"));
        assert!(output.contains(
            "Unreferenced Recipes:\n  • stack                                   [UNREFERENCED]\n"
        ));
        assert!(output.contains(
            "Validation Summary:\n- Recipe files for 'cloud', 'media' and 'proxy' not found!\n- 1 unreferenced recipe found!\n"
        ));

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn inspect_with_displays_settings_directories_and_clean_config() {
        let temp_dir = create_temp_test_dir("clean");
        let pods_dir = temp_dir.join("pods");
        let recipes_dir = temp_dir.join("recipes");
        std_fs::create_dir_all(&pods_dir).unwrap();
        std_fs::create_dir_all(&recipes_dir).unwrap();

        fs::write_file(&recipes_dir.join("app_one.pod"), VALID_RECIPE).unwrap();
        fs::write_file(&recipes_dir.join("ignored.txt"), "not a recipe").unwrap();

        let mut config = Config::new();
        config.editor = String::from("vim");
        config.path = temp_dir.join("pods.conf");
        config.pods.path = pods_dir;
        config.recipes.path = recipes_dir;
        config.simulate = true;
        config
            .recipes
            .groups
            .insert(String::from("all"), vec![String::from("app_one")]);

        let mut stdout = Vec::new();
        inspect_with(&config, &mut stdout).unwrap();
        let output = String::from_utf8(stdout).unwrap();

        assert!(output.contains("Configuration: "));
        assert!(output.contains("pods.conf\n"));
        assert!(output.contains("Settings:\n  Editor:       vim\n  Simulate:     true\n"));
        assert!(output.contains("Directories:\n"));
        assert!(output.contains("[OK]\n"));
        assert!(output.contains("[OK, 1 known recipe found]\n"));
        assert!(output.contains("Groups:\n  • all\n"));
        assert!(output.contains("    └── app_one                             [OK]\n"));
        assert!(!output.contains("Unreferenced Recipes:"));
        assert!(!output.contains("Validation Summary:"));

        let mut context = Context::new();
        context.config = config;
        assert!(handle(&context).is_ok());

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn inspect_with_handles_missing_directories_and_singular_validation_warnings() {
        let temp_dir = create_temp_test_dir("singular");
        let recipes_dir = temp_dir.join("recipes");
        std_fs::create_dir_all(&recipes_dir).unwrap();
        fs::write_file(&recipes_dir.join("orphan.pod"), "invalid {{{ psl").unwrap();
        fs::write_file(&recipes_dir.join("broken_one.pod"), "invalid {{{ psl").unwrap();

        let mut config = Config::new();
        config.path = PathBuf::new();
        config.pods.path = PathBuf::from("/pods");
        config.recipes.path = recipes_dir;
        config.recipes.groups.insert(
            String::from("all"),
            vec![String::from("missing_one"), String::from("broken_one")],
        );

        let mut stdout = Vec::new();
        inspect_with(&config, &mut stdout).unwrap();
        let output = String::from_utf8(stdout).unwrap();

        assert!(output.contains("Configuration: \n"));
        assert!(output.contains("  Pods:"));
        assert!(output.contains("[NOT FOUND]\n"));
        assert!(output.contains("[OK, 1 known and 1 unreferenced recipe found]\n"));
        assert!(output.contains("    ├── missing_one                         [NOT FOUND]\n"));
        assert!(output.contains("    └── broken_one                          [ERROR]\n"));
        assert!(output.contains(
            "Unreferenced Recipes:\n  • orphan                                  [UNREFERENCED]\n"
        ));
        assert!(output.contains("Validation Summary:\n"));
        assert!(output.contains("- Recipe file for 'missing_one' not found!\n"));
        assert!(output.contains(
            "- Recipe 'broken_one' has errors! Run 'pods recipe inspect broken_one' for details.\n"
        ));
        assert!(output.contains("- 1 unreferenced recipe found!\n"));

        config.recipes.path = temp_dir.join("nonexistent_recipes");
        let mut missing_recipes_dir_out = Vec::new();
        inspect_with(&config, &mut missing_recipes_dir_out).unwrap();
        let missing_dir_text = String::from_utf8(missing_recipes_dir_out).unwrap();
        assert!(missing_dir_text.contains("Recipes:"));
        assert!(missing_dir_text.contains("[NOT FOUND]\n"));
        assert!(!missing_dir_text.contains("Unreferenced Recipes:"));

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn inspect_with_renders_tree_hierarchy_missing_groups_and_plural_validation_summary() {
        let temp_dir = create_temp_test_dir("hierarchy");
        let recipes_dir = temp_dir.join("recipes");
        std_fs::create_dir_all(&recipes_dir).unwrap();

        fs::write_file(&recipes_dir.join("recipe_001_empty.pod"), "").unwrap();
        fs::write_file(&recipes_dir.join("recipe_002_no_pod.pod"), "").unwrap();
        fs::write_file(&recipes_dir.join("recipe_003_extra.pod"), "").unwrap();
        fs::write_file(&recipes_dir.join("broken_second.pod"), "pod {\n").unwrap();

        let mut config = Config::new();
        config.editor = String::from("vim");
        config.simulate = false;
        config.pods.path = temp_dir.join("non_existent_pods_dir");
        config.recipes.path = recipes_dir;
        config
            .recipes
            .groups
            .insert(String::from("all"), vec![String::from("missing_recipe")]);
        config
            .recipes
            .groups
            .insert(String::from("cycle_a"), vec![String::from("@cycle_b")]);
        config
            .recipes
            .groups
            .insert(String::from("cycle_b"), vec![String::from("@cycle_a")]);
        config.recipes.groups.insert(
            String::from("database"),
            vec![
                String::from("recipe_001_empty"),
                String::from("broken_second"),
                String::from("very_long_missing_recipe_name_exceeding_column_forty_four"),
            ],
        );
        config
            .recipes
            .groups
            .insert(String::from("meta"), vec![String::from("@stack")]);
        config.recipes.groups.insert(
            String::from("stack"),
            vec![
                String::from("@missing_mid"),
                String::from("@database"),
                String::from("@web"),
                String::from("@missing_end"),
            ],
        );
        config
            .recipes
            .groups
            .insert(String::from("web"), vec![String::from("frontend")]);

        let mut stdout = Vec::new();
        inspect_with(&config, &mut stdout).unwrap();
        let output = String::from_utf8(stdout).unwrap();

        assert!(output.contains("[OK, 2 known and 2 unreferenced recipes found]\n"));
        assert!(output.contains("  • all\n"));
        assert!(output.contains("    └── missing_recipe"));
        assert!(output.contains("  • cycle_a\n    └── @cycle_b\n        └── @cycle_a\n            └── [MISSING GROUP]\n"));
        assert!(output.contains("  • database\n"));
        assert!(output.contains("    ├── recipe_001_empty"));
        assert!(output.contains("[ERROR]\n"));
        assert!(output.contains("  • meta\n    └── @stack\n        ├── @missing_mid\n        │   └── [MISSING GROUP]\n"));
        assert!(output.contains("        ├── @database\n        │   ├── recipe_001_empty"));
        assert!(output.contains("  • stack\n"));
        assert!(output.contains("    ├── @missing_mid\n    │   └── [MISSING GROUP]\n"));
        assert!(output.contains("    ├── @database\n    │   ├── recipe_001_empty"));
        assert!(output.contains("    ├── @web\n    │   └── frontend"));
        assert!(output.contains("    └── @missing_end\n        └── [MISSING GROUP]\n"));
        assert!(output.contains("  • web\n"));
        assert!(output.contains(
            "Unreferenced Recipes:\n  • recipe_002_no_pod                       [UNREFERENCED]\n  • recipe_003_extra                        [UNREFERENCED]\n"
        ));
        assert!(output.contains("Validation Summary:\n"));
        assert!(!output.contains("'@"));
        assert!(output.contains(
            "- Recipe files for 'frontend', 'missing_recipe' and 'very_long_missing_recipe_name_exceeding_column_forty_four' not found!\n"
        ));
        assert!(output.contains(
            "- Recipes 'broken_second' and 'recipe_001_empty' have errors! Run 'pods recipe inspect <recipe>' for details.\n"
        ));
        assert!(output.contains("- 2 unreferenced recipes found!\n"));

        let _ = std_fs::remove_dir_all(&temp_dir);
    }
}
