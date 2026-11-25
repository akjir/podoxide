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

//! Compiler-style source diagnostics, caret annotation builders, and sanitized error formatters.

use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Display, Formatter, Write};
use std::iter;

use crate::error::MAX_ERROR_TOKEN_BYTES;
use crate::utilities::string;

/// Ellipsis inserted during middle-truncation of long I/O error messages.
const IO_ERROR_ELLIPSIS: &str = "...";
/// Leading byte budget preserved before the ellipsis for long I/O error messages.
const IO_ERROR_HEAD_BYTES: usize = 124;
/// Trailing byte budget preserved after the ellipsis for long I/O error messages.
const IO_ERROR_TAIL_BYTES: usize =
    MAX_ERROR_TOKEN_BYTES - IO_ERROR_HEAD_BYTES - IO_ERROR_ELLIPSIS.len();
/// Static buffer of carets for zero-allocation caret borrowing on typical line lengths.
const STATIC_CARETS: &str =
    "^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^";

/// Compiler-style diagnostic error representation for PSL, configuration, and recipe files.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticError<'a> {
    /// Caret string pointing to the error location (for example, `^^^`).
    pub annotation_carets: Cow<'a, str>,
    /// Explanation accompanying the carets.
    pub annotation_text: Cow<'a, str>,
    /// Column number where the error occurred (1-based).
    pub col_number: usize,
    /// Path to the file where the error occurred.
    pub file_path: Cow<'a, str>,
    /// Actionable advice on how to resolve the error.
    pub help_text: Cow<'a, str>,
    /// Line number where the error occurred (1-based).
    pub line_number: usize,
    /// Raw line content snippet from the source file.
    pub line_snippet: Cow<'a, str>,
}

impl DiagnosticError<'_> {
    /// Writes the location header (`--> file:line:col`), source gutter, carets, and `= help:` block.
    pub fn write_body(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let line_len = self.line_number.checked_ilog10().unwrap_or(0) as usize + 1;

        formatter.write_str(" --> ")?;
        write_sanitized_token(formatter, &self.file_path)?;
        writeln!(formatter, ":{}:{}", self.line_number, self.col_number)?;
        writeln!(formatter, "{:>width$} |", "", width = line_len)?;
        write!(
            formatter,
            "{:>width$} | ",
            self.line_number,
            width = line_len
        )?;
        write_sanitized_token(formatter, &self.line_snippet)?;
        formatter.write_char('\n')?;
        write!(formatter, "{:>width$} | ", "", width = line_len)?;
        write_sanitized_carets(formatter, &self.annotation_carets)?;
        formatter.write_char(' ')?;
        write_sanitized_token(formatter, &self.annotation_text)?;
        formatter.write_char('\n')?;
        writeln!(formatter, "{:>width$} |", "", width = line_len)?;
        formatter.write_str("= help: ")?;
        write_sanitized_token(formatter, &self.help_text)
    }
}

impl Display for DiagnosticError<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        self.write_body(formatter)
    }
}

impl Error for DiagnosticError<'_> {}

/// Constructs a diagnostic caret annotation string pointing to a 1-based source column and span.
#[must_use]
pub fn build_caret_annotation(
    col_number: usize,
    span_len: usize,
    max_len: usize,
) -> Cow<'static, str> {
    let clamped_max = max_len.clamp(1, MAX_ERROR_TOKEN_BYTES);
    let span = span_len.clamp(1, MAX_ERROR_TOKEN_BYTES);
    let max_padding = clamped_max.min(MAX_ERROR_TOKEN_BYTES - 1);
    let padding_len = col_number.saturating_sub(1).min(max_padding);

    if padding_len == 0 {
        let effective_span = span.min(clamped_max);
        if effective_span <= STATIC_CARETS.len() {
            return Cow::Borrowed(&STATIC_CARETS[..effective_span]);
        }
        return Cow::Owned("^".repeat(effective_span));
    }

    let remaining = MAX_ERROR_TOKEN_BYTES - padding_len;
    let effective_span = span
        .min(clamped_max.saturating_sub(padding_len).max(1))
        .min(remaining);
    let mut carets = String::with_capacity(padding_len + effective_span);
    carets.extend(iter::repeat_n(' ', padding_len));
    carets.extend(iter::repeat_n('^', effective_span));
    Cow::Owned(carets)
}

