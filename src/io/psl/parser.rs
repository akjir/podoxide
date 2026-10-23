//! Single-pass O(N) lexical scanner and structural parser for PSL documents.

use std::borrow::Cow;

use crate::error::{PslError, PslErrorKind};
use super::ast::{PslBlock, PslDirective, PslDocument, PslStatement};
use super::{
    MAX_ARGS_PER_STMT, MAX_BLOCKS_PER_DOC, MAX_FILE_BYTES, MAX_NESTING_DEPTH, MAX_STMTS_PER_BLOCK,
};

const INITIAL_ARGS_CAPACITY: usize = 4;
const INITIAL_BLOCKS_CAPACITY: usize = 8;
const INITIAL_ESCAPE_EXTRA_CAPACITY: usize = 16;
const INITIAL_STMTS_CAPACITY: usize = 8;

struct Parser<'a> {
    bytes: &'a [u8],
    cursor: usize,
    input: &'a str,
    line: usize,
    open_blocks: Vec<PslBlock<'a>>,
    root_blocks: Vec<PslBlock<'a>>,
    total_blocks: usize,
}

impl<'a> Parser<'a> {
    #[must_use]
    fn new(input: &'a str) -> Self {
        Self {
            bytes: input.as_bytes(),
            cursor: 0,
            input,
            line: 1,
            open_blocks: Vec::with_capacity(MAX_NESTING_DEPTH),
            root_blocks: Vec::with_capacity(INITIAL_BLOCKS_CAPACITY),
            total_blocks: 0,
        }
    }

    fn advance_char(&mut self) -> Option<char> {
        let character = self.peek_char()?;
        self.cursor += character.len_utf8();
        Some(character)
    }

    fn advance_newline(&mut self) -> Result<bool, PslError> {
        match self.peek_byte() {
            Some(b'\n') => {
                self.cursor += 1;
                self.line += 1;
                Ok(true)
            }
            Some(b'\r') => {
                if self.bytes.get(self.cursor + 1) == Some(&b'\n') {
                    self.cursor += 2;
                    self.line += 1;
                    Ok(true)
                } else {
                    Err(PslError::new(PslErrorKind::UnexpectedToken, self.line))
                }
            }
            _ => Ok(false),
        }
    }

    fn append_escape_sequence(
        &mut self,
        content_start: usize,
        segment_start: &mut usize,
        unescaped_buffer: &mut Option<String>,
        statement_line: usize,
    ) -> Result<(), PslError> {
        let backslash_pos = self.cursor;
        self.cursor += 1;
        let unescaped = self.parse_escape_sequence(statement_line)?;
        let prefix = self
            .input
            .get(*segment_start..backslash_pos)
            .ok_or_else(|| PslError::new(PslErrorKind::UnexpectedToken, statement_line))?;
        let buffer = unescaped_buffer.get_or_insert_with(|| {
            let capacity =
                (backslash_pos - content_start).saturating_add(INITIAL_ESCAPE_EXTRA_CAPACITY);
            String::with_capacity(capacity)
        });
        buffer.push_str(prefix);
        buffer.push(unescaped);
        *segment_start = self.cursor;
        Ok(())
    }

    fn close_block(&mut self, statement_line: usize) -> Result<(), PslError> {
        let Some(block) = self.open_blocks.pop() else {
            return Err(PslError::new(
                PslErrorKind::UnexpectedClosingBrace,
                statement_line,
            ));
        };
        self.cursor += 1;
        self.expect_end_of_line(statement_line)?;
        self.push_block(block);
        Ok(())
    }

    fn expect_end_of_line(&mut self, statement_line: usize) -> Result<(), PslError> {
        self.skip_horizontal_whitespace()?;
        if self.peek_byte() == Some(b'#') {
            self.skip_comment()?;
        }
        if self.cursor >= self.bytes.len() || self.advance_newline()? {
            Ok(())
        } else {
            Err(PslError::new(PslErrorKind::UnexpectedToken, statement_line))
        }
    }

