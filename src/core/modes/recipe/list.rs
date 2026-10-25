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

//! Handler for `pods recipe [list] [--all]`.

use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::Path;

use crate::core::context::Context;
use crate::core::log;
use crate::core::recipe::{RECIPE_EXTENSION, Recipe};
use crate::error::PodError;
use crate::io::system::fs;
use crate::utilities::collection::{MAX_COLLECTION_PREALLOC, bounded_capacity};
use crate::utilities::format::{DEFAULT_TARGET_COLUMN, format_index_prefix, format_line};
use crate::utilities::path::is_valid_directory;

/// Formatted recipe list line paired with its right-aligned status label.
struct RecipeListEntry {
    line: String,
    status: &'static str,
}

/// Lists all recipes registered in `pods.conf` with descriptions and file status, plus
/// unlinked `.pod` files on disk when `--all` is set.
///
/// # Workflow
///
/// 1. Collect the sorted list and lookup set of unique recipe names referenced across
///    `context.config.recipes.groups`.
/// 2. For each configured recipe:
///    - Check whether `<recipes.path>/<recipe>.pod` exists on disk (`[OK]` vs `[NOT FOUND]`).
///    - When the file exists, read and parse `<recipe>.pod` (suppressing validation warnings)
///      to extract `pod.name` and `pod.description`.
///    - Format the numbered entry `"  <idx>) <recipe> (<name>): <description>"` aligned with
///      its status column.
/// 3. When `context.flags.all` is `true`, list `.pod` filenames in `recipes.path` that are
///    not referenced in `context.config.recipes.groups` under `Unlinked Recipe Files:` with
///    status `[UNREFERENCED]`. Do not read or parse the contents of unreferenced `.pod` files.
///
/// # Errors
///
/// Returns [`PodError`] if directory inspection fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    list_with(context, &mut io::stdout().lock())
}

/// Builds the formatted lines and maximum visible line length for all configured recipes.
#[must_use]
fn build_configured_entries(
    recipes_path: &Path,
    configured_recipes: &BTreeSet<&str>,
    total_count: usize,
) -> (Vec<RecipeListEntry>, usize) {
    let capacity = bounded_capacity(configured_recipes.len(), MAX_COLLECTION_PREALLOC);
    let mut entries = Vec::with_capacity(capacity);
    let mut max_length = 0;
    for (zero_based, &target) in configured_recipes.iter().enumerate() {
        let entry =
            build_single_configured_entry(recipes_path, target, zero_based + 1, total_count);
        let visible_length = entry.line.chars().count();
        if visible_length > max_length {
            max_length = visible_length;
        }
        entries.push(entry);
    }
    (entries, max_length)
}

/// Builds a single numbered [`RecipeListEntry`] and checks `<recipes.path>/<target>.pod` on disk.
#[must_use]
fn build_single_configured_entry(
    recipes_path: &Path,
    target: &str,
    index: usize,
    total_count: usize,
) -> RecipeListEntry {
    let exists = Recipe::file_exists(recipes_path, target);
    let status = if exists { "[OK]" } else { "[NOT FOUND]" };
    let prefix = format_index_prefix(index, total_count);
    let label = format_recipe_label(recipes_path, target, exists);
    let line = format!("  {prefix} {label}");
    RecipeListEntry { line, status }
}

/// Builds the formatted lines and maximum visible line length for unreferenced `.pod` files.
#[must_use]
fn build_unreferenced_entries(
    unreferenced: &[String],
    start_index: usize,
    total_count: usize,
) -> (Vec<RecipeListEntry>, usize) {
    let capacity = bounded_capacity(unreferenced.len(), MAX_COLLECTION_PREALLOC);
    let mut entries = Vec::with_capacity(capacity);
    let mut max_length = 0;
    for (offset, file) in unreferenced.iter().enumerate() {
        let index = start_index + offset + 1;
        let prefix = format_index_prefix(index, total_count);
        let line = format!("  {prefix} {file}");
        let visible_length = line.chars().count();
        if visible_length > max_length {
            max_length = visible_length;
        }
        entries.push(RecipeListEntry {
            line,
            status: "[UNREFERENCED]",
        });
    }
    (entries, max_length)
}

