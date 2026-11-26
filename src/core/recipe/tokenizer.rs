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

//! Quote-aware command-line tokenizer for `command.execute` directives in `.pod` recipes.

use std::mem;

use crate::error::{RecipeError, RecipeErrorKind};
use crate::io::psl::MAX_ARGS_PER_STMT;
use crate::utilities::string;

/// Initial capacity pre-allocated for parsed `execute` argv tokens.
const INITIAL_EXEC_TOKENS_CAPACITY: usize = 8;

/// Quote state while scanning a command `execute` string into argv tokens.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum QuoteState {
    Double,
    DoubleEscape,
    #[default]
    None,
    Single,
    UnquotedEscape,
}

/// Splits `input` into shell-style argv tokens, preserving single-quoted and double-quoted arguments.
#[must_use]
pub(crate) fn split_execute_tokens(input: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::with_capacity(INITIAL_EXEC_TOKENS_CAPACITY);
    let mut current = String::new();
    let mut in_token = false;
    let mut state = QuoteState::None;
    for character in input.chars() {
        if !step_execute_char(
            character,
            &mut state,
            &mut in_token,
            &mut current,
            &mut tokens,
        ) {
            return None;
        }
    }
    if state != QuoteState::None {
        return None;
    }
    if in_token {
        if tokens.len() >= MAX_ARGS_PER_STMT {
            return None;
        }
        tokens.push(current);
    }
    Some(tokens)
}

/// Validates and tokenizes a `command.execute` string, rejecting unclosed quotes, flag prefixes, or invalid program tokens.
pub(crate) fn validate_execute_tokens(
    execute: &str,
    fallback_token: &str,
    line: usize,
) -> Result<Vec<String>, RecipeError> {
    if !string::is_valid_text(execute) || execute.trim_start().starts_with('-') {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            fallback_token,
        ));
    }
    let Some(tokens) = split_execute_tokens(execute) else {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            fallback_token,
        ));
    };
    let Some(program) = tokens.first() else {
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            fallback_token,
        ));
    };
    if !string::is_valid_token(program) {
        let token = if program.is_empty() {
            fallback_token
        } else {
            program.as_str()
        };
        return Err(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            token,
        ));
    }
    Ok(tokens)
}

/// Processes an escaped `character` inside a double-quoted segment.
fn step_double_escape(character: char, state: &mut QuoteState, current: &mut String) {
    if !matches!(character, '"' | '\\' | '$' | '`') {
        current.push('\\');
    }
    current.push(character);
    *state = QuoteState::Double;
}

/// Processes `character` while inside a double-quoted segment.
fn step_double_quoted(character: char, state: &mut QuoteState, current: &mut String) {
    match character {
        '"' => *state = QuoteState::None,
        '\\' => *state = QuoteState::DoubleEscape,
        _ => current.push(character),
    }
}

/// Advances the quote-aware tokenizer by one `character`, returning `false` if token capacity is exceeded.
fn step_execute_char(
    character: char,
    state: &mut QuoteState,
    in_token: &mut bool,
    current: &mut String,
    tokens: &mut Vec<String>,
) -> bool {
    match *state {
        QuoteState::Double => step_double_quoted(character, state, current),
        QuoteState::DoubleEscape => step_double_escape(character, state, current),
        QuoteState::None => return step_unquoted(character, state, in_token, current, tokens),
        QuoteState::Single => {
            if character == '\'' {
                *state = QuoteState::None;
            } else {
                current.push(character);
            }
        }
        QuoteState::UnquotedEscape => {
            current.push(character);
            *state = QuoteState::None;
        }
    }
    true
}

/// Processes `character` while outside of quotes, returning `false` if `MAX_ARGS_PER_STMT` is exceeded.
fn step_unquoted(
    character: char,
    state: &mut QuoteState,
    in_token: &mut bool,
    current: &mut String,
    tokens: &mut Vec<String>,
) -> bool {
    match character {
        '\'' => {
            *in_token = true;
            *state = QuoteState::Single;
        }
        '"' => {
            *in_token = true;
            *state = QuoteState::Double;
        }
        '\\' => {
            *in_token = true;
            *state = QuoteState::UnquotedEscape;
        }
        _ if character.is_whitespace() && *in_token => {
            if tokens.len() >= MAX_ARGS_PER_STMT {
                return false;
            }
            tokens.push(mem::take(current));
            *in_token = false;
        }
        _ if character.is_whitespace() => {}
        _ => {
            *in_token = true;
            current.push(character);
        }
    }
    true
}