    fn finish_double_quoted(
        &mut self,
        content_start: usize,
        segment_start: usize,
        unescaped_buffer: Option<String>,
        statement_line: usize,
    ) -> Result<Cow<'a, str>, PslError> {
        let content_end = self.cursor;
        self.cursor += 1;
        self.validate_post_quote(statement_line)?;
        if let Some(mut buffer) = unescaped_buffer {
            let tail = self
                .input
                .get(segment_start..content_end)
                .ok_or_else(|| PslError::new(PslErrorKind::UnexpectedToken, statement_line))?;
            buffer.push_str(tail);
            return Ok(Cow::Owned(buffer));
        }
        let slice = self
            .input
            .get(content_start..content_end)
            .ok_or_else(|| PslError::new(PslErrorKind::UnexpectedToken, statement_line))?;
        Ok(Cow::Borrowed(slice))
    }

    fn is_statement_end(&mut self, statement_line: usize) -> Result<bool, PslError> {
        self.skip_horizontal_whitespace()?;
        match self.peek_byte() {
            None => Ok(true),
            Some(b'#') => {
                self.skip_comment()?;
                let _ = self.advance_newline()?;
                Ok(true)
            }
            Some(b'\n' | b'\r') => {
                let _ = self.advance_newline()?;
                Ok(true)
            }
            Some(b'}') => Err(PslError::new(PslErrorKind::UnexpectedToken, statement_line)),
            _ => Ok(false),
        }
    }

    fn open_block(
        &mut self,
        name: &'a str,
        arguments: Vec<Cow<'a, str>>,
        statement_line: usize,
    ) -> Result<(), PslError> {
        if self.open_blocks.len() >= MAX_NESTING_DEPTH || self.total_blocks >= MAX_BLOCKS_PER_DOC {
            return Err(PslError::new(PslErrorKind::LimitExceeded, statement_line));
        }

        self.skip_horizontal_whitespace()?;
        let is_inline_empty = if self.peek_byte() == Some(b'}') {
            self.cursor += 1;
            true
        } else {
            false
        };
        self.expect_end_of_line(statement_line)?;

        self.total_blocks += 1;
        if is_inline_empty {
            let block = PslBlock::new(name, arguments, Vec::new(), statement_line);
            self.push_block(block);
        } else {
            let statements = Vec::with_capacity(INITIAL_STMTS_CAPACITY);
            let block = PslBlock::new(name, arguments, statements, statement_line);
            self.open_blocks.push(block);
        }
        Ok(())
    }

    fn parse(mut self) -> Result<PslDocument<'a>, PslError> {
        if self.input.len() > MAX_FILE_BYTES {
            return Err(PslError::new(PslErrorKind::LimitExceeded, 1));
        }

        loop {
            self.skip_horizontal_whitespace()?;
            if self.peek_byte() == Some(b'#') {
                self.skip_comment()?;
            }
            if self.advance_newline()? {
                continue;
            }
            if self.cursor >= self.bytes.len() {
                break;
            }

            let statement_line = self.line;
            if self.peek_byte() == Some(b'}') {
                self.close_block(statement_line)?;
            } else {
                self.parse_statement(statement_line)?;
            }
        }

        if !self.open_blocks.is_empty() {
            return Err(PslError::new(PslErrorKind::UnclosedBlock, self.line));
        }

        Ok(PslDocument {
            blocks: self.root_blocks,
        })
    }

    fn parse_argument(&mut self, statement_line: usize) -> Result<Cow<'a, str>, PslError> {
        match self.peek_byte() {
            Some(b'"') => self.parse_double_quoted(statement_line),
            Some(b'\'') => self.parse_single_quoted(statement_line),
            _ => self.parse_bare_word(statement_line),
        }
    }

    fn parse_bare_word(&mut self, statement_line: usize) -> Result<Cow<'a, str>, PslError> {
        let start = self.cursor;
        while let Some(character) = self.peek_char() {
            if !self.step_bare_word_char(character, statement_line)? {
                break;
            }
        }

        let slice = self
            .input
            .get(start..self.cursor)
            .ok_or_else(|| PslError::new(PslErrorKind::UnexpectedToken, statement_line))?;
        Ok(Cow::Borrowed(slice))
    }

    fn parse_double_quoted(&mut self, statement_line: usize) -> Result<Cow<'a, str>, PslError> {
        self.cursor += 1;
        let content_start = self.cursor;
        let mut segment_start = content_start;
        let mut unescaped_buffer: Option<String> = None;

        while let Some(character) = self.peek_char() {
            if character == '"' {
                return self.finish_double_quoted(
                    content_start,
                    segment_start,
                    unescaped_buffer,
                    statement_line,
                );
            }
            if character == '\\' {
                self.append_escape_sequence(
                    content_start,
                    &mut segment_start,
                    &mut unescaped_buffer,
                    statement_line,
                )?;
                continue;
            }
            if character == '\r' || character == '\n' {
                return Err(PslError::new(
                    PslErrorKind::UnterminatedString,
                    statement_line,
                ));
            }
            self.cursor += character.len_utf8();
        }

        Err(PslError::new(
            PslErrorKind::UnterminatedString,
            statement_line,
        ))
    }

    fn parse_escape_sequence(&mut self, statement_line: usize) -> Result<char, PslError> {
        let Some(byte) = self.peek_byte() else {
            return Err(PslError::new(
                PslErrorKind::UnterminatedString,
                statement_line,
            ));
        };
        if byte == b'\r' || byte == b'\n' {
            return Err(PslError::new(
                PslErrorKind::UnterminatedString,
                statement_line,
            ));
        }
        let Some(unescaped) = unescape_byte(byte) else {
            return Err(PslError::new(
                PslErrorKind::InvalidEscapeSequence,
                statement_line,
            ));
        };
        self.cursor += 1;
        Ok(unescaped)
    }

    fn parse_identifier(&mut self, statement_line: usize) -> Result<&'a str, PslError> {
        if !self.peek_byte().is_some_and(is_identifier_start) {
            return Err(PslError::new(
                PslErrorKind::InvalidIdentifier,
                statement_line,
            ));
        }

        let start = self.cursor;
        self.cursor += 1;
        while self.peek_byte().is_some_and(is_identifier_continue) {
            self.cursor += 1;
        }

        self.validate_post_identifier(statement_line)?;
        self.input
            .get(start..self.cursor)
            .ok_or_else(|| PslError::new(PslErrorKind::InvalidIdentifier, statement_line))
    }

    fn parse_single_quoted(&mut self, statement_line: usize) -> Result<Cow<'a, str>, PslError> {
        self.cursor += 1;
        let content_start = self.cursor;

        while let Some(character) = self.advance_char() {
            if character == '\'' {
                let content_end = self.cursor - 1;
                self.validate_post_quote(statement_line)?;
                let slice = self
                    .input
                    .get(content_start..content_end)
                    .ok_or_else(|| PslError::new(PslErrorKind::UnexpectedToken, statement_line))?;
                return Ok(Cow::Borrowed(slice));
            }
            if character == '\r' || character == '\n' {
                return Err(PslError::new(
                    PslErrorKind::UnterminatedString,
                    statement_line,
                ));
            }
        }

        Err(PslError::new(
            PslErrorKind::UnterminatedString,
            statement_line,
        ))
    }

    fn parse_statement(&mut self, statement_line: usize) -> Result<(), PslError> {
        if self
            .open_blocks
            .last()
            .is_some_and(|parent| parent.statements.len() >= MAX_STMTS_PER_BLOCK)
        {
            return Err(PslError::new(PslErrorKind::LimitExceeded, statement_line));
        }

        let name = self.parse_identifier(statement_line)?;
        let mut arguments = Vec::with_capacity(INITIAL_ARGS_CAPACITY);

        while !self.is_statement_end(statement_line)? {
            if self.peek_byte() == Some(b'{') {
                self.cursor += 1;
                return self.open_block(name, arguments, statement_line);
            }
            if arguments.len() >= MAX_ARGS_PER_STMT {
                return Err(PslError::new(PslErrorKind::LimitExceeded, statement_line));
            }
            let argument = self.parse_argument(statement_line)?;
            arguments.push(argument);
        }

        self.push_directive(name, arguments, statement_line)
    }

    #[inline]
    fn peek_byte(&self) -> Option<u8> {
        self.bytes.get(self.cursor).copied()
    }

    fn peek_char(&self) -> Option<char> {
        self.input.get(self.cursor..)?.chars().next()
    }

    fn push_block(&mut self, block: PslBlock<'a>) {
        if let Some(parent) = self.open_blocks.last_mut() {
            parent.statements.push(PslStatement::Block(block));
        } else {
            self.root_blocks.push(block);
        }
    }

    fn push_directive(
        &mut self,
        name: &'a str,
        arguments: Vec<Cow<'a, str>>,
        statement_line: usize,
    ) -> Result<(), PslError> {
        if arguments.is_empty() {
            return Err(PslError::new(
                PslErrorKind::MissingDirectiveArgument,
                statement_line,
            ));
        }
        let Some(parent) = self.open_blocks.last_mut() else {
            return Err(PslError::new(
                PslErrorKind::UnexpectedRootDirective,
                statement_line,
            ));
        };
        let directive = PslDirective::new(name, arguments, statement_line);
        parent.statements.push(PslStatement::Directive(directive));
        Ok(())
    }

    fn skip_comment(&mut self) -> Result<(), PslError> {
        while let Some(byte) = self.peek_byte() {
            if byte == b'\n' {
                break;
            }
            if byte == b'\r' {
                if self.bytes.get(self.cursor + 1) == Some(&b'\n') {
                    break;
                }
                return Err(PslError::new(PslErrorKind::UnexpectedToken, self.line));
            }
            self.cursor += 1;
        }
        Ok(())
    }

    fn skip_horizontal_whitespace(&mut self) -> Result<(), PslError> {
        while self.peek_byte().is_some_and(is_horizontal_whitespace) {
            self.cursor += 1;
        }
        if !matches!(self.peek_byte(), None | Some(b'\n' | b'\r'))
            && let Some(character) = self.peek_char()
            && (character.is_whitespace() || character.is_control())
        {
            return Err(PslError::new(PslErrorKind::UnexpectedToken, self.line));
        }
        Ok(())
    }

    fn step_bare_word_char(
        &mut self,
        character: char,
        statement_line: usize,
    ) -> Result<bool, PslError> {
        match character {
            ' ' | '\t' | '\n' | '#' => Ok(false),
            '\r' => {
                if self.bytes.get(self.cursor + 1) == Some(&b'\n') {
                    Ok(false)
                } else {
                    Err(PslError::new(PslErrorKind::UnexpectedToken, statement_line))
                }
            }
            '{' => Err(PslError::new(
                PslErrorKind::InvalidBlockHeader,
                statement_line,
            )),
            '}' | '"' | '\'' => Err(PslError::new(PslErrorKind::UnexpectedToken, statement_line)),
            _ if character.is_whitespace() || character.is_control() => {
                Err(PslError::new(PslErrorKind::UnexpectedToken, statement_line))
            }
            _ => {
                self.cursor += character.len_utf8();
                Ok(true)
            }
        }
    }

    fn validate_post_identifier(&self, statement_line: usize) -> Result<(), PslError> {
        match self.peek_byte() {
            None | Some(b' ' | b'\t' | b'\n' | b'#') => Ok(()),
            Some(b'\r') => {
                if self.bytes.get(self.cursor + 1) == Some(&b'\n') {
                    Ok(())
                } else {
                    Err(PslError::new(PslErrorKind::UnexpectedToken, statement_line))
                }
            }
            Some(b'{') => Err(PslError::new(
                PslErrorKind::InvalidBlockHeader,
                statement_line,
            )),
            Some(b'}' | b'"' | b'\'') => {
                Err(PslError::new(PslErrorKind::UnexpectedToken, statement_line))
            }
            Some(_) => {
                if let Some(character) = self.peek_char()
                    && (character.is_whitespace() || character.is_control())
                {
                    return Err(PslError::new(PslErrorKind::UnexpectedToken, statement_line));
                }
                Err(PslError::new(
                    PslErrorKind::InvalidIdentifier,
                    statement_line,
                ))
            }
        }
    }

    fn validate_post_quote(&self, statement_line: usize) -> Result<(), PslError> {
        match self.peek_byte() {
            None => Ok(()),
            Some(b'\r') => {
                if self.bytes.get(self.cursor + 1) == Some(&b'\n') {
                    Ok(())
                } else {
                    Err(PslError::new(PslErrorKind::UnexpectedToken, statement_line))
                }
            }
            Some(b'{') => Err(PslError::new(
                PslErrorKind::InvalidBlockHeader,
                statement_line,
            )),
            Some(byte) if is_post_quote_delimiter(byte) => Ok(()),
            Some(_) => Err(PslError::new(PslErrorKind::UnexpectedToken, statement_line)),
        }
    }
}

