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

//! Single-pass O(N) structural parser for PSL documents.

use std::borrow::Cow;

use crate::error::{PslError, PslErrorKind};
use super::ast::{PslBlock, PslDirective, PslDocument, PslStatement};
use super::scanner::Scanner;
use super::{
    MAX_ARGS_PER_STMT, MAX_BLOCKS_PER_DOC, MAX_FILE_BYTES, MAX_NESTING_DEPTH, MAX_STMTS_PER_BLOCK,
};

const INITIAL_ARGS_CAPACITY: usize = 4;
const INITIAL_BLOCKS_CAPACITY: usize = 8;
const INITIAL_STMTS_CAPACITY: usize = 8;

/// Open block frame stored on the parser stack with its opening `{` byte coordinates.
struct OpenBlock<'a> {
    block: PslBlock<'a>,
    line_start: usize,
    open_brace_offset: usize,
}

/// Stateful single-pass block stack and statement parser for PSL documents.
struct Parser<'a> {
    open_blocks: Vec<OpenBlock<'a>>,
    root_blocks: Vec<PslBlock<'a>>,
    scanner: Scanner<'a>,
    total_blocks: usize,
}

impl<'a> Parser<'a> {
    /// Creates a new [`Parser`] initialized at the beginning of `input`.
    #[must_use]
    fn new(input: &'a str) -> Self {
        Self {
            open_blocks: Vec::with_capacity(MAX_NESTING_DEPTH),
            root_blocks: Vec::with_capacity(INITIAL_BLOCKS_CAPACITY),
            scanner: Scanner::new(input),
            total_blocks: 0,
        }
    }

    /// Pops the innermost open block on `}`, verifies that no trailing tokens follow on the line, and attaches the block to its parent or root document.
    fn close_block(&mut self, statement_line: usize) -> Result<(), PslError> {
        let Some(open_block) = self.open_blocks.pop() else {
            return Err(self
                .scanner
                .error_here(PslErrorKind::UnexpectedClosingBrace, statement_line));
        };
        self.scanner.advance_byte();
        self.scanner.expect_end_of_line(statement_line)?;
        self.push_block(open_block.block);
        Ok(())
    }

    /// Enforces block nesting and document block bounds, handles inline empty `{}` blocks, or pushes a new open block onto the stack.
    fn open_block(
        &mut self,
        name: &'a str,
        arguments: Vec<Cow<'a, str>>,
        statement_line: usize,
        open_line_start: usize,
        open_brace_offset: usize,
    ) -> Result<(), PslError> {
        if self.open_blocks.len() >= MAX_NESTING_DEPTH || self.total_blocks >= MAX_BLOCKS_PER_DOC {
            return Err(self.scanner.error_at(
                PslErrorKind::LimitExceeded,
                statement_line,
                open_brace_offset,
            ));
        }

        self.scanner.skip_horizontal_whitespace()?;
        let is_inline_empty = if self.scanner.peek_byte() == Some(b'}') {
            self.scanner.advance_byte();
            true
        } else {
            false
        };
        self.scanner.expect_end_of_line(statement_line)?;

        self.total_blocks += 1;
        if is_inline_empty {
            let block = PslBlock::new(name, arguments, Vec::new(), statement_line);
            self.push_block(block);
        } else {
            let statements = Vec::with_capacity(INITIAL_STMTS_CAPACITY);
            let block = PslBlock::new(name, arguments, statements, statement_line);
            self.open_blocks.push(OpenBlock {
                block,
                line_start: open_line_start,
                open_brace_offset,
            });
        }
        Ok(())
    }

    /// Consumes the entire input buffer and returns the parsed [`PslDocument`].
    fn parse(mut self) -> Result<PslDocument<'a>, PslError> {
        loop {
            self.scanner.skip_horizontal_whitespace()?;
            if self.scanner.peek_byte() == Some(b'#') {
                self.scanner.skip_comment()?;
            }
            if self.scanner.advance_newline()? {
                continue;
            }
            if self.scanner.is_eof() {
                break;
            }

            let statement_line = self.scanner.line();
            if self.scanner.peek_byte() == Some(b'}') {
                self.close_block(statement_line)?;
            } else {
                self.parse_statement(statement_line)?;
            }
        }