/// Builds an owned [`DiagnosticError`] by extracting `line_number` from `source` and resolving its span.
#[must_use]
pub(super) fn build_diagnostic_from_line<F>(
    source: &str,
    file_path: &str,
    line_number: usize,
    annotation_text: &'static str,
    help_text: &'static str,
    resolve_span: F,
) -> DiagnosticError<'static>
where
    F: FnOnce(&str) -> (usize, usize),
{
    let effective_line = line_number.max(1);
    let raw_line = extract_source_line(source, effective_line);
    let sanitized_line = string::sanitize_text(raw_line, MAX_ERROR_TOKEN_BYTES);
    let (col_number, span_len) = resolve_span(&sanitized_line);
    let max_len = sanitized_line.chars().count().max(1);
    let carets = build_caret_annotation(col_number, span_len, max_len);

    DiagnosticError {
        annotation_carets: carets,
        annotation_text: Cow::Borrowed(annotation_text),
        col_number,
        file_path: Cow::Owned(string::sanitize_text(file_path, MAX_ERROR_TOKEN_BYTES)),
        help_text: Cow::Borrowed(help_text),
        line_number: effective_line,
        line_snippet: Cow::Owned(sanitized_line),
    }
}

/// Writes an I/O error message sanitized of control characters, middle-truncating long paths to preserve the OS error reason.
pub(super) fn write_sanitized_io_message(formatter: &mut Formatter<'_>, raw: &str) -> fmt::Result {
    if raw.len() <= MAX_ERROR_TOKEN_BYTES {
        return write_sanitized_slice(formatter, raw);
    }
    let head = string::clamp_utf8_prefix(raw, IO_ERROR_HEAD_BYTES);
    let tail = clamp_utf8_suffix(raw, IO_ERROR_TAIL_BYTES);
    write_sanitized_slice(formatter, head)?;
    formatter.write_str(IO_ERROR_ELLIPSIS)?;
    write_sanitized_slice(formatter, tail)
}

/// Writes `raw` to `formatter`, clamped to [`MAX_ERROR_TOKEN_BYTES`] and sanitized of control characters.
pub(super) fn write_sanitized_token(formatter: &mut Formatter<'_>, raw: &str) -> fmt::Result {
    let clamped = string::clamp_utf8_prefix(raw, MAX_ERROR_TOKEN_BYTES);
    write_sanitized_slice(formatter, clamped)
}

/// Formats a structured error header (optional line, kind, optional sanitized token) and optional diagnostic body.
pub(super) fn write_structured_error<K: Display>(
    formatter: &mut Formatter<'_>,
    line: Option<usize>,
    kind: &K,
    token: Option<&str>,
    diagnostic: Option<&DiagnosticError<'_>>,
) -> fmt::Result {
    if let Some(line_number) = line {
        write!(formatter, "line {}: {kind}", line_number.max(1))?;
    } else {
        write!(formatter, "{kind}")?;
    }
    if let Some(value) = token {
        formatter.write_str(": '")?;
        write_sanitized_token(formatter, value)?;
        formatter.write_char('\'')?;
    }
    if let Some(detail) = diagnostic {
        formatter.write_char('\n')?;
        detail.write_body(formatter)?;
    }
    Ok(())
}

/// Truncates `input` from the front at the smallest valid UTF-8 boundary retaining at most `max_bytes` trailing bytes.
#[must_use]
fn clamp_utf8_suffix(input: &str, max_bytes: usize) -> &str {
    if input.len() <= max_bytes {
        return input;
    }
    let mut start = input.len() - max_bytes;
    while start < input.len() && !input.is_char_boundary(start) {
        start += 1;
    }
    &input[start..]
}

/// Extracts the 1-based `line_number` from `source`, or returns an empty slice when out of bounds.
#[must_use]
fn extract_source_line(source: &str, line_number: usize) -> &str {
    source
        .lines()
        .nth(line_number.saturating_sub(1))
        .unwrap_or("")
}

/// Writes `carets` to `formatter`, clamping to [`MAX_ERROR_TOKEN_BYTES`] while keeping at least one `'^'` visible if present.
fn write_sanitized_carets(formatter: &mut Formatter<'_>, carets: &str) -> fmt::Result {
    let clamped = string::clamp_utf8_prefix(carets, MAX_ERROR_TOKEN_BYTES);
    if !clamped.contains('^') && carets.contains('^') {
        for _ in 0..MAX_ERROR_TOKEN_BYTES - 1 {
            formatter.write_char(' ')?;
        }
        return formatter.write_char('^');
    }
    for character in clamped.chars() {
        let safe = if character == '^' { '^' } else { ' ' };
        formatter.write_char(safe)?;
    }
    Ok(())
}

