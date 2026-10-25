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

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::io::{self, BufRead, Read, Write};

use crate::core::context::Context;
use crate::core::log;
use crate::error::PodError;
use crate::io::exec::podman;
use crate::io::system::env::MAX_CONFIRMATION_BYTES;
use crate::utilities::collection::{MAX_COLLECTION_PREALLOC, bounded_capacity};
use crate::utilities::format::{format_bytes, format_line, parse_size_to_bytes};

const MAX_PREVIEW_RECORDS: usize = 1024;
const NONE_MARKER: &str = "<none>";
const PREVIEW_SEPARATOR: &str =
    "---------------------------------------------------------------------------";
const PREVIEW_TARGET_COLUMN: usize = 75;
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

/// Aggregated count, formatted preview lines, and total reclaimable bytes for prunable images.
#[derive(Debug, Default, Eq, PartialEq)]
struct PruneScan {
    image_count: usize,
    preview_lines: Vec<String>,
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
///    filtering out images attached to existing containers and summing reclaimable byte sizes
///    in a single pass.
/// 4. If no prunable images exist, print `"No images found to prune."` and return.
/// 5. If `context.flags.preview` is `true`, print the formatted preview list and total
///    reclaimable space and return.
/// 6. Unless `context.flags.force` is `true`, prompt the user for interactive `"yes"`
///    confirmation (including the prunable image count and total reclaimable space) before
///    executing `podman image prune -f` (`-a`).
///
/// # Errors
///
/// Returns [`PodError`] if querying or pruning images via `podman` fails or interactive I/O
/// fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
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

/// Prompts the user on `stdout` and reads `reader` to confirm image pruning, returning `true` only when the user answers `"yes"`.
fn confirm_prune<R: BufRead, W: Write>(
    is_all: bool,
    image_count: usize,
    total_bytes: u64,
    reader: &mut R,
    stdout: &mut W,
) -> Result<bool, PodError> {
    let scope = if is_all { "unused" } else { "dangling" };
    let noun = if image_count == 1 { "image" } else { "images" };
    let reclaimable = format_bytes(total_bytes);
    let prompt = format!(
        "Are you sure you want to prune {image_count} {scope} {noun} ({reclaimable})? Type 'yes' to proceed: "
    );
    stdout.write_all(prompt.as_bytes())?;
    stdout.flush()?;
    let Some(input) = read_confirmation_line(reader)? else {
        return Ok(false);
    };
    Ok(input.trim_end_matches(['\r', '\n']) == "yes")
}

/// Orchestrates `pods image prune` across simulate, `--preview`, and interactive or forced prune execution.
fn execute_with<R, W, E, F, G>(
    context: &Context,
    query_in_use: &mut F,
    query_images: &mut G,
    reader: &mut R,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    R: BufRead,
    W: Write,
    E: Write,
    F: FnMut() -> BTreeSet<String>,
    G: FnMut(bool) -> Option<String>,
{
    let is_all = context.flags.all;
    let is_preview = context.flags.preview;
    if !is_preview && context.config.simulate {
        return podman::prune_images(is_all, PRUNE_PREFIX, true, stdout, stderr);
    }
    let scan = scan_prunable_images(is_all, is_preview, query_in_use, query_images)?;
    if scan.image_count == 0 {
        log::write_print(stdout, "No images found to prune.");
        return Ok(());
    }
    if is_preview {
        write_preview_results(&scan.preview_lines, scan.total_bytes, stdout);
        return Ok(());
    }
    if !context.flags.force
        && !confirm_prune(is_all, scan.image_count, scan.total_bytes, reader, stdout)?
    {
        log::write_print(stdout, "Aborted.");
        return Ok(());
    }
    podman::prune_images(is_all, PRUNE_PREFIX, false, stdout, stderr)
}

/// Resolves the human-readable image label (`<id>`, `<repo>:<tag>`, or `<repo>`) for `record`.
#[must_use]
fn format_display_name<'a>(record: &ImageRecord<'a>) -> Cow<'a, str> {
    if record.repository == NONE_MARKER {
        return Cow::Borrowed(record.id);
    }
    if record.tag != NONE_MARKER {
        return Cow::Owned(format!("{}:{}", record.repository, record.tag));
    }
    Cow::Borrowed(record.repository)
}

