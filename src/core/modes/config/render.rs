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

//! Output formatting helpers for `pods config inspect`.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::io::Write;

use crate::core::config::Config;
use crate::core::log;
use crate::io::system::fs;
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::format::{self, DEFAULT_TARGET_COLUMN};
use crate::utilities::path;
use super::inspect::{GroupOutputLine, GroupValidationState, MAX_GROUP_OUTPUT_LINES};

const UNREFERENCED_PREFIX_CHARS: usize = 4;

/// Checks `config.pods.path` and `config.recipes.path` and writes the `Directories:` section to `stdout`.
pub(super) fn write_directories_section<W: Write>(
    config: &Config,
    total_recipe_files: Option<usize>,
    unreferenced_count: usize,
    stdout: &mut W,
) {
    log::write_print(stdout, "Directories:");

    let (pods_display, pods_status) = if config.pods.path.as_os_str().is_empty() {
        (Cow::Borrowed("-"), "[NOT SET]")
    } else if path::is_valid_directory(&config.pods.path) && fs::directory_exists(&config.pods.path)
    {
        (
            Cow::Owned(
                fs::get_absolute_path(&config.pods.path)
                    .display()
                    .to_string(),
            ),
            "[OK]",
        )
    } else {
        (
            Cow::Owned(
                fs::get_absolute_path(&config.pods.path)
                    .display()
                    .to_string(),
            ),
            "[NOT FOUND]",
        )
    };
    let pods_line = format!("  {:<14}{pods_display}", "Pods:");

    let recipes_status = format_recipes_directory_status(total_recipe_files, unreferenced_count);
    let absolute_recipes_path = fs::get_absolute_path(&config.recipes.path);
    let recipes_line = format!("  {:<14}{}", "Recipes:", absolute_recipes_path.display());

    let max_directory_line_length = pods_line.chars().count().max(recipes_line.chars().count());
    let directory_target_column =
        DEFAULT_TARGET_COLUMN.max(max_directory_line_length.saturating_add(1));

    log::write_print(
        stdout,
        &format::format_line(&pods_line, pods_status, directory_target_column),
    );
    log::write_print(
        stdout,
        &format::format_line(&recipes_line, &recipes_status, directory_target_column),
    );
    log::write_print(stdout, "");
}

/// Writes the `Groups:` header, rendered group hierarchy lines, and an optional truncation notice to `stdout`.
pub(super) fn write_groups_section<W: Write>(state: &GroupValidationState<'_>, stdout: &mut W) {
    log::write_print(stdout, "Groups:");
    let target_column = DEFAULT_TARGET_COLUMN.max(state.max_line_length.saturating_add(1));
    for item in &state.output_lines {
        match item {
            GroupOutputLine::Status { line, status } => {
                log::write_print(stdout, &format::format_line(line, status, target_column));
            }
            GroupOutputLine::Text(text) => {
                log::write_print(stdout, text);
            }
        }
    }
    if state.truncated {
        log::write_print(
            stdout,
            &format!("  ... (output truncated at {MAX_GROUP_OUTPUT_LINES} lines)"),
        );
    }
}

/// Writes the resolved configuration file path and general settings to `stdout`.
pub(super) fn write_header_and_settings<W: Write>(config: &Config, stdout: &mut W) {
    let config_path = fs::get_absolute_path(&config.path);
    let editor_display = if config.editor.is_empty() {
        "-"
    } else {
        config.editor.as_str()
    };
    log::write_print(stdout, &format!("Configuration: {}", config_path.display()));
    log::write_print(stdout, "");
    log::write_print(stdout, "Settings:");
    log::write_print(stdout, &format!("  Editor:       {editor_display}"));
    log::write_print(stdout, &format!("  Simulate:     {}", config.simulate));
    log::write_print(stdout, "");
}

/// Writes the `Unreferenced Recipes:` section to `stdout` when `unreferenced` is non-empty.
pub(super) fn write_unreferenced_section<W: Write>(unreferenced: &[&str], stdout: &mut W) {
    if unreferenced.is_empty() {
        return;
    }
    log::write_print(stdout, "");
    log::write_print(stdout, "Unreferenced Recipes:");
    let target_column = unreferenced_target_column(unreferenced);
    for stem in unreferenced {
        let line = format!("  • {stem}");
        log::write_print(
            stdout,
            &format::format_line(&line, "[UNREFERENCED]", target_column),
        );
    }
}

/// Writes the `Validation Summary:` section when missing, invalid, or unreferenced recipe files are detected.
pub(super) fn write_validation_summary<W: Write>(
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

/// Formats the status badge for the `Recipes:` line in the `Directories:` section.
#[must_use]
fn format_recipes_directory_status(
    total_recipe_files: Option<usize>,
    unreferenced_count: usize,
) -> Cow<'static, str> {
    let Some(total_files) = total_recipe_files else {
        return Cow::Borrowed("[NOT FOUND]");
    };
    let known_count = total_files.saturating_sub(unreferenced_count);
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

/// Writes the invalid recipe files bullet line to `stdout` when `invalid_recipes` is non-empty.
fn write_invalid_recipes_summary<W: Write>(invalid_recipes: &BTreeSet<&str>, stdout: &mut W) {
    if invalid_recipes.is_empty() {
        return;
    }
    let capacity = collection::bounded_capacity(invalid_recipes.len(), MAX_COLLECTION_PREALLOC);
    let mut invalid_list = Vec::with_capacity(capacity);
    invalid_list.extend(invalid_recipes.iter().copied());
    let hint = format::format_recipe_error_hint(&invalid_list);
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
        log::write_print(stdout, &format!("- Recipe file for '{single}' not found!"));
        return;
    }
    let capacity = collection::bounded_capacity(missing_recipes.len(), MAX_COLLECTION_PREALLOC);
    let mut missing_list = Vec::with_capacity(capacity);
    missing_list.extend(missing_recipes.iter().copied());
    let joined = format::format_quoted_conjunction(&missing_list);
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