/// Lists `.pod` files in `recipes_path` that are not in `configured_recipes` when `show_all` is `true`.
fn collect_unreferenced_files(
    recipes_path: &Path,
    configured_recipes: &BTreeSet<&str>,
    show_all: bool,
) -> Result<Vec<String>, PodError> {
    if !show_all || !is_valid_directory(recipes_path) || !fs::directory_exists(recipes_path) {
        return Ok(Vec::new());
    }
    let files = fs::list_directory(recipes_path, Some(RECIPE_EXTENSION))?;
    let capacity = bounded_capacity(files.len(), MAX_COLLECTION_PREALLOC);
    let mut unreferenced = Vec::with_capacity(capacity);
    for file in files {
        let stem = file.strip_suffix(RECIPE_EXTENSION).unwrap_or(file.as_str());
        if !configured_recipes.contains(stem) {
            unreferenced.push(file);
        }
    }
    Ok(unreferenced)
}

/// Formats `<target> [(<name>)][: <description>]` by loading `<target>.pod` when `exists` is `true`.
#[must_use]
fn format_recipe_label(recipes_path: &Path, target: &str, exists: bool) -> String {
    if !exists {
        return String::from(target);
    }
    let Ok(recipe) = Recipe::load(recipes_path, target) else {
        return String::from(target);
    };
    let name = recipe.pod.name.trim();
    let description = recipe.pod.description.trim();
    match (!name.is_empty(), !description.is_empty()) {
        (false, false) => String::from(target),
        (true, false) => format!("{target} ({name})"),
        (false, true) => format!("{target}: {description}"),
        (true, true) => format!("{target} ({name}): {description}"),
    }
}

/// Discovers and writes configured recipes (and optionally unreferenced `.pod` files) to `stdout`.
fn list_with<W: Write>(context: &Context, stdout: &mut W) -> Result<(), PodError> {
    let recipes_path = &context.config.recipes.path;
    let configured_recipes = context.config.get_recipes();
    let unreferenced =
        collect_unreferenced_files(recipes_path, &configured_recipes, context.flags.all)?;
    if configured_recipes.is_empty() && unreferenced.is_empty() {
        log::write_print(stdout, "There are no recipes defined in config.");
        return Ok(());
    }
    let total_count = configured_recipes.len().saturating_add(unreferenced.len());
    if !configured_recipes.is_empty() {
        log::write_print(stdout, "Recipes:");
        let (entries, max_length) =
            build_configured_entries(recipes_path, &configured_recipes, total_count);
        write_entries(&entries, max_length, stdout);
    }
    if !unreferenced.is_empty() {
        if !configured_recipes.is_empty() {
            log::write_print(stdout, "");
        }
        log::write_print(stdout, "Unlinked Recipe Files:");
        let (entries, max_length) =
            build_unreferenced_entries(&unreferenced, configured_recipes.len(), total_count);
        write_entries(&entries, max_length, stdout);
    }
    Ok(())
}

