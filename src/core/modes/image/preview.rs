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

//! Dynamic column alignment and preview table rendering for `pods image prune --preview`.

use std::io::Write;
use std::iter;

use crate::core::log;
use crate::utilities::format::{self, MAX_FORMATTED_LINE_BYTES};

const BULLET_PREFIX: &str = "  - ";
const CREATED_WRAPPER_CHARS: usize = 3;
const MIN_NAME_WIDTH: usize = 45;
const MIN_SIZE_GAP: usize = 2;
const MIN_TARGET_COLUMN: usize = 75;
const TOTAL_LABEL: &str = "Total space reclaimable:";

/// Structured preview entry for a distinct prunable container image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct PreviewEntry {
    pub(super) created: String,
    pub(super) display_name: String,
    pub(super) size: String,
}

/// Computed dynamic column widths for rendering the image prune preview table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PreviewLayout {
    name_width: usize,
    separator_width: usize,
    target_column: usize,
}

/// Formats and writes the aligned image prune preview list, dynamic separator, and reclaimable space total (prefixed with `>= ` when `truncated` is `true`) to `stdout`.
pub(super) fn write_preview_results<W: Write>(
    entries: &[PreviewEntry],
    total_bytes: u64,
    truncated: bool,
    stdout: &mut W,
) {
    let reclaimable = if truncated {
        format!(">= {}", format::format_bytes(total_bytes))
    } else {
        format::format_bytes(total_bytes)
    };
    let layout = compute_preview_layout(entries, &reclaimable);
    log::write_print(stdout, "Images to be pruned:");
    for entry in entries {
        let line = format_preview_entry(entry, layout);
        log::write_print(stdout, &line);
    }
    let separator = "-".repeat(layout.separator_width);
    log::write_print(stdout, &separator);
    log::write_print(
        stdout,
        &format::format_line(TOTAL_LABEL, &reclaimable, layout.target_column),
    );
}

/// Computes dynamic name, target, and separator widths across all preview `entries` and `reclaimable`.
#[must_use]
fn compute_preview_layout(entries: &[PreviewEntry], reclaimable: &str) -> PreviewLayout {
    let mut name_width = MIN_NAME_WIDTH;
    let mut max_created_chars = 0usize;
    let mut max_size_chars = reclaimable.chars().count();
    for entry in entries {
        name_width = name_width.max(entry.display_name.chars().count());
        max_created_chars = max_created_chars.max(entry.created.chars().count());
        max_size_chars = max_size_chars.max(entry.size.chars().count());
    }
    name_width = name_width.min(MAX_FORMATTED_LINE_BYTES);
    let max_left = BULLET_PREFIX
        .len()
        .saturating_add(name_width)
        .saturating_add(CREATED_WRAPPER_CHARS)
        .saturating_add(max_created_chars)
        .max(TOTAL_LABEL.len());
    let target_column = max_left
        .saturating_add(MIN_SIZE_GAP)
        .clamp(MIN_TARGET_COLUMN, MAX_FORMATTED_LINE_BYTES);
    let separator_width = target_column
        .saturating_add(max_size_chars)
        .clamp(MIN_TARGET_COLUMN, MAX_FORMATTED_LINE_BYTES);
    PreviewLayout {
        name_width,
        separator_width,
        target_column,
    }
}

/// Formats a single [`PreviewEntry`] using the precomputed `layout` widths.
#[must_use]
fn format_preview_entry(entry: &PreviewEntry, layout: PreviewLayout) -> String {
    let name_chars = entry.display_name.chars().count();
    let pad = layout.name_width.saturating_sub(name_chars);
    let left_capacity = BULLET_PREFIX
        .len()
        .saturating_add(entry.display_name.len())
        .saturating_add(pad)
        .saturating_add(CREATED_WRAPPER_CHARS)
        .saturating_add(entry.created.len())
        .min(MAX_FORMATTED_LINE_BYTES);
    let mut left_side = String::with_capacity(left_capacity);
    left_side.push_str(BULLET_PREFIX);
    left_side.push_str(&entry.display_name);
    left_side.extend(iter::repeat_n(' ', pad));
    left_side.push_str(" (");
    left_side.push_str(&entry.created);
    left_side.push(')');
    format::format_line(&left_side, &entry.size, layout.target_column)
}
