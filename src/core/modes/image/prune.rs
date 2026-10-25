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
use crate::io::exec::command::{self, ExecOptions};
use crate::io::system::env::MAX_CONFIRMATION_BYTES;
use crate::utilities::collection::{MAX_COLLECTION_PREALLOC, bounded_capacity};
use crate::utilities::format::{format_bytes, format_line, parse_size_to_bytes};
use crate::utilities::string::is_valid_token;

const IMAGES_FORMAT: &str =
    "{{.ID}};;;{{.Id}};;;{{.Repository}};;;{{.Tag}};;;{{.Size}};;;{{.Created}}";
const MAX_PREVIEW_RECORDS: usize = 1024;
const NONE_MARKER: &str = "<none>";
const PREVIEW_SEPARATOR: &str =
    "---------------------------------------------------------------------------";
const PREVIEW_TARGET_COLUMN: usize = 75;
const PRUNE_ALL_ARGS: [&str; 4] = ["image", "prune", "-f", "-a"];
const PRUNE_DANGLING_ARGS: [&str; 3] = ["image", "prune", "-f"];
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

/// Previews reclaimable image disk space (`--preview`) or prunes dangling and unused
/// container images (`--all`, `--force`).
///
/// # Workflow
///
/// 1. Do not read `pods.conf` or any `.pod` recipe files.
/// 2. If `context.flags.preview` is `true`:
///    - Query existing container IDs via `podman ps -aq` and inspect their attached image IDs
///      (`podman inspect -f '{{.Image}}' ...`).
///    - Query candidate images via `podman images [--filter dangling=true] --format ...`
///      (omitting the dangling filter when `context.flags.all` is `true`).
///    - Filter out images attached to existing containers, sum reclaimable byte sizes, and
///      print the formatted preview list and total reclaimable space.
/// 3. If `context.flags.preview` is `false`:
///    - Unless `context.flags.force` or `context.config.simulate` is `true`, prompt the user
///      for interactive `"yes"` confirmation before deleting images.
///    - Execute (or simulate) `podman image prune -f` (appending `-a` when `context.flags.all`
///      is `true`).
///
/// # Errors
///
/// Returns [`PodError`] if querying or pruning images via `podman` fails or interactive I/O
/// fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let mut capture = |arguments: &[&str]| command::exec_capture("podman", arguments);
    execute_with(context, &mut capture, &mut stdin, &mut stdout, &mut stderr)
}

/// Extracts valid container ID tokens from `podman ps -aq` output up to `MAX_PREVIEW_RECORDS`.
#[must_use]
fn collect_container_ids(ps_output: &str) -> Vec<&str> {
    let line_count = ps_output.lines().count();
    if line_count == 0 {
        return Vec::new();
    }
    let capacity = bounded_capacity(line_count, MAX_COLLECTION_PREALLOC);
    let mut container_ids = Vec::with_capacity(capacity);
    for line in ps_output.lines() {
        if container_ids.len() >= MAX_PREVIEW_RECORDS {
            break;
        }
        let trimmed = line.trim();
        if is_valid_token(trimmed) {
            container_ids.push(trimmed);
        }
    }
    container_ids
}

/// Queries `podman ps -aq` and `podman inspect` via `capture` to collect image IDs attached to existing containers.
#[must_use]
fn collect_in_use_images<F>(capture: &mut F) -> BTreeSet<String>
where
    F: FnMut(&[&str]) -> Option<String>,
{
    let Some(ps_output) = capture(&["ps", "-aq"]) else {
        return BTreeSet::new();
    };
    let container_ids = collect_container_ids(&ps_output);
    if container_ids.is_empty() {
        return BTreeSet::new();
    }
    let mut inspect_args = Vec::with_capacity(container_ids.len().saturating_add(3));
    inspect_args.push("inspect");
    inspect_args.push("-f");
    inspect_args.push("{{.Image}}");
    inspect_args.extend(container_ids);
    let Some(inspect_output) = capture(&inspect_args) else {
        return BTreeSet::new();
    };
    parse_in_use_output(&inspect_output)
}