/// Writes every character of `slice` to `formatter` sanitized via [`string::sanitize_char`].
fn write_sanitized_slice(formatter: &mut Formatter<'_>, slice: &str) -> fmt::Result {
    for character in slice.chars() {
        formatter.write_char(string::sanitize_char(character))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;
    use std::error::Error;

    use crate::error::MAX_ERROR_TOKEN_BYTES;
    use super::{DiagnosticError, build_caret_annotation};

    #[test]
    fn caret_annotation_zero_allocation_and_extreme_bounds() {
        let carets = build_caret_annotation(1, 3, 10);
        assert_eq!(carets, "^^^");
        assert!(matches!(carets, Cow::Borrowed(_)));

        let padded = build_caret_annotation(4, 3, 10);
        assert_eq!(padded, "   ^^^");

        let carets_0 = build_caret_annotation(0, 5, 20);
        assert_eq!(carets_0, "^^^^^");
        assert!(matches!(carets_0, Cow::Borrowed(_)));

        let carets_clamped = build_caret_annotation(0, 50, 10);
        assert_eq!(carets_clamped, "^^^^^^^^^^");

        let carets_zero_span = build_caret_annotation(0, 0, 0);
        assert_eq!(carets_zero_span, "^");

        let max_len = 80;
        let extreme_col = build_caret_annotation(usize::MAX, 5, max_len);
        assert_eq!(extreme_col.len(), max_len + 1);
        assert_eq!(&extreme_col[..max_len], " ".repeat(max_len));
        assert_eq!(&extreme_col[max_len..], "^");

        let overflow_col =
            build_caret_annotation(MAX_ERROR_TOKEN_BYTES + 1, 3, MAX_ERROR_TOKEN_BYTES);
        assert_eq!(overflow_col.len(), MAX_ERROR_TOKEN_BYTES);
        assert!(overflow_col.ends_with('^'));

        let long_len = 120;
        let long_carets = build_caret_annotation(1, long_len, 200);
        assert_eq!(long_carets.len(), long_len);
        assert!(long_carets.chars().all(|character| character == '^'));
        assert!(matches!(long_carets, Cow::Owned(_)));
    }

    #[test]
    fn diagnostic_error_render_gutter_overflow_and_sanitization() {
        let diagnostic = DiagnosticError {
            annotation_carets: build_caret_annotation(19, 18, 74),
            annotation_text: Cow::Borrowed(
                "volume mount path or option is invalid (paths cannot contain ':' or '..')",
            ),
            col_number: 19,
            file_path: Cow::Borrowed("./recipe.pod"),
            help_text: Cow::Borrowed(
                "pass volume options as a separate third argument: 'volume <host_dir> <container_dir> [<options>]'.",
            ),
            line_number: 28,
            line_snippet: Cow::Borrowed(
                "    volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'",
            ),
        };

        let rendered = diagnostic.to_string();
        assert_eq!(format!("{diagnostic}"), rendered);
        assert!(rendered.contains(" --> ./recipe.pod:28:19"));
        assert!(rendered.contains(
            "   |\n28 |     volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'\n   |                   ^^^^^^^^^^^^^^^^^^ volume mount path or option is invalid"
        ));
        assert!(rendered.contains("= help: pass volume options"));
        let standard_error: &dyn Error = &diagnostic;
        assert!(standard_error.source().is_none());

        let overflow_diagnostic = DiagnosticError {
            annotation_carets: Cow::Owned(format!("{}^^^", " ".repeat(MAX_ERROR_TOKEN_BYTES + 5))),
            annotation_text: Cow::Borrowed("overflow caret"),
            col_number: MAX_ERROR_TOKEN_BYTES + 6,
            file_path: Cow::Borrowed("./test.pod"),
            help_text: Cow::Borrowed("shorten line"),
            line_number: 1,
            line_snippet: Cow::Borrowed("x"),
        };
        assert!(overflow_diagnostic.to_string().contains('^'));

        let injected =
            "bad\nINFO: Forged\rWARNING: Overwrite\x1b[31mANSI\0null\u{009b}CSI\u{202e}RLO";
        let direct_diagnostic = DiagnosticError {
            annotation_carets: Cow::Borrowed("^^^\nINFO: Forged"),
            annotation_text: Cow::Borrowed(injected),
            col_number: 1,
            file_path: Cow::Borrowed(injected),
            help_text: Cow::Borrowed(injected),
            line_number: 1,
            line_snippet: Cow::Borrowed(injected),
        };
        let diagnostic_display = direct_diagnostic.to_string();
        assert_eq!(diagnostic_display.lines().count(), 6);
        assert!(!diagnostic_display.contains('\x1b'));
        assert!(!diagnostic_display.contains('\0'));
        assert!(!diagnostic_display.contains('\u{009b}'));
        assert!(!diagnostic_display.contains('\u{202e}'));
    }
}
