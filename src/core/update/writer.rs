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

//! Line- and carriage-return-aware indented writer adapter for nested pod output.

use std::io::{self, Write};

/// Writer adapter that prepends a fixed indentation prefix to every non-empty line and carriage-return rewrite.
pub struct IndentedWriter<'a, W: Write> {
    at_line_start: bool,
    indent: &'a str,
    inner: &'a mut W,
}

impl<'a, W: Write> IndentedWriter<'a, W> {
    /// Creates an [`IndentedWriter`] that prefixes non-empty lines written to `inner` with `indent`.
    #[must_use]
    pub const fn new(inner: &'a mut W, indent: &'a str) -> Self {
        Self {
            at_line_start: true,
            indent,
            inner,
        }
    }
}

impl<W: Write> Write for IndentedWriter<'_, W> {
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }

    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        for segment in buf.split_inclusive(|&byte| byte == b'\n' || byte == b'\r') {
            if self.at_line_start && segment != b"\n" && segment != b"\r" {
                self.inner.write_all(self.indent.as_bytes())?;
            }
            self.inner.write_all(segment)?;
            self.at_line_start = segment.ends_with(b"\n") || segment.ends_with(b"\r");
        }
        Ok(buf.len())
    }
}
