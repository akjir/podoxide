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

//! Unified PodScript Language (PSL) scanner, parser, and intermediate AST.

pub mod ast;
pub mod parser;

use crate::error::PslError;
pub use self::ast::{PslBlock, PslDirective, PslDocument, PslStatement};

/// Maximum arguments allowed on a single block header or directive.
pub const MAX_ARGS_PER_STMT: usize = 64;
/// Maximum total blocks allowed per PSL document.
pub const MAX_BLOCKS_PER_DOC: usize = 256;
/// Maximum input size in bytes (1 MiB) before rejecting input.
pub const MAX_FILE_BYTES: usize = 1_048_576;
/// Maximum `{ ... }` block nesting depth.
pub const MAX_NESTING_DEPTH: usize = 8;
/// Maximum directives and sub-blocks allowed inside a single block.
pub const MAX_STMTS_PER_BLOCK: usize = 512;

/// Parses a PSL configuration or recipe string into an untyped [`PslDocument`].
///
/// # Errors
///
/// Returns [`PslError`] if the input exceeds any resource limit defined in
/// `PSL_SPEC.md` or violates PSL lexical or grammatical rules.
pub fn parse_psl<'a>(input: &'a str) -> Result<PslDocument<'a>, PslError> {
    parser::parse_psl(input)
}
