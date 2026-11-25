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

//! Untyped intermediate Abstract Syntax Tree (AST) for the PodScript Language (PSL).

use std::borrow::Cow;

/// Named block containing arguments and nested directives or sub-blocks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PslBlock<'a> {
    pub arguments: Vec<Cow<'a, str>>,
    pub line: usize,
    pub name: &'a str,
    pub statements: Vec<PslStatement<'a>>,
}

impl<'a> PslBlock<'a> {
    /// Creates a new PSL block node.
    #[must_use]
    pub const fn new(
        name: &'a str,
        arguments: Vec<Cow<'a, str>>,
        statements: Vec<PslStatement<'a>>,
        line: usize,
    ) -> Self {
        Self {
            arguments,
            line,
            name,
            statements,
        }
    }
}

/// Single-line directive inside a block with one or more arguments.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PslDirective<'a> {
    pub arguments: Vec<Cow<'a, str>>,
    pub line: usize,
    pub name: &'a str,
}

impl<'a> PslDirective<'a> {
    /// Creates a new PSL directive node.
    #[must_use]
    pub const fn new(name: &'a str, arguments: Vec<Cow<'a, str>>, line: usize) -> Self {
        Self {
            arguments,
            line,
            name,
        }
    }
}

/// Root PSL document containing a sequence of top-level blocks.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PslDocument<'a> {
    pub blocks: Vec<PslBlock<'a>>,
}

impl<'a> PslDocument<'a> {
    /// Creates an empty PSL document.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Statement appearing inside a PSL block body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PslStatement<'a> {
    Block(PslBlock<'a>),
    Directive(PslDirective<'a>),
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{PslBlock, PslDirective, PslDocument, PslStatement};

    #[test]
    fn ast_constructors_preserve_fields_and_empty_defaults() {
        let directive = PslDirective::new("image", vec![Cow::Borrowed("alpine:latest")], 2);
        assert_eq!(directive.name, "image");
        assert_eq!(directive.arguments, vec![Cow::Borrowed("alpine:latest")]);
        assert_eq!(directive.line, 2);

        let child_block = PslBlock::new("command", vec![Cow::Borrowed("migrate")], Vec::new(), 3);
        let parent_block = PslBlock::new(
            "pod",
            vec![Cow::Borrowed("app")],
            vec![
                PslStatement::Directive(directive.clone()),
                PslStatement::Block(child_block.clone()),
            ],
            1,
        );
        assert_eq!(parent_block.name, "pod");
        assert_eq!(parent_block.line, 1);
        assert_eq!(parent_block.statements.len(), 2);
        assert_eq!(
            parent_block.statements[0],
            PslStatement::Directive(directive)
        );
        assert_eq!(parent_block.statements[1], PslStatement::Block(child_block));

        let empty_document = PslDocument::new();
        assert_eq!(empty_document, PslDocument::default());
        assert!(empty_document.blocks.is_empty());
        assert_eq!(empty_document.blocks.capacity(), 0);
    }
}
