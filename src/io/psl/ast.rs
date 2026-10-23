//! Untyped intermediate Abstract Syntax Tree (AST) for the PodScript Language (PSL).

use std::borrow::Cow;

use super::MAX_BLOCKS_PER_DOC;

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

    /// Returns an iterator over all nested sub-blocks directly inside this block.
    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn blocks(&self) -> impl Iterator<Item = &PslBlock<'a>> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                PslStatement::Block(block) => Some(block),
                PslStatement::Directive(_) => None,
            })
    }

    /// Returns an iterator over all directives directly inside this block.
    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn directives(&self) -> impl Iterator<Item = &PslDirective<'a>> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                PslStatement::Block(_) => None,
                PslStatement::Directive(directive) => Some(directive),
            })
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

    /// Creates a PSL document with pre-allocated capacity for root blocks,
    /// clamped to [`MAX_BLOCKS_PER_DOC`].
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            blocks: Vec::with_capacity(capacity.min(MAX_BLOCKS_PER_DOC)),
        }
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

    use crate::io::psl::MAX_BLOCKS_PER_DOC;
    use super::{PslBlock, PslDirective, PslDocument, PslStatement};

    #[test]
    fn block_iterators_filter_blocks_and_directives() {
        let directive = PslDirective::new("image", vec![Cow::Borrowed("alpine:latest")], 2);
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

        let directives: Vec<_> = parent_block.directives().collect();
        assert_eq!(directives, vec![&directive]);

        let blocks: Vec<_> = parent_block.blocks().collect();
        assert_eq!(blocks, vec![&child_block]);
    }

    #[test]
    fn document_with_capacity_clamps_extreme_bounds() {
        let empty_document = PslDocument::new();
        assert!(empty_document.blocks.is_empty());
        assert_eq!(empty_document.blocks.capacity(), 0);

        let bounded_document = PslDocument::with_capacity(16);
        assert!(bounded_document.blocks.capacity() >= 16);
        assert!(bounded_document.blocks.capacity() <= MAX_BLOCKS_PER_DOC);

        let oversized_document = PslDocument::with_capacity(MAX_BLOCKS_PER_DOC + 100);
        assert_eq!(oversized_document.blocks.capacity(), MAX_BLOCKS_PER_DOC);

        let max_usize_document = PslDocument::with_capacity(usize::MAX);
        assert_eq!(max_usize_document.blocks.capacity(), MAX_BLOCKS_PER_DOC);
    }
}