/// Formats a single candidate image record as an aligned preview bullet line.
#[must_use]
fn format_preview_entry(record: &ImageRecord<'_>) -> String {
    let display_name = format_display_name(record);
    let left_side = format!("  - {:<45} ({})", display_name, record.created);
    format_line(&left_side, record.size, PREVIEW_TARGET_COLUMN)
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

/// Parses a `;;;`-delimited `podman images` line into an [`ImageRecord`].
#[must_use]
fn parse_image_record(line: &str) -> Option<ImageRecord<'_>> {
    let (id, rest) = line.split_once(";;;")?;
    let (long_id, rest) = rest.split_once(";;;")?;
    let (repository, rest) = rest.split_once(";;;")?;
    let (tag, rest) = rest.split_once(";;;")?;
    let (size, rest) = rest.split_once(";;;")?;
    let created = rest.split_once(";;;").map_or(rest, |(first, _)| first);
    Some(ImageRecord {
        created,
        id,
        long_id,
        repository,
        size,
        tag,
    })
}

/// Reads at most [`MAX_CONFIRMATION_BYTES`] bytes of a single confirmation line from `reader`.
fn read_confirmation_line<R: BufRead>(reader: &mut R) -> Result<Option<String>, PodError> {
    let mut buffer = String::with_capacity(MAX_CONFIRMATION_BYTES);
    let mut bounded = reader.take((MAX_CONFIRMATION_BYTES + 1) as u64);
    let bytes_read = bounded.read_line(&mut buffer)?;
    if bytes_read == 0 || buffer.len() > MAX_CONFIRMATION_BYTES {
        return Ok(None);
    }
    Ok(Some(buffer))
}

/// Queries Podman via `query_in_use` and `query_images`, filters out in-use images, and accumulates the prunable image count, reclaimable bytes, and optional preview lines.
fn scan_prunable_images<F, G>(
    is_all: bool,
    include_lines: bool,
    query_in_use: &mut F,
    query_images: &mut G,
) -> Result<PruneScan, PodError>
where
    F: FnMut() -> BTreeSet<String>,
    G: FnMut(bool) -> Option<String>,
{
    let in_use_images = query_in_use();
    let output = query_images(is_all)
        .ok_or_else(|| PodError::from(io::Error::other("Failed to query podman images.")))?;
    let preview_lines = if include_lines {
        let capacity = bounded_capacity(output.lines().count(), MAX_COLLECTION_PREALLOC);
        Vec::with_capacity(capacity)
    } else {
        Vec::new()
    };
    let mut scan = PruneScan {
        image_count: 0,
        preview_lines,
        total_bytes: 0,
    };

    for line in output.lines() {
        if scan.image_count >= MAX_PREVIEW_RECORDS {
            break;
        }
        let Some(record) = parse_candidate_record(line, &in_use_images) else {
            continue;
        };
        scan.image_count += 1;
        scan.total_bytes = scan
            .total_bytes
            .saturating_add(parse_size_to_bytes(record.size));
        if include_lines {
            scan.preview_lines.push(format_preview_entry(&record));
        }
    }

    Ok(scan)
}