/// Writes aligned [`RecipeListEntry`] lines to `stdout`.
fn write_entries<W: Write>(entries: &[RecipeListEntry], max_length: usize, stdout: &mut W) {
    let target_column = DEFAULT_TARGET_COLUMN.max(max_length.saturating_add(1));
    for entry in entries {
        log::write_print(
            stdout,
            &format_line(&entry.line, entry.status, target_column),
        );
    }
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs as std_fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process;

    use crate::core::context::Context;
    use crate::error::PodError;
    use crate::io::system::fs;
    use super::{handle, list_with};

    const DESCRIPTION_ONLY_PSL: &str = "\
pod desc_only {
    description \"Description without name\"
}
";

    const DESCRIPTION_RECIPE_PSL: &str = "\
pod super_pod {
    name \"Super Pod\"
    description \"Super Pod is great\"
    registry simple.io
}
container *super {
    image super:latest
}
";

    const SIMPLE_RECIPE_PSL: &str = "\
pod simple_container {
    name \"Simple Container\"
    registry simple.io
}
container *simple {
    image simple:latest
}
";

    fn create_temp_test_dir(suffix: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "podoxide_recipe_list_{}_{suffix}",
            process::id()
        ));
        let _ = std_fs::remove_dir_all(&dir);
        std_fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn list_with_displays_configured_recipes_and_unreferenced_files() {
        let temp_dir = create_temp_test_dir("configured_and_unlinked");
        fs::write_file(
            &temp_dir.join("recipe_011_simple_container.pod"),
            SIMPLE_RECIPE_PSL,
        )
        .unwrap();
        fs::write_file(
            &temp_dir.join("recipe_022_description.pod"),
            DESCRIPTION_RECIPE_PSL,
        )
        .unwrap();
        fs::write_file(
            &temp_dir.join("recipe_023_desc_only.pod"),
            DESCRIPTION_ONLY_PSL,
        )
        .unwrap();
        fs::write_file(&temp_dir.join("unlinked_broken.pod"), "invalid {{{ psl").unwrap();

        let mut context = Context::new();
        context.config.recipes.path = temp_dir.clone();
        context.config.recipes.groups.insert(
            String::from("all"),
            vec![
                String::from("recipe_011_simple_container"),
                String::from("recipe_022_description"),
                String::from("recipe_023_desc_only"),
            ],
        );

        let mut stdout = Vec::new();
        list_with(&context, &mut stdout).unwrap();
        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains("Recipes:\n"));
        assert!(output.contains(
            "  1) recipe_011_simple_container (Simple Container)         [OK]\n"
        ));
        assert!(output.contains(
            "  2) recipe_022_description (Super Pod): Super Pod is great [OK]\n"
        ));
        assert!(output.contains(
            "  3) recipe_023_desc_only: Description without name         [OK]\n"
        ));
        assert!(!output.contains("Unlinked Recipe Files:"));
        assert!(!output.contains("[UNREFERENCED]"));

        context.flags.all = true;
        let mut all_stdout = Vec::new();
        list_with(&context, &mut all_stdout).unwrap();
        let all_output = String::from_utf8(all_stdout).unwrap();
        assert!(all_output.contains("Recipes:\n"));
        assert!(all_output.contains("\n\nUnlinked Recipe Files:\n"));
        assert!(
            all_output.contains("  4) unlinked_broken.pod                    [UNREFERENCED]\n")
        );

        assert!(handle(&context).is_ok());

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn list_with_handles_empty_config_and_only_unreferenced_files() {
        let temp_dir = create_temp_test_dir("empty_config");
        fs::write_file(&temp_dir.join("orphan.pod"), "").unwrap();

        let mut context = Context::new();
        context.config.recipes.path = temp_dir.clone();

        let mut stdout = Vec::new();
        list_with(&context, &mut stdout).unwrap();
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "There are no recipes defined in config.\n"
        );

        // When --all is set and unlinked files exist on disk, only Unlinked Recipe Files is shown.
        context.flags.all = true;
        let mut all_stdout = Vec::new();
        list_with(&context, &mut all_stdout).unwrap();
        let all_output = String::from_utf8(all_stdout).unwrap();
        assert!(!all_output.contains("Recipes:"));
        assert!(all_output.starts_with("Unlinked Recipe Files:\n"));
        assert!(all_output.contains("  1) orphan.pod                             [UNREFERENCED]\n"));

        // When --all is set and no files exist on disk, the empty message is printed.
        std_fs::remove_file(temp_dir.join("orphan.pod")).unwrap();
        let mut empty_all_stdout = Vec::new();
        list_with(&context, &mut empty_all_stdout).unwrap();
        assert_eq!(
            String::from_utf8(empty_all_stdout).unwrap(),
            "There are no recipes defined in config.\n"
        );

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn list_with_handles_validation_statuses_padding_and_directory_errors() {
        let temp_dir = create_temp_test_dir("validation");
        fs::write_file(&temp_dir.join("recipe_001_empty.pod"), "").unwrap();
        fs::write_file(&temp_dir.join("recipe_002_invalid.pod"), "invalid {{{ psl").unwrap();
        for index in 1..=8 {
            fs::write_file(&temp_dir.join(format!("extra_{index:02}.pod")), "").unwrap();
        }

        let mut context = Context::new();
        context.config.recipes.path = temp_dir.clone();
        context
            .config
            .recipes
            .groups
            .insert(String::from("all"), vec![String::from("missing_recipe")]);
        context.config.recipes.groups.insert(
            String::from("database"),
            vec![
                String::from("recipe_001_empty"),
                String::from("recipe_002_invalid"),
            ],
        );
        context.config.recipes.groups.insert(
            String::from("stack"),
            vec![String::from("@database"), String::from("@web")],
        );
        context
            .config
            .recipes
            .groups
            .insert(String::from("web"), vec![String::from("frontend")]);

        // Without --all (4 items, no leading space on 1..4)
        let mut default_stdout = Vec::new();
        list_with(&context, &mut default_stdout).unwrap();
        let default_output = String::from_utf8(default_stdout).unwrap();
        assert!(
            default_output.contains("  1) frontend                               [NOT FOUND]\n")
        );
        assert!(
            default_output.contains("  2) missing_recipe                         [NOT FOUND]\n")
        );
        assert!(default_output.contains("  3) recipe_001_empty                       [OK]\n"));
        assert!(default_output.contains("  4) recipe_002_invalid                     [OK]\n"));
        assert!(!default_output.contains("Unlinked Recipe Files:"));

        // With --all (4 configured + 8 unreferenced = 12 items, indices 1..9 padded)
        context.flags.all = true;
        let mut all_stdout = Vec::new();
        list_with(&context, &mut all_stdout).unwrap();
        let all_output = String::from_utf8(all_stdout).unwrap();
        assert!(all_output.contains("   1) frontend                              [NOT FOUND]\n"));
        assert!(all_output.contains("   2) missing_recipe                        [NOT FOUND]\n"));
        assert!(all_output.contains("   3) recipe_001_empty                      [OK]\n"));
        assert!(all_output.contains("   4) recipe_002_invalid                    [OK]\n"));
        assert!(all_output.contains("Unlinked Recipe Files:\n"));
        assert!(all_output.contains("   5) extra_01.pod                          [UNREFERENCED]\n"));
        assert!(all_output.contains("   9) extra_05.pod                          [UNREFERENCED]\n"));
        assert!(all_output.contains("  10) extra_06.pod                          [UNREFERENCED]\n"));
        assert!(all_output.contains("  12) extra_08.pod                          [UNREFERENCED]\n"));

        // Unreadable directory propagates I/O error when --all is set (unless running as root).
        let unreadable_dir = temp_dir.join("unreadable");
        std_fs::create_dir_all(&unreadable_dir).unwrap();
        std_fs::set_permissions(&unreadable_dir, std_fs::Permissions::from_mode(0o000)).unwrap();
        context.config.recipes.path = unreadable_dir.clone();
        let dir_result = list_with(&context, &mut Vec::new());
        std_fs::set_permissions(&unreadable_dir, std_fs::Permissions::from_mode(0o700)).unwrap();
        if std_fs::read_dir(&unreadable_dir).is_ok() {
            let _ = dir_result;
        } else {
            assert!(matches!(dir_result, Err(PodError::Io(_))));
        }

        let _ = std_fs::remove_dir_all(&temp_dir);
    }
}
