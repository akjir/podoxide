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

//! Handler for `pods image prune`.

use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::error::{PodError, SystemError};
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::{self, FIELD_DELIMITER, RECORD_DELIMITER};
use crate::io::system::env;
use crate::utilities::collection::{self, MAX_COLLECTION_PREALLOC};
use crate::utilities::format;
use super::preview::{self, PreviewEntry};

const MAX_PREVIEW_RECORDS: usize = 1024;
const NONE_MARKER: &str = "<none>";
const PRUNE_PREFIX: &str = "Prune images: ";

/// Parsed metadata fields from a single `podman images --format` output line.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ImageRecord<'a> {
    created: &'a str,
    id: &'a str,
    long_id: &'a str,
    repository: &'a str,
    size: &'a str,
    tag: &'a str,
}

/// Aggregated distinct image count, preview entries, and total reclaimable bytes for prunable images.
#[derive(Debug, Default, Eq, PartialEq)]
struct PruneScan {
    image_count: usize,
    preview_entries: Vec<PreviewEntry>,
    total_bytes: u64,
}

/// Previews reclaimable image disk space (`--preview`) or prunes dangling and unused
/// container images (`--all`, `--force`).
///
/// # Workflow
///
/// 1. Do not read any `.pod` recipe files.
/// 2. When `context.flags.preview` is `false` and `context.config.simulate` is `true`,
///    simulate `podman image prune -f` (appending `-a` when `context.flags.all` is `true`).
/// 3. Otherwise, query in-use container images and candidate images via the Podman API,
///    filtering out images attached to existing containers, deduplicating multi-tagged images by
///    `long_id`, and summing reclaimable byte sizes in a single pass.
/// 4. If no prunable images exist, print `"No images found to prune."` and return.
/// 5. If `context.flags.preview` is `true`, print the dynamically aligned preview list and total
///    reclaimable space and return.
/// 6. Unless `context.flags.force` is `true`, prompt the user on `stderr` for interactive `"yes"`
///    confirmation (including the distinct prunable image count and total reclaimable space)
///    before executing `podman image prune -f` (`-a`). Declining or EOF aborts with
///    [`SystemError::AbortedExecution`].
///
/// # Errors
///
/// Returns [`PodError`] if querying or pruning images via `podman` fails, interactive I/O fails,
/// or the user declines interactive confirmation.
pub fn handle(context: &Context) -> Result<ExecOutcome, PodError> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut query_in_use = podman::query_in_use_images;
    let mut query_images = podman::query_images;
    execute_with(
        context,
        &mut query_in_use,
        &mut query_images,
        &mut stdin,
        &mut stdout,
        &mut stderr,
    )
}

/// Prompts the user on `stderr` and reads `reader` to confirm image pruning, returning `Ok(())` only when the user answers `"yes"`.
///
/// # Errors
///
/// Returns [`PodError::Io`] if writing the prompt or reading from `reader` fails, or
/// [`PodError::System`] with [`SystemError::AbortedExecution`] if the user declines, sends EOF,
/// or exceeds the confirmation input byte bound.
fn confirm_prune<R: BufRead, E: Write>(
    is_all: bool,
    image_count: usize,
    total_bytes: u64,
    reader: &mut R,
    stderr: &mut E,
) -> Result<(), PodError> {
    let scope = if is_all { "unused" } else { "dangling" };
    let noun = if image_count == 1 { "image" } else { "images" };
    let reclaimable = format::format_bytes(total_bytes);
    let prompt = format!(
        "Are you sure you want to prune {image_count} {scope} {noun} ({reclaimable})? Type 'yes' to proceed: "
    );
    stderr.write_all(prompt.as_bytes())?;
    stderr.flush()?;
    let Some(input) = env::read_bounded_confirmation_line(reader)? else {
        let _ = stderr.write_all(b"\n");
        let _ = stderr.flush();
        return Err(PodError::from(SystemError::AbortedExecution));
    };
    if !input.ends_with('\n') {
        let _ = stderr.write_all(b"\n");
        let _ = stderr.flush();
    }
    if input.trim_end_matches(['\r', '\n']) != "yes" {
        return Err(PodError::from(SystemError::AbortedExecution));
    }
    Ok(())
}