/// Writes the formatted preview list and total reclaimable space to `stdout`.
fn write_preview_results<W: Write>(preview_lines: &[String], total_bytes: u64, stdout: &mut W) {
    log::write_print(stdout, "Images to be pruned:");
    for line in preview_lines {
        log::write_print(stdout, line);
    }
    log::write_print(stdout, PREVIEW_SEPARATOR);
    log::write_print(
        stdout,
        &format_line(
            "Total space reclaimable:",
            &format_bytes(total_bytes),
            PREVIEW_TARGET_COLUMN,
        ),
    );
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::io::{self, Cursor, Write};

    use crate::core::context::Context;
    use crate::error::PodError;
    use crate::io::system::env::MAX_CONFIRMATION_BYTES;
    use super::{
        MAX_PREVIEW_RECORDS, PREVIEW_SEPARATOR, execute_with, handle, scan_prunable_images,
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

    #[test]
    fn execute_with_confirm_prompt_and_simulate_prune_variants() {
        let mut sim_context = Context::new();
        sim_context.config.simulate = true;
        sim_context.flags.force = false;

        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut no_in_use = BTreeSet::new;
        let mut no_images = |_: bool| None;
        execute_with(
            &sim_context,
            &mut no_in_use,
            &mut no_images,
            &mut reader,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "Prune images: \npodman image prune -f\n"
        );
        assert!(stderr.is_empty());

        let mut flag_sim_context = Context::new();
        flag_sim_context.config.simulate = true;
        flag_sim_context.flags.simulate = true;
        flag_sim_context.flags.all = true;
        flag_sim_context.flags.force = true;
        let mut all_stdout = Vec::new();
        execute_with(
            &flag_sim_context,
            &mut no_in_use,
            &mut no_images,
            &mut reader,
            &mut all_stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(all_stdout).unwrap(),
            "Prune images: \npodman image prune -f -a\n"
        );

        assert!(handle(&sim_context).is_ok());

        // When simulate is false and no prunable images exist, prints "No images found to prune." without prompting.
        let mut interactive_context = Context::new();
        interactive_context.config.simulate = false;
        interactive_context.flags.simulate = false;
        interactive_context.flags.force = false;

        let mut empty_images = |_: bool| Some(String::new());
        for force in [false, true] {
            interactive_context.flags.force = force;
            let mut empty_out = Vec::new();
            let mut empty_err = Vec::new();
            execute_with(
                &interactive_context,
                &mut no_in_use,
                &mut empty_images,
                &mut reader,
                &mut empty_out,
                &mut empty_err,
            )
            .unwrap();
            assert_eq!(
                String::from_utf8(empty_out).unwrap(),
                "No images found to prune.\n"
            );
            assert!(empty_err.is_empty());
        }
        interactive_context.flags.force = false;

        // Interactive confirmation declined or EOF prints "Aborted." with image count and reclaimable space.
        let single_image_line = "abcd1234efgh;;;long1;;;<none>;;;<none>;;;500 B;;;1 hour ago\n";
        let three_images_lines = "\
abcd1234efgh;;;long1;;;my-repo;;;1.0;;;10.5 MB;;;2 weeks ago\n\
ijkl5678mnop;;;long2;;;<none>;;;<none>;;;500 kB;;;3 weeks ago\n\
qrst9012uvwx;;;long3;;;untagged-repo;;;<none>;;;1.5 MB;;;5 weeks ago\n";

        for (is_all, images_output, expected_prompt, raw_input) in [
            (
                false,
                single_image_line,
                "Are you sure you want to prune 1 dangling image (500 B)? Type 'yes' to proceed: ",
                &b"no\n"[..],
            ),
            (
                false,
                three_images_lines,
                "Are you sure you want to prune 3 dangling images (12.5 MB)? Type 'yes' to proceed: ",
                &b"no\n"[..],
            ),
            (
                true,
                single_image_line,
                "Are you sure you want to prune 1 unused image (500 B)? Type 'yes' to proceed: ",
                &b""[..],
            ),
            (
                true,
                three_images_lines,
                "Are you sure you want to prune 3 unused images (12.5 MB)? Type 'yes' to proceed: ",
                &vec![b'y'; MAX_CONFIRMATION_BYTES + 1][..],
            ),
        ] {
            interactive_context.flags.all = is_all;
            let mut candidate_images = |_: bool| Some(String::from(images_output));
            let mut input_cursor = Cursor::new(raw_input.to_vec());
            let mut out = Vec::new();
            let mut err = Vec::new();
            execute_with(
                &interactive_context,
                &mut no_in_use,
                &mut candidate_images,
                &mut input_cursor,
                &mut out,
                &mut err,
            )
            .unwrap();
            let rendered = String::from_utf8(out).unwrap();
            assert_eq!(rendered, format!("{expected_prompt}Aborted.\n"));
            assert!(err.is_empty());
        }

        // Broken pipe when writing confirmation prompt propagates PodError::Io.
        let mut broken_out = BrokenPipeWriter;
        let mut yes_cursor = Cursor::new(b"yes\n".to_vec());
        let mut single_images = |_: bool| Some(String::from(single_image_line));
        let prompt_err = execute_with(
            &interactive_context,
            &mut no_in_use,
            &mut single_images,
            &mut yes_cursor,
            &mut broken_out,
            &mut stderr,
        )
        .unwrap_err();
        assert!(matches!(prompt_err, PodError::Io(_)));
    }

    #[test]
    fn execute_with_preview_handles_all_flag_empty_results_bounds_and_errors() {
        let mut context = Context::new();
        context.flags.preview = true;
        context.flags.all = true;

        // 1. --all passes true to query_images and prints "No images found to prune." when empty
        let mut captured_is_all = false;
        let mut no_in_use = BTreeSet::new;
        let mut empty_images = |is_all: bool| -> Option<String> {
            captured_is_all = is_all;
            Some(String::new())
        };
        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        execute_with(
            &context,
            &mut no_in_use,
            &mut empty_images,
            &mut reader,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        assert!(captured_is_all);
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "No images found to prune.\n"
        );

        // 2. All candidate images in use -> prints "No images found to prune."
        let mut in_use_long1 = || BTreeSet::from([String::from("long1")]);
        let mut one_image = |_: bool| {
            Some(String::from(
                "abcd1234efgh;;;long1;;;my-repo;;;1.0;;;10.5 MB;;;2 weeks ago\n",
            ))
        };
        let mut in_use_stdout = Vec::new();
        execute_with(
            &context,
            &mut in_use_long1,
            &mut one_image,
            &mut reader,
            &mut in_use_stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(in_use_stdout).unwrap(),
            "No images found to prune.\n"
        );

        // 3. Empty in-use set previews candidate images cleanly
        let mut fallback_images = |_: bool| {
            Some(String::from(
                "abcd1234efgh;;;long1;;;my-repo;;;1.0;;;500 B;;;1 hour ago\n",
            ))
        };
        let mut fallback_stdout = Vec::new();
        execute_with(
            &context,
            &mut no_in_use,
            &mut fallback_images,
            &mut reader,
            &mut fallback_stdout,
            &mut stderr,
        )
        .unwrap();
        assert!(
            String::from_utf8(fallback_stdout)
                .unwrap()
                .contains("500 B\n")
        );

        // 4. Failed `podman images` returns PodError::Io containing "Failed to query podman images."
        let mut fail_images = |_: bool| -> Option<String> { None };
        let error = execute_with(
            &context,
            &mut no_in_use,
            &mut fail_images,
            &mut reader,
            &mut Vec::new(),
            &mut stderr,
        )
        .unwrap_err();
        assert!(error.to_string().contains("Failed to query podman images."));

        // 5. Extreme capacity bounds (MAX_PREVIEW_RECORDS)
        let mut oversized_images = String::new();
        for index in 0..(MAX_PREVIEW_RECORDS + 10) {
            oversized_images.push_str(&format!(
                "id_{index};;;unused_{index};;;repo;;;tag;;;1 B;;;now\n"
            ));
        }

        let mut bound_images = |_: bool| Some(oversized_images.clone());
        let mut bound_stdout = Vec::new();
        execute_with(
            &context,
            &mut no_in_use,
            &mut bound_images,
            &mut reader,
            &mut bound_stdout,
            &mut stderr,
        )
        .unwrap();
        let bound_output = String::from_utf8(bound_stdout).unwrap();
        assert_eq!(bound_output.lines().count(), MAX_PREVIEW_RECORDS + 3);
        let scan = scan_prunable_images(true, false, &mut no_in_use, &mut bound_images).unwrap();
        assert_eq!(scan.image_count, MAX_PREVIEW_RECORDS);
        assert!(scan.preview_lines.is_empty());
        assert_eq!(scan.total_bytes, MAX_PREVIEW_RECORDS as u64);
    }

    #[test]
    fn execute_with_preview_queries_podman_and_excludes_in_use_images() {
        let mut context = Context::new();
        context.flags.preview = true;
        context.flags.all = false;

        let mut in_use = || BTreeSet::from([String::from("long2")]);
        let mut query_images = |is_all: bool| -> Option<String> {
            assert!(!is_all);
            Some(String::from(
                "abcd1234efgh;;;long1;;;my-repo;;;1.0;;;10.5 MB;;;2 weeks ago\n\
                 ijkl5678mnop;;;long2;;;<none>;;;<none>;;;500 kB;;;3 weeks ago\n\
                 qrst9012uvwx;;;long3;;;<none>;;;<none>;;;500 kB;;;4 weeks ago\n\
                 yzab3456cdef;;;long4;;;untagged-repo;;;<none>;;;1.5 MB;;;5 weeks ago;;;extra\n\
                 malformed;;;line\n\n",
            ))
        };

        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        execute_with(
            &context,
            &mut in_use,
            &mut query_images,
            &mut reader,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();

        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains("Images to be pruned:\n"));
        assert!(output.contains("  - my-repo:1.0"));
        assert!(output.contains("(2 weeks ago)            10.5 MB\n"));
        assert!(output.contains("  - qrst9012uvwx"));
        assert!(output.contains("(4 weeks ago)            500 kB\n"));
        assert!(output.contains("  - untagged-repo"));
        assert!(output.contains("(5 weeks ago)            1.5 MB\n"));
        assert!(!output.contains("ijkl5678mnop"));
        assert!(output.contains(PREVIEW_SEPARATOR));
        assert!(output.contains("Total space reclaimable:"));
        assert!(output.contains("12.5 MB\n"));
        assert!(stderr.is_empty());
    }
}