        if let Some(unclosed) = self.open_blocks.last() {
            let column = self
                .scanner
                .column_between(unclosed.line_start, unclosed.open_brace_offset);
            return Err(PslError::with_column(
                PslErrorKind::UnclosedBlock,
                unclosed.block.line,
                column,
            ));
        }

        Ok(PslDocument {
            blocks: self.root_blocks,
        })
    }

    /// Parses a single block header or directive line starting at `statement_line`.
    fn parse_statement(&mut self, statement_line: usize) -> Result<(), PslError> {
        if self.open_blocks.is_empty() && self.total_blocks >= MAX_BLOCKS_PER_DOC {
            return Err(self
                .scanner
                .error_here(PslErrorKind::LimitExceeded, statement_line));
        }
        if self
            .open_blocks
            .last()
            .is_some_and(|parent| parent.block.statements.len() >= MAX_STMTS_PER_BLOCK)
        {
            return Err(self
                .scanner
                .error_here(PslErrorKind::LimitExceeded, statement_line));
        }

        let statement_line_start = self.scanner.line_start();
        let identifier_offset = self.scanner.cursor();
        let name = self.scanner.scan_identifier(statement_line)?;
        let mut arguments = Vec::new();

        while !self.scanner.is_statement_end(statement_line)? {
            if self.scanner.peek_byte() == Some(b'{') {
                let open_brace_offset = self.scanner.cursor();
                self.scanner.advance_byte();
                return self.open_block(
                    name,
                    arguments,
                    statement_line,
                    statement_line_start,
                    open_brace_offset,
                );
            }
            if arguments.len() >= MAX_ARGS_PER_STMT {
                return Err(self
                    .scanner
                    .error_here(PslErrorKind::LimitExceeded, statement_line));
            }
            let argument = self.scanner.scan_argument(statement_line)?;
            if arguments.is_empty() {
                arguments.reserve(INITIAL_ARGS_CAPACITY);
            }
            arguments.push(argument);
        }

        self.push_directive(
            name,
            arguments,
            statement_line,
            statement_line_start,
            identifier_offset,
        )
    }

    /// Appends a completed [`PslBlock`] to its enclosing parent block or to `root_blocks`.
    fn push_block(&mut self, block: PslBlock<'a>) {
        if let Some(parent) = self.open_blocks.last_mut() {
            parent.block.statements.push(PslStatement::Block(block));
        } else {
            self.root_blocks.push(block);
        }
    }

    /// Validates that a directive has at least one argument and is inside an open block, then appends it.
    fn push_directive(
        &mut self,
        name: &'a str,
        arguments: Vec<Cow<'a, str>>,
        statement_line: usize,
        statement_line_start: usize,
        identifier_offset: usize,
    ) -> Result<(), PslError> {
        if arguments.is_empty() {
            let column = self
                .scanner
                .column_between(statement_line_start, identifier_offset);
            return Err(PslError::with_column(
                PslErrorKind::MissingDirectiveArgument,
                statement_line,
                column,
            ));
        }
        let Some(parent) = self.open_blocks.last_mut() else {
            let column = self
                .scanner
                .column_between(statement_line_start, identifier_offset);
            return Err(PslError::with_column(
                PslErrorKind::UnexpectedRootDirective,
                statement_line,
                column,
            ));
        };
        let directive = PslDirective::new(name, arguments, statement_line);
        parent
            .block
            .statements
            .push(PslStatement::Directive(directive));
        Ok(())
    }
}

/// Parses a PSL input slice into a [`PslDocument`] in a single O(N) pass.
///
/// # Errors
///
/// Returns [`PslError`] if the input violates PSL lexical, grammatical, or resource bound rules.
pub fn parse_psl<'a>(input: &'a str) -> Result<PslDocument<'a>, PslError> {
    if input.len() > MAX_FILE_BYTES {
        return Err(PslError::new(PslErrorKind::LimitExceeded, 1));
    }
    Parser::new(input).parse()
}