/// Orchestrates `pods image prune` across simulate, `--preview`, and interactive or forced prune execution.
fn execute_with<R, W, E, F, G>(
    context: &Context,
    query_in_use: &mut F,
    query_images: &mut G,
    reader: &mut R,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError>
where
    R: BufRead,
    W: Write,
    E: Write,
    F: FnMut() -> Result<BTreeSet<String>, PodError>,
    G: FnMut(bool) -> Option<String>,
{
    let is_all = context.flags.all();
    let is_preview = context.flags.preview();
    if !is_preview && context.config.simulate {
        return podman::prune_images(is_all, PRUNE_PREFIX, true, stdout, stderr);
    }
    let scan = scan_prunable_images(is_all, is_preview, query_in_use, query_images)?;
    if scan.image_count == 0 {
        log::write_print(stdout, "No images found to prune.");
        return Ok(ExecOutcome::Succeeded);
    }
    if is_preview {
        preview::write_preview_results(&scan.preview_entries, scan.total_bytes, stdout);
        return Ok(ExecOutcome::Succeeded);
    }
    if !context.flags.force() {
        confirm_prune(is_all, scan.image_count, scan.total_bytes, reader, stderr)?;
    }
    podman::prune_images(is_all, PRUNE_PREFIX, false, stdout, stderr)
}

/// Resolves the human-readable image label (`<id>`, `<repo>:<tag>`, or `<repo>`) for `record`.
#[must_use]
fn format_display_name(record: &ImageRecord<'_>) -> String {
    if record.repository == NONE_MARKER {
        return String::from(record.id);
    }
    if record.tag != NONE_MARKER {
        return format!("{}:{}", record.repository, record.tag);
    }
    String::from(record.repository)
}

/// Parses `line` and returns the [`ImageRecord`] when it is not in `in_use_images`.
#[must_use]
fn parse_candidate_record<'a>(
    line: &'a str,
    in_use_images: &BTreeSet<String>,
) -> Option<ImageRecord<'a>> {
    if line.trim().is_empty() {
        return None;
    }
    let record = parse_image_record(line)?;
    if in_use_images.contains(record.long_id) {
        return None;
    }
    Some(record)
}

/// Parses a control-delimited `podman images` record into an [`ImageRecord`].
#[must_use]
fn parse_image_record(record: &str) -> Option<ImageRecord<'_>> {
    let (id, rest) = record.split_once(FIELD_DELIMITER)?;
    let (long_id, rest) = rest.split_once(FIELD_DELIMITER)?;
    let (repository, rest) = rest.split_once(FIELD_DELIMITER)?;
    let (tag, rest) = rest.split_once(FIELD_DELIMITER)?;
    let (size, rest) = rest.split_once(FIELD_DELIMITER)?;
    let created = rest
        .split_once(FIELD_DELIMITER)
        .map_or(rest, |(first, _)| first);
    Some(ImageRecord {
        created,
        id,
        long_id,
        repository,
        size,
        tag,
    })
}

/// Queries Podman via `query_in_use` and `query_images`, filters out in-use images, deduplicates multi-tagged images by `long_id`, and accumulates the prunable image count, reclaimable bytes, and optional preview entries.
fn scan_prunable_images<F, G>(
    is_all: bool,
    include_entries: bool,
    query_in_use: &mut F,
    query_images: &mut G,
) -> Result<PruneScan, PodError>
where
    F: FnMut() -> Result<BTreeSet<String>, PodError>,
    G: FnMut(bool) -> Option<String>,
{
    let in_use_images = query_in_use()?;
    let output = query_images(is_all)
        .ok_or_else(|| PodError::from(io::Error::other("Failed to query podman images.")))?;
    let preview_entries = if include_entries {
        let record_count = output
            .bytes()
            .filter(|&byte| byte == RECORD_DELIMITER as u8)
            .count();
        let capacity = collection::bounded_capacity(record_count, MAX_COLLECTION_PREALLOC);
        Vec::with_capacity(capacity)
    } else {
        Vec::new()
    };
    let mut scan = PruneScan {
        image_count: 0,
        preview_entries,
        total_bytes: 0,
    };
    let mut seen_ids = BTreeSet::new();

    for raw_record in output.split(RECORD_DELIMITER) {
        if scan.image_count >= MAX_PREVIEW_RECORDS {
            break;
        }
        let record_text = raw_record.trim_start_matches(['\r', '\n']);
        let Some(record) = parse_candidate_record(record_text, &in_use_images) else {
            continue;
        };
        let dedup_key = if record.long_id.is_empty() {
            record.id
        } else {
            record.long_id
        };
        if !seen_ids.insert(dedup_key) {
            continue;
        }
        scan.image_count += 1;
        scan.total_bytes = scan
            .total_bytes
            .saturating_add(format::parse_size_to_bytes(record.size));
        if include_entries {
            scan.preview_entries.push(PreviewEntry {
                created: String::from(record.created),
                display_name: format_display_name(&record),
                size: String::from(record.size),
            });
        }
    }

    Ok(scan)
}

#[cfg(test)]
#[path = "prune_tests.rs"]
mod tests;
