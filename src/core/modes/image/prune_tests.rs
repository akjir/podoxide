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

//! Unit tests for `pods image prune` deduplication, interactive confirmation, and preview rendering in [`super`].

use std::collections::BTreeSet;
use std::io::{self, Cursor, Write};

use crate::core::context::Context;
use crate::error::{PodError, SystemError};
use crate::io::system::env::MAX_CONFIRMATION_BYTES;
use super::{MAX_PREVIEW_RECORDS, execute_with, handle, scan_prunable_images};

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
fn execute_with_confirm_prompt_writes_stderr_and_aborts_on_decline_or_eof() {
    let mut sim_context = Context::new();
    sim_context.config.simulate = true;
    sim_context.flags.set_force(false);

    let mut reader = Cursor::new(Vec::<u8>::new());
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut no_in_use = || Ok(BTreeSet::new());
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
    flag_sim_context.flags.set_simulate(true);
    flag_sim_context.flags.set_all(true);
    flag_sim_context.flags.set_force(true);
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

    let mut interactive_context = Context::new();
    interactive_context.config.simulate = false;
    interactive_context.flags.set_simulate(false);
    interactive_context.flags.set_force(false);

    let mut empty_images = |_: bool| Some(String::new());
    for force in [false, true] {
        interactive_context.flags.set_force(force);
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
    interactive_context.flags.set_force(false);

    let single_image_line =
        "abcd1234efgh\x1flong1\x1f<none>\x1f<none>\x1f500 B\x1f1 hour ago\x1e\n";
    let three_images_lines = "\
abcd1234efgh\x1flong1\x1fmy-repo\x1f1.0\x1f10.5 MB\x1f2 weeks ago\x1e\n\
ijkl5678mnop\x1flong2\x1f<none>\x1f<none>\x1f500 kB\x1f3 weeks ago\x1e\n\
qrst9012uvwx\x1flong3\x1funtagged-repo\x1f<none>\x1f1.5 MB\x1f5 weeks ago\x1e\n";

    for (is_all, images_output, expected_prompt, raw_input, expects_newline) in [
        (
            false,
            single_image_line,
            "Are you sure you want to prune 1 dangling image (500 B)? Type 'yes' to proceed: ",
            &b"no\n"[..],
            false,
        ),
        (
            false,
            three_images_lines,
            "Are you sure you want to prune 3 dangling images (12.5 MB)? Type 'yes' to proceed: ",
            &b"no\n"[..],
            false,
        ),
        (
            true,
            single_image_line,
            "Are you sure you want to prune 1 unused image (500 B)? Type 'yes' to proceed: ",
            &b""[..],
            true,
        ),
        (
            true,
            three_images_lines,
            "Are you sure you want to prune 3 unused images (12.5 MB)? Type 'yes' to proceed: ",
            &vec![b'y'; MAX_CONFIRMATION_BYTES + 1][..],
            true,
        ),
    ] {
        interactive_context.flags.set_all(is_all);
        let mut candidate_images = |_: bool| Some(String::from(images_output));
        let mut input_cursor = Cursor::new(raw_input.to_vec());
        let mut out = Vec::new();
        let mut err = Vec::new();
        let result = execute_with(
            &interactive_context,
            &mut no_in_use,
            &mut candidate_images,
            &mut input_cursor,
            &mut out,
            &mut err,
        );
        assert!(
            matches!(result, Err(PodError::System(SystemError::AbortedExecution))),
            "expected AbortedExecution for declined/EOF prompt, got {result:?}"
        );
        assert!(
            out.is_empty(),
            "confirmation prompt must not write to stdout"
        );
        let expected_stderr = if expects_newline {
            format!("{expected_prompt}\n")
        } else {
            String::from(expected_prompt)
        };
        assert_eq!(String::from_utf8(err).unwrap(), expected_stderr);
    }

    let mut broken_err = BrokenPipeWriter;
    let mut yes_cursor = Cursor::new(b"yes\n".to_vec());
    let mut single_images = |_: bool| Some(String::from(single_image_line));
    let prompt_err = execute_with(
        &interactive_context,
        &mut no_in_use,
        &mut single_images,
        &mut yes_cursor,
        &mut Vec::new(),
        &mut broken_err,
    )
    .unwrap_err();
    assert!(matches!(prompt_err, PodError::Io(_)));
}

#[test]
fn execute_with_deduplicates_multitag_images_and_aligns_long_names() {
    let multi_tag_output = "\
abcd1234efgh\x1fsha256:shared\x1fmy-repo\x1f1.0\x1f100 MB\x1f2 weeks ago\x1e\n\
abcd1234efgh\x1fsha256:shared\x1fmy-repo\x1flatest\x1f100 MB\x1f2 weeks ago\x1e\n\
abcd1234efgh\x1fsha256:shared\x1fmy-repo\x1fstable\x1f100 MB\x1f2 weeks ago\x1e\n";

    let mut no_in_use = || Ok(BTreeSet::new());
    let mut multi_tag_images = |_: bool| Some(String::from(multi_tag_output));
    let scan = scan_prunable_images(true, true, &mut no_in_use, &mut multi_tag_images).unwrap();
    assert_eq!(
        scan.image_count, 1,
        "3 tags sharing one long_id must count as 1 image"
    );
    assert_eq!(
        scan.total_bytes, 100_000_000,
        "3 tags sharing one long_id must sum size once (100 MB)"
    );
    assert_eq!(scan.preview_entries.len(), 1);
    assert_eq!(scan.preview_entries[0].display_name, "my-repo:1.0");

    let mut context = Context::new();
    context.config.simulate = false;
    context.flags.set_all(true);
    context.flags.set_preview(true);

    let long_names_output = "\
id1\x1flong1\x1fshort\x1f1.0\x1f10.5 MB\x1f2 weeks ago\x1e\n\
id2\x1flong2\x1fregistry.example.com/organization/project/very-long-service-image\x1fv1.2.3\x1f250 MB\x1f3 days ago\x1e\n";
    let mut long_images = |_: bool| Some(String::from(long_names_output));
    let mut preview_out = Vec::new();
    let mut preview_err = Vec::new();
    execute_with(
        &context,
        &mut no_in_use,
        &mut long_images,
        &mut Cursor::new(Vec::new()),
        &mut preview_out,
        &mut preview_err,
    )
    .unwrap();
    assert!(preview_err.is_empty());
    let preview_text = String::from_utf8(preview_out).unwrap();
    let bullet_lines: Vec<&str> = preview_text
        .lines()
        .filter(|line| line.starts_with("  - "))
        .collect();
    assert_eq!(bullet_lines.len(), 2);
    let created_col_0 = bullet_lines[0].find('(').unwrap();
    let created_col_1 = bullet_lines[1].find('(').unwrap();
    assert_eq!(
        created_col_0, created_col_1,
        "(created) columns must align for names > 45 chars:\n{preview_text}"
    );
    let size_col_0 = bullet_lines[0].find("10.5 MB").unwrap();
    let size_col_1 = bullet_lines[1].find("250 MB").unwrap();
    assert_eq!(
        size_col_0, size_col_1,
        "size columns must align for names > 45 chars:\n{preview_text}"
    );
    let total_line = preview_text
        .lines()
        .find(|line| line.starts_with("Total space reclaimable:"))
        .unwrap();
    assert_eq!(
        total_line.find("260.5 MB").unwrap(),
        size_col_0,
        "total reclaimable column must align with entry sizes:\n{preview_text}"
    );
}

#[test]
fn execute_with_preview_handles_all_flag_empty_results_bounds_and_errors() {
    let mut context = Context::new();
    context.flags.set_preview(true);
    context.flags.set_all(true);

    let mut captured_is_all = false;
    let mut no_in_use = || Ok(BTreeSet::new());
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

    let mut in_use_long1 = || Ok(BTreeSet::from([String::from("long1")]));
    let mut one_image = |_: bool| {
        Some(String::from(
            "abcd1234efgh\x1flong1\x1fmy-repo\x1f1.0\x1f10.5 MB\x1f2 weeks ago\x1e\n",
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

    let mut fail_in_use = || -> Result<BTreeSet<String>, PodError> {
        Err(PodError::from(io::Error::other(
            "Failed to query in-use podman images.",
        )))
    };
    let in_use_err = execute_with(
        &context,
        &mut fail_in_use,
        &mut one_image,
        &mut reader,
        &mut Vec::new(),
        &mut stderr,
    )
    .unwrap_err();
    assert!(
        in_use_err
            .to_string()
            .contains("Failed to query in-use podman images.")
    );

    let mut fallback_images = |_: bool| {
        Some(String::from(
            "abcd1234efgh\x1flong1\x1fmy-repo\x1f1.0\x1f500 B\x1f1 hour ago\x1e\n",
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

    let mut oversized_images = String::new();
    for index in 0..(MAX_PREVIEW_RECORDS + 10) {
        oversized_images.push_str(&format!(
            "id_{index}\x1funused_{index}\x1frepo\x1ftag\x1f1 B\x1fnow\x1e\n"
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
    assert!(scan.preview_entries.is_empty());
    assert_eq!(scan.total_bytes, MAX_PREVIEW_RECORDS as u64);
}

#[test]
fn execute_with_preview_queries_podman_and_excludes_in_use_images() {
    let mut context = Context::new();
    context.flags.set_preview(true);
    context.flags.set_all(false);

    let mut in_use = || Ok(BTreeSet::from([String::from("long2")]));
    let mut query_images = |is_all: bool| -> Option<String> {
        assert!(!is_all);
        Some(String::from(
            "abcd1234efgh\x1flong1\x1fmy-repo\x1f1.0\x1f10.5 MB\x1f2 weeks ago\x1e\n\
             ijkl5678mnop\x1flong2\x1f<none>\x1f<none>\x1f500 kB\x1f3 weeks ago\x1e\n\
             qrst9012uvwx\x1flong3\x1f<none>\x1f<none>\x1f500 kB\x1f4 weeks ago\x1e\n\
             yzab3456cdef\x1flong4\x1funtagged-repo\x1f<none>\x1f1.5 MB\x1f5 weeks ago\x1fextra\x1e\n\
             malformed\x1fline\x1e\n\n",
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
    assert!(
        output.contains(
            "---------------------------------------------------------------------------"
        )
    );
    assert!(output.contains("Total space reclaimable:"));
    assert!(output.contains("12.5 MB\n"));
    assert!(stderr.is_empty());
}