/// Parses a PSL input slice into a [`PslDocument`] in a single O(N) pass.
///
/// # Errors
///
/// Returns [`PslError`] if the input violates PSL lexical, grammatical, or resource bound rules.
pub fn parse_psl<'a>(input: &'a str) -> Result<PslDocument<'a>, PslError> {
    Parser::new(input).parse()
}

#[inline]
const fn is_horizontal_whitespace(byte: u8) -> bool {
    byte == b' ' || byte == b'\t'
}

#[inline]
const fn is_identifier_continue(byte: u8) -> bool {
    is_identifier_start(byte) || byte.is_ascii_digit() || byte == b'-'
}

#[inline]
const fn is_identifier_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

#[inline]
const fn is_post_quote_delimiter(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'#')
}

#[inline]
const fn unescape_byte(byte: u8) -> Option<char> {
    match byte {
        b'"' => Some('"'),
        b'\\' => Some('\\'),
        b'n' => Some('\n'),
        b'r' => Some('\r'),
        b't' => Some('\t'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use crate::error::{PslError, PslErrorKind};
    use crate::io::psl::{
        MAX_ARGS_PER_STMT, MAX_BLOCKS_PER_DOC, MAX_FILE_BYTES, MAX_NESTING_DEPTH,
        MAX_STMTS_PER_BLOCK, PslDirective, PslDocument, PslStatement,
    };
    use super::parse_psl;

    const CANONICAL_EXAMPLE_POD: &str = "\
# Example Web Stack Recipe

pod example-stack {
    name        \"Example Web Stack\"
    description \"A minimal but complete example recipe with a web app and database.\"
    path        /pods/example-stack
    registry    docker.io
    publish     8080 80 TCP
    option      --userns=keep-id

    command migrate {
        description \"Run database migrations.\"
        container   *db
        user        dbuser
        execute     \"db-cli -U dbuser -d app -f /migrations/run.sql\"
    }
}

container *db {
    detach   true
    restart  always
    registry docker.io
    image    example/db-server:latest
    volume   data /var/lib/db/data:Z # Mounts 'data' with SELinux label 'Z'
    option   \"--env DB_PASSWORD=secret\"
}

container *app {
    detach  true
    restart always
    image   example/example-app:latest
    command start-server.sh
}
";

    const CANONICAL_PODS_CONF: &str = "\
# Podoxide Configuration (PSL)

general {
    editor   vim
    simulate false
}

pods {
    path /pods
}

recipes {
    path ./recipes

    groups {
        all      missing_recipe
        stack    @database @web
        database recipe_001_empty
        web      frontend
    }
}
";

    #[test]
    fn parse_canonical_pods_conf() {
        let document = parse_psl(CANONICAL_PODS_CONF).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(document.blocks.len(), 3);

        let general = &document.blocks[0];
        assert_eq!(general.name, "general");
        assert!(general.arguments.is_empty());
        assert_eq!(general.line, 3);
        let general_directives: Vec<_> = general.directives().collect();
        assert_eq!(general_directives.len(), 2);
        assert_eq!(general_directives[0].name, "editor");
        assert_eq!(general_directives[0].arguments, vec![Cow::Borrowed("vim")]);
        assert_eq!(general_directives[0].line, 4);
        assert_eq!(general_directives[1].name, "simulate");
        assert_eq!(
            general_directives[1].arguments,
            vec![Cow::Borrowed("false")]
        );
        assert_eq!(general_directives[1].line, 5);

        let pods = &document.blocks[1];
        assert_eq!(pods.name, "pods");
        assert_eq!(pods.line, 8);
        let pods_directives: Vec<_> = pods.directives().collect();
        assert_eq!(pods_directives.len(), 1);
        assert_eq!(pods_directives[0].name, "path");
        assert_eq!(pods_directives[0].arguments, vec![Cow::Borrowed("/pods")]);
        assert_eq!(pods_directives[0].line, 9);

        let recipes = &document.blocks[2];
        assert_eq!(recipes.name, "recipes");
        assert_eq!(recipes.line, 12);
        assert_eq!(recipes.statements.len(), 2);
        let recipes_directives: Vec<_> = recipes.directives().collect();
        assert_eq!(recipes_directives.len(), 1);
        assert_eq!(recipes_directives[0].name, "path");
        assert_eq!(
            recipes_directives[0].arguments,
            vec![Cow::Borrowed("./recipes")]
        );
        assert_eq!(recipes_directives[0].line, 13);

        let recipes_blocks: Vec<_> = recipes.blocks().collect();
        assert_eq!(recipes_blocks.len(), 1);
        let groups = recipes_blocks[0];
        assert_eq!(groups.name, "groups");
        assert_eq!(groups.line, 15);
        let group_directives: Vec<_> = groups.directives().collect();
        assert_eq!(group_directives.len(), 4);
        assert_eq!(group_directives[1].name, "stack");
        assert_eq!(
            group_directives[1].arguments,
            vec![Cow::Borrowed("@database"), Cow::Borrowed("@web")]
        );
        assert_eq!(group_directives[1].line, 17);
    }

    #[test]
    fn parse_canonical_example_pod() {
        let document = parse_psl(CANONICAL_EXAMPLE_POD).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(document.blocks.len(), 3);

        let pod_block = &document.blocks[0];
        assert_eq!(pod_block.name, "pod");
        assert_eq!(pod_block.arguments, vec![Cow::Borrowed("example-stack")]);
        assert_eq!(pod_block.line, 3);
        assert_eq!(pod_block.statements.len(), 7);

        let pod_directives: Vec<_> = pod_block.directives().collect();
        assert_eq!(pod_directives.len(), 6);
        assert!(matches!(pod_directives[0].arguments[0], Cow::Borrowed(_)));
        assert_eq!(pod_directives[0].arguments[0], "Example Web Stack");
        assert_eq!(
            pod_directives[4].arguments,
            vec![
                Cow::Borrowed("8080"),
                Cow::Borrowed("80"),
                Cow::Borrowed("TCP"),
            ]
        );

        let nested_blocks: Vec<_> = pod_block.blocks().collect();
        assert_eq!(nested_blocks.len(), 1);
        let migrate = nested_blocks[0];
        assert_eq!(migrate.name, "command");
        assert_eq!(migrate.arguments, vec![Cow::Borrowed("migrate")]);
        assert_eq!(migrate.line, 11);
        assert_eq!(migrate.directives().count(), 4);

        let db_container = &document.blocks[1];
        assert_eq!(db_container.name, "container");
        assert_eq!(db_container.arguments, vec![Cow::Borrowed("*db")]);
        assert_eq!(db_container.line, 19);
        let db_directives: Vec<_> = db_container.directives().collect();
        assert_eq!(db_directives.len(), 6);
        assert_eq!(db_directives[4].name, "volume");
        assert_eq!(
            db_directives[4].arguments,
            vec![Cow::Borrowed("data"), Cow::Borrowed("/var/lib/db/data:Z")]
        );
        assert_eq!(db_directives[4].line, 24);

        let app_container = &document.blocks[2];
        assert_eq!(app_container.name, "container");
        assert_eq!(app_container.arguments, vec![Cow::Borrowed("*app")]);
        assert_eq!(app_container.line, 28);
        assert_eq!(app_container.directives().count(), 4);
    }

    #[test]
    fn parse_empty_comments_crlf_and_inline_blocks() {
        assert_eq!(parse_psl(""), Ok(PslDocument::new()));
        assert_eq!(
            parse_psl("   \t  \n# comment only\r\n  # another comment"),
            Ok(PslDocument::with_capacity(0))
        );

        let input = "block_1 {}\r\nblock-2 arg1 { #inline-comment\r\n\tkey val#comment\r\n}\r\nempty_spaced {   } # trailing";
        let document = parse_psl(input).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(document.blocks.len(), 3);
        assert_eq!(document.blocks[0].name, "block_1");
        assert!(document.blocks[0].statements.is_empty());
        assert_eq!(document.blocks[0].line, 1);

        assert_eq!(document.blocks[1].name, "block-2");
        assert_eq!(document.blocks[1].arguments, vec![Cow::Borrowed("arg1")]);
        assert_eq!(document.blocks[1].line, 2);
        assert_eq!(
            document.blocks[1].statements,
            vec![PslStatement::Directive(PslDirective::new(
                "key",
                vec![Cow::Borrowed("val")],
                3,
            ))]
        );

        assert_eq!(document.blocks[2].name, "empty_spaced");
        assert_eq!(document.blocks[2].line, 5);
    }

    #[test]
    fn parse_quoted_strings_cow_borrowed_vs_owned() {
        let input = "\
block {
    plain_double \"no escapes here\"
    escaped_double \"line1\\nline2\\t\\\"quoted\\\"\\\\\\r\"
    single_raw 'raw \\x \\n \\0 \"literal\"'
}";
        let document = parse_psl(input).unwrap_or_else(|error| panic!("{error}"));
        let directives: Vec<_> = document.blocks[0].directives().collect();
        assert_eq!(directives.len(), 3);

        assert!(matches!(directives[0].arguments[0], Cow::Borrowed(_)));
        assert_eq!(directives[0].arguments[0], "no escapes here");

        assert!(matches!(directives[1].arguments[0], Cow::Owned(_)));
        assert_eq!(directives[1].arguments[0], "line1\nline2\t\"quoted\"\\\r");

        assert!(matches!(directives[2].arguments[0], Cow::Borrowed(_)));
        assert_eq!(directives[2].arguments[0], "raw \\x \\n \\0 \"literal\"");
    }

    #[test]
    fn parse_utf8_and_root_templates() {
        let utf8_input = "\
# UTF-8 comment: äöü ß
pod_utf8 arg_äöü {
    label \"Grüße \\\"Welt\\\" αβγ\"
    raw_label 'Grüße Welt αβγ'
}
";
        let document = parse_psl(utf8_input).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(document.blocks.len(), 1);
        assert_eq!(document.blocks[0].arguments, vec![Cow::Borrowed("arg_äöü")]);
        let directives: Vec<_> = document.blocks[0].directives().collect();
        assert_eq!(directives[0].arguments[0], "Grüße \"Welt\" αβγ");
        assert_eq!(directives[1].arguments[0], "Grüße Welt αβγ");

        let root_conf = include_str!("../../../pods.conf");
        let conf_doc = parse_psl(root_conf).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(conf_doc.blocks.len(), 3);

        let root_recipe = include_str!("../../../recipe.pod");
        let recipe_doc = parse_psl(root_recipe).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(recipe_doc.blocks.len(), 3);
    }

    #[test]
    fn reject_unexpected_root_directive() {
        let error = parse_psl("editor vim\n").unwrap_err();
        assert_eq!(
            error,
            PslError::new(PslErrorKind::UnexpectedRootDirective, 1)
        );
    }

    #[test]
    fn reject_missing_directive_argument() {
        assert_eq!(
            parse_psl("general\n"),
            Err(PslError::new(PslErrorKind::MissingDirectiveArgument, 1))
        );
        assert_eq!(
            parse_psl("general {\n    editor\n}\n"),
            Err(PslError::new(PslErrorKind::MissingDirectiveArgument, 2))
        );
    }

    #[test]
    fn reject_invalid_identifiers() {
        assert_eq!(
            parse_psl("\"general\" {}\n"),
            Err(PslError::new(PslErrorKind::InvalidIdentifier, 1))
        );
        assert_eq!(
            parse_psl("'general' {}\n"),
            Err(PslError::new(PslErrorKind::InvalidIdentifier, 1))
        );
        assert_eq!(
            parse_psl("123pod {}\n"),
            Err(PslError::new(PslErrorKind::InvalidIdentifier, 1))
        );
        assert_eq!(
            parse_psl("general {\n    -opt val\n}\n"),
            Err(PslError::new(PslErrorKind::InvalidIdentifier, 2))
        );
        assert_eq!(
            parse_psl("general {\n    bad.name val\n}\n"),
            Err(PslError::new(PslErrorKind::InvalidIdentifier, 2))
        );
    }

    #[test]
    fn reject_invalid_block_headers() {
        assert_eq!(
            parse_psl("general{\n}\n"),
            Err(PslError::new(PslErrorKind::InvalidBlockHeader, 1))
        );
        assert_eq!(
            parse_psl("pod app{\n}\n"),
            Err(PslError::new(PslErrorKind::InvalidBlockHeader, 1))
        );
        assert_eq!(
            parse_psl("pod \"app\"{\n}\n"),
            Err(PslError::new(PslErrorKind::InvalidBlockHeader, 1))
        );
        assert_eq!(
            parse_psl("pod 'app'{\n}\n"),
            Err(PslError::new(PslErrorKind::InvalidBlockHeader, 1))
        );
    }

    #[test]
    fn reject_unexpected_tokens_and_delimiter_injections() {
        // Trailing token after '{'
        assert_eq!(
            parse_psl("general { editor vim\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 1))
        );
        // Trailing token after '}'
        assert_eq!(
            parse_psl("general {\n    editor vim\n} trailing\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 3))
        );
        // Trailing token after inline '{}'
        assert_eq!(
            parse_psl("general {} trailing\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 1))
        );
        // Adjacent quotes on bare word and after closing quotes
        assert_eq!(
            parse_psl("general {\n    key foo\"bar\"\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 2))
        );
        assert_eq!(
            parse_psl("general {\n    key \"foo\"bar\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 2))
        );
        assert_eq!(
            parse_psl("general {\n    key 'foo'bar\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 2))
        );
        // Standalone CR outside quotes (whitespace, identifier, bare word, post-quote, comment)
        assert_eq!(
            parse_psl("general {\r    key val\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 1))
        );
        assert_eq!(
            parse_psl("general\r {\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 1))
        );
        assert_eq!(
            parse_psl("general {\n    key val\r}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 2))
        );
        assert_eq!(
            parse_psl("general {\n    key \"val\"\r}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 2))
        );
        assert_eq!(
            parse_psl("# comment\r\ngeneral {}\n# bad\rcomment\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 3))
        );
        // Unicode non-breaking space and control chars outside quotes
        assert_eq!(
            parse_psl("general\u{00A0}{\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 1))
        );
        assert_eq!(
            parse_psl("\u{00A0}general {\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 1))
        );
        assert_eq!(
            parse_psl("general {\n    key val\x07\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 2))
        );
        // Closing brace on directive line or attached to identifier
        assert_eq!(
            parse_psl("general {\n    key val }\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 2))
        );
        assert_eq!(
            parse_psl("general {\n    key}\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedToken, 2))
        );
    }

    #[test]
    fn reject_unclosed_block_and_unexpected_closing_brace() {
        assert_eq!(
            parse_psl("general {\n    editor vim\n"),
            Err(PslError::new(PslErrorKind::UnclosedBlock, 3))
        );
        assert_eq!(
            parse_psl("}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedClosingBrace, 1))
        );
        assert_eq!(
            parse_psl("general {}\n}\n"),
            Err(PslError::new(PslErrorKind::UnexpectedClosingBrace, 2))
        );
    }

    #[test]
    fn reject_unterminated_strings() {
        assert_eq!(
            parse_psl("general {\n    key \"unclosed\n}\n"),
            Err(PslError::new(PslErrorKind::UnterminatedString, 2))
        );
        assert_eq!(
            parse_psl("general {\n    key 'unclosed\n}\n"),
            Err(PslError::new(PslErrorKind::UnterminatedString, 2))
        );
        assert_eq!(
            parse_psl("general {\n    key \"eof"),
            Err(PslError::new(PslErrorKind::UnterminatedString, 2))
        );
        assert_eq!(
            parse_psl("general {\n    key 'eof"),
            Err(PslError::new(PslErrorKind::UnterminatedString, 2))
        );
        assert_eq!(
            parse_psl("general {\n    key \"trailing\\\n\"\n}\n"),
            Err(PslError::new(PslErrorKind::UnterminatedString, 2))
        );
    }

    #[test]
    fn reject_invalid_escape_sequences() {
        for invalid in ["\"\\x\"", "\"\\a\"", "\"\\0\"", "\"\\'\""] {
            let input = format!("general {{\n    key {invalid}\n}}\n");
            assert_eq!(
                parse_psl(&input),
                Err(PslError::new(PslErrorKind::InvalidEscapeSequence, 2))
            );
        }
    }

    #[test]
    fn enforce_all_resource_limits() {
        // 1. MAX_FILE_BYTES + 1
        let oversized = " ".repeat(MAX_FILE_BYTES + 1);
        assert_eq!(
            parse_psl(&oversized),
            Err(PslError::new(PslErrorKind::LimitExceeded, 1))
        );

        // 2. MAX_NESTING_DEPTH (8 succeeds, 9 fails)
        let valid_depth = "b {\n".repeat(MAX_NESTING_DEPTH) + &"}\n".repeat(MAX_NESTING_DEPTH);
        assert!(parse_psl(&valid_depth).is_ok());

        let excessive_depth =
            "b {\n".repeat(MAX_NESTING_DEPTH + 1) + &"}\n".repeat(MAX_NESTING_DEPTH + 1);
        assert_eq!(
            parse_psl(&excessive_depth),
            Err(PslError::new(
                PslErrorKind::LimitExceeded,
                MAX_NESTING_DEPTH + 1
            ))
        );

        // 3. MAX_BLOCKS_PER_DOC (256 succeeds, 257 fails)
        let valid_blocks = "b {}\n".repeat(MAX_BLOCKS_PER_DOC);
        assert!(parse_psl(&valid_blocks).is_ok());

        let excessive_blocks = "b {}\n".repeat(MAX_BLOCKS_PER_DOC + 1);
        assert_eq!(
            parse_psl(&excessive_blocks),
            Err(PslError::new(
                PslErrorKind::LimitExceeded,
                MAX_BLOCKS_PER_DOC + 1
            ))
        );

        // 4. MAX_STMTS_PER_BLOCK (512 succeeds, 513 fails before scanning 513th statement)
        let valid_stmts = format!("b {{\n{}}}\n", "k v\n".repeat(MAX_STMTS_PER_BLOCK));
        assert!(parse_psl(&valid_stmts).is_ok());

        let excessive_stmts = format!(
            "b {{\n{}123invalid \"unterminated\n}}\n",
            "k v\n".repeat(MAX_STMTS_PER_BLOCK)
        );
        assert_eq!(
            parse_psl(&excessive_stmts),
            Err(PslError::new(
                PslErrorKind::LimitExceeded,
                MAX_STMTS_PER_BLOCK + 2
            ))
        );

        // 5. MAX_ARGS_PER_STMT (64 succeeds, 65 fails before scanning 65th argument)
        let valid_args = format!("b {{\n    k{}\n}}\n", " a".repeat(MAX_ARGS_PER_STMT));
        assert!(parse_psl(&valid_args).is_ok());

        let excessive_args = format!(
            "b {{\n    k{} \"unterminated\n}}\n",
            " a".repeat(MAX_ARGS_PER_STMT)
        );
        assert_eq!(
            parse_psl(&excessive_args),
            Err(PslError::new(PslErrorKind::LimitExceeded, 2))
        );
    }
}