/// Prompts the user on `stdout` and reads `reader` to confirm image pruning, returning `true` only when the user answers `"yes"`.
fn confirm_prune<R: BufRead, W: Write>(
    is_all: bool,
    reader: &mut R,
    stdout: &mut W,
) -> Result<bool, PodError> {
    let scope = if is_all { "all unused" } else { "dangling" };
    let prompt = format!("Are you sure you want to prune {scope} images? Type 'yes' to proceed: ");
    stdout.write_all(prompt.as_bytes())?;
    stdout.flush()?;
    let Some(input) = read_confirmation_line(reader)? else {
        return Ok(false);
    };
    Ok(input.trim_end_matches(['\r', '\n']) == "yes")
}

/// Prompts for confirmation when required and executes or simulates `podman image prune -f [-a]`.
fn execute_prune<R, W, E>(
    context: &Context,
    reader: &mut R,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    R: BufRead,
    W: Write,
    E: Write,
{
    let simulate = context.config.simulate || context.flags.simulate;
    if !context.flags.force && !simulate && !confirm_prune(context.flags.all, reader, stdout)? {
        log::write_print(stdout, "Aborted.");
        return Ok(());
    }
    let arguments: &[&str] = if context.flags.all {
        &PRUNE_ALL_ARGS
    } else {
        &PRUNE_DANGLING_ARGS
    };
    command::exec_with(
        "podman",
        arguments,
        &ExecOptions {
            interactive: false,
            prefix: PRUNE_PREFIX,
            simulate,
        },
        stdout,
        stderr,
    )
}

/// Routes `pods image prune` to either `--preview` space calculation or `podman image prune` execution.
fn execute_with<R, W, E, F>(
    context: &Context,
    capture: &mut F,
    reader: &mut R,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<(), PodError>
where
    R: BufRead,
    W: Write,
    E: Write,
    F: FnMut(&[&str]) -> Option<String>,
{
    if context.flags.preview {
        return preview_images(context.flags.all, capture, stdout);
    }
    execute_prune(context, reader, stdout, stderr)
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

/// Collects non-empty trimmed image IDs from `podman inspect -f '{{.Image}}'` output up to `MAX_PREVIEW_RECORDS`.
#[must_use]
fn parse_in_use_output(inspect_output: &str) -> BTreeSet<String> {
    let mut in_use = BTreeSet::new();
    for line in inspect_output.lines() {
        if in_use.len() >= MAX_PREVIEW_RECORDS {
            break;
        }
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            in_use.insert(String::from(trimmed));
        }
    }
    in_use
}

/// Queries in-use and candidate images via `capture` and writes the reclaimable space preview to `stdout`.
fn preview_images<W, F>(is_all: bool, capture: &mut F, stdout: &mut W) -> Result<(), PodError>
where
    W: Write,
    F: FnMut(&[&str]) -> Option<String>,
{
    let in_use_images = collect_in_use_images(capture);
    let output = query_candidate_images(is_all, capture)?;
    let line_count = output.lines().count();
    let mut preview_lines =
        Vec::with_capacity(bounded_capacity(line_count, MAX_COLLECTION_PREALLOC));
    let mut total_bytes: u64 = 0;

    for line in output.lines() {
        if preview_lines.len() >= MAX_PREVIEW_RECORDS {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let Some(record) = parse_image_record(line) else {
            continue;
        };
        if in_use_images.contains(record.long_id) {
            continue;
        }
        total_bytes = total_bytes.saturating_add(parse_size_to_bytes(record.size));
        preview_lines.push(format_preview_entry(&record));
    }

    write_preview_results(&preview_lines, total_bytes, stdout);
    Ok(())
}

/// Queries `podman images` (with `--filter dangling=true` when `is_all` is `false`) via `capture`.
fn query_candidate_images<F>(is_all: bool, capture: &mut F) -> Result<String, PodError>
where
    F: FnMut(&[&str]) -> Option<String>,
{
    let output = if is_all {
        capture(&["images", "--format", IMAGES_FORMAT])
    } else {
        capture(&[
            "images",
            "--filter",
            "dangling=true",
            "--format",
            IMAGES_FORMAT,
        ])
    };
    output.ok_or_else(|| PodError::from(io::Error::other("Failed to query podman images.")))
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

/// Writes the formatted preview list and total reclaimable space (or `"No images found to prune."`) to `stdout`.
fn write_preview_results<W: Write>(preview_lines: &[String], total_bytes: u64, stdout: &mut W) {
    if preview_lines.is_empty() {
        log::write_print(stdout, "No images found to prune.");
        return;
    }
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
    use std::io::{self, Cursor, Write};

    use crate::core::context::Context;
    use crate::error::PodError;
    use crate::io::system::env::MAX_CONFIRMATION_BYTES;
    use super::{
        IMAGES_FORMAT, MAX_PREVIEW_RECORDS, PREVIEW_SEPARATOR, execute_with, handle,
        parse_in_use_output,
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
        let mut capture = |_: &[&str]| None;
        execute_with(
            &sim_context,
            &mut capture,
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
        flag_sim_context.config.simulate = false;
        flag_sim_context.flags.simulate = true;
        flag_sim_context.flags.all = true;
        flag_sim_context.flags.force = true;
        let mut all_stdout = Vec::new();
        execute_with(
            &flag_sim_context,
            &mut capture,
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

        // Interactive confirmation declined or EOF prints "Aborted." without invoking podman.
        let mut interactive_context = Context::new();
        interactive_context.config.simulate = false;
        interactive_context.flags.simulate = false;
        interactive_context.flags.force = false;

        for (is_all, expected_prompt, raw_input) in [
            (
                false,
                "Are you sure you want to prune dangling images? Type 'yes' to proceed: ",
                &b"no\n"[..],
            ),
            (
                true,
                "Are you sure you want to prune all unused images? Type 'yes' to proceed: ",
                &b""[..],
            ),
            (
                false,
                "Are you sure you want to prune dangling images? Type 'yes' to proceed: ",
                &vec![b'y'; MAX_CONFIRMATION_BYTES + 1][..],
            ),
        ] {
            interactive_context.flags.all = is_all;
            let mut input_cursor = Cursor::new(raw_input.to_vec());
            let mut out = Vec::new();
            let mut err = Vec::new();
            execute_with(
                &interactive_context,
                &mut capture,
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
        let prompt_err = execute_with(
            &interactive_context,
            &mut capture,
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

        // 1. --all omits --filter dangling=true and prints "No images found to prune." when empty
        let mut captured_images_args = Vec::new();
        let mut empty_capture = |arguments: &[&str]| -> Option<String> {
            if arguments.first() == Some(&"images") {
                captured_images_args = arguments.iter().map(|item| (*item).to_owned()).collect();
                return Some(String::new());
            }
            None
        };
        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        execute_with(
            &context,
            &mut empty_capture,
            &mut reader,
            &mut stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(
            captured_images_args,
            vec!["images", "--format", IMAGES_FORMAT]
        );
        assert_eq!(
            String::from_utf8(stdout).unwrap(),
            "No images found to prune.\n"
        );

        // 2. All candidate images in use -> prints "No images found to prune."
        let mut all_in_use_capture = |arguments: &[&str]| -> Option<String> {
            match arguments.first().copied() {
                Some("ps") => Some(String::from("c1\n")),
                Some("inspect") => Some(String::from("long1\n")),
                Some("images") => Some(String::from(
                    "abcd1234efgh;;;long1;;;my-repo;;;1.0;;;10.5 MB;;;2 weeks ago\n",
                )),
                _ => None,
            }
        };
        let mut in_use_stdout = Vec::new();
        execute_with(
            &context,
            &mut all_in_use_capture,
            &mut reader,
            &mut in_use_stdout,
            &mut stderr,
        )
        .unwrap();
        assert_eq!(
            String::from_utf8(in_use_stdout).unwrap(),
            "No images found to prune.\n"
        );

        // 3. Failed `podman inspect` falls back cleanly and still previews candidate images
        let mut inspect_fail_capture = |arguments: &[&str]| -> Option<String> {
            match arguments.first().copied() {
                Some("ps") => Some(String::from("c1\n")),
                Some("inspect") => None,
                Some("images") => Some(String::from(
                    "abcd1234efgh;;;long1;;;my-repo;;;1.0;;;500 B;;;1 hour ago\n",
                )),
                _ => None,
            }
        };
        let mut fallback_stdout = Vec::new();
        execute_with(
            &context,
            &mut inspect_fail_capture,
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
        let mut fail_capture = |_: &[&str]| -> Option<String> { None };
        let error = execute_with(
            &context,
            &mut fail_capture,
            &mut reader,
            &mut Vec::new(),
            &mut stderr,
        )
        .unwrap_err();
        assert!(error.to_string().contains("Failed to query podman images."));

        // 5. Extreme capacity bounds (MAX_PREVIEW_RECORDS)
        let mut oversized_ps = String::new();
        let mut oversized_inspect = String::new();
        let mut oversized_images = String::new();
        for index in 0..(MAX_PREVIEW_RECORDS + 10) {
            oversized_ps.push_str(&format!("container_{index}\n"));
            oversized_inspect.push_str(&format!("used_{index}\n"));
            oversized_images.push_str(&format!(
                "id_{index};;;unused_{index};;;repo;;;tag;;;1 B;;;now\n"
            ));
        }
        assert_eq!(parse_in_use_output(&oversized_inspect).len(), MAX_PREVIEW_RECORDS);

        let mut bound_capture = |arguments: &[&str]| -> Option<String> {
            match arguments.first().copied() {
                Some("ps") => Some(oversized_ps.clone()),
                Some("inspect") => Some(String::new()),
                Some("images") => Some(oversized_images.clone()),
                _ => None,
            }
        };
        let mut bound_stdout = Vec::new();
        execute_with(
            &context,
            &mut bound_capture,
            &mut reader,
            &mut bound_stdout,
            &mut stderr,
        )
        .unwrap();
        let bound_output = String::from_utf8(bound_stdout).unwrap();
        assert_eq!(bound_output.lines().count(), MAX_PREVIEW_RECORDS + 3);
    }

    #[test]
    fn execute_with_preview_queries_podman_and_excludes_in_use_images() {
        let mut context = Context::new();
        context.flags.preview = true;
        context.flags.all = false;

        let mut captured_calls: Vec<Vec<String>> = Vec::new();
        let mut capture = |arguments: &[&str]| -> Option<String> {
            captured_calls.push(arguments.iter().map(|item| (*item).to_owned()).collect());
            match arguments.first().copied() {
                Some("ps") => Some(String::from("c1\n--bad-flag\n\nc2\n")),
                Some("inspect") => Some(String::from("  long2  \n\n")),
                Some("images") => Some(String::from(
                    "abcd1234efgh;;;long1;;;my-repo;;;1.0;;;10.5 MB;;;2 weeks ago\n\
                     ijkl5678mnop;;;long2;;;<none>;;;<none>;;;500 kB;;;3 weeks ago\n\
                     qrst9012uvwx;;;long3;;;<none>;;;<none>;;;500 kB;;;4 weeks ago\n\
                     yzab3456cdef;;;long4;;;untagged-repo;;;<none>;;;1.5 MB;;;5 weeks ago;;;extra\n\
                     malformed;;;line\n\n",
                )),
                _ => None,
            }
        };

        let mut reader = Cursor::new(Vec::<u8>::new());
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        execute_with(&context, &mut capture, &mut reader, &mut stdout, &mut stderr).unwrap();

        assert_eq!(captured_calls.len(), 3);
        assert_eq!(captured_calls[0], vec!["ps", "-aq"]);
        assert_eq!(
            captured_calls[1],
            vec!["inspect", "-f", "{{.Image}}", "c1", "c2"]
        );
        assert_eq!(
            captured_calls[2],
            vec![
                "images",
                "--filter",
                "dangling=true",
                "--format",
                IMAGES_FORMAT,
            ]
        );

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
