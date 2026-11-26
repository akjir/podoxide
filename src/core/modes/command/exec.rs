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

//! Handler for `pods command exec <recipe> <command|index>`.

use std::io::{self, Write};

use crate::core::context::Context;
use crate::core::pod::PodCommand;
use crate::core::recipe::Recipe;
use crate::error::{CliError, CliErrorKind, PodError, RecipeError, RecipeErrorKind};
use crate::io::exec::command::ExecOutcome;
use crate::io::exec::podman::{self, ExecContainer};
use crate::utilities::string;
use super::tokenizer;

/// Resolves and executes a predefined maintenance command declared in a recipe's `pod` block.
///
/// # Workflow
///
/// 1. Extract `<recipe>` (`context.parameters[0]`) and `<command|index>` (`context.parameters[1]`).
/// 2. Verify that `<recipe>` is registered in `context.config.recipes.groups`.
/// 3. Read and validate only the single targeted recipe file (`<recipes.path>/<recipe>.pod`).
///    Do not read any other `.pod` files in the directory.
/// 4. Resolve the target `command <name>` block from `recipe.pod.commands`:
///    - If `<command|index>` parses as a 1-based positive integer, collect and sort valid
///      commands alphabetically (suppressing missing-description warnings) and select the
///      command at that 1-based index.
///    - Otherwise, look up the command block by its exact identifier `<command>`.
/// 5. Validate that the resolved command block defines valid `container`, `execute`, and
///    optional `user` fields.
/// 6. Resolve `command.container` (`<index>`, `*<relative>`, `<relative>`, or `<full_name>`)
///    against the recipe's container list.
/// 7. Build and execute (or simulate) `podman exec -it [-u <user>] <container_name> <execute>`.
///
/// # Errors
///
/// Returns [`PodError`] if `<recipe>` or `<command|index>` is missing, the recipe is not
/// registered, reading or validating the `.pod` file fails, the command or target container
/// cannot be resolved, or `podman exec` fails.
pub fn handle(context: &Context) -> Result<ExecOutcome, PodError> {
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    execute_with(context, &mut stdout, &mut stderr)
}

/// Resolves, validates, and executes `command_spec` against an already loaded `recipe`.
///
/// # Errors
///
/// Returns [`PodError`] if `command_spec` cannot be resolved, the command definition is
/// incomplete or invalid, the target container does not exist, or `podman exec` fails.
fn exec_command<W: Write, E: Write>(
    recipe: &Recipe,
    command_spec: &str,
    simulate: bool,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    let (key, command) = resolve_command(recipe, command_spec, stderr)?;
    let execute_tokens = validate_command(key, command, command.line)?;
    let container_name = recipe.resolve_container_name(&command.container)?;
    let prefix = format!(
        "Execute command '{}' in container '{container_name}': ",
        command.execute
    );
    podman::exec_container(
        &ExecContainer {
            command: &execute_tokens,
            container_name: &container_name,
            interactive: true,
            user: command.user.as_deref(),
        },
        &prefix,
        simulate,
        stdout,
        stderr,
    )
}

/// Loads the target recipe from `context` and executes the requested command.
///
/// # Errors
///
/// Returns [`PodError`] if required CLI parameters are missing, the recipe is not registered
/// or fails to load, or command resolution/execution fails.
fn execute_with<W: Write, E: Write>(
    context: &Context,
    stdout: &mut W,
    stderr: &mut E,
) -> Result<ExecOutcome, PodError> {
    let (Some(target), Some(command_spec)) =
        (context.parameters.first(), context.parameters.get(1))
    else {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    };
    let recipe = super::load_target_recipe(context, target)?;
    exec_command(
        &recipe,
        command_spec,
        context.config.simulate,
        stdout,
        stderr,
    )
}

/// Resolves `command_spec` by 1-based numeric index or exact command name from `recipe.pod.commands`.
///
/// # Errors
///
/// Returns [`PodError`] with [`RecipeErrorKind::InvalidCommand`] (without blaming a recipe line)
/// when no command matches `command_spec`.
fn resolve_command<'a, E: Write>(
    recipe: &'a Recipe,
    command_spec: &str,
    stderr: &mut E,
) -> Result<(&'a str, &'a PodCommand), PodError> {
    if let Some(index) =
        string::parse_ascii_u32(command_spec).and_then(|value| usize::try_from(value).ok())
    {
        let valid_commands = super::get_valid_commands(recipe, true, stderr);
        return index
            .checked_sub(1)
            .and_then(|zero_based| valid_commands.get(zero_based).copied())
            .map(|command| (command.name.as_str(), command))
            .ok_or_else(|| {
                PodError::from(RecipeError::without_line(
                    RecipeErrorKind::InvalidCommand,
                    command_spec,
                ))
            });
    }
    recipe
        .pod
        .commands
        .get_key_value(command_spec)
        .map(|(key, command)| (key.as_str(), command))
        .ok_or_else(|| {
            PodError::from(RecipeError::without_line(
                RecipeErrorKind::InvalidCommand,
                command_spec,
            ))
        })
}

/// Validates that `command` specifies safe, non-empty `container` and `execute` fields and an optional valid `user` token.
///
/// # Errors
///
/// Returns [`PodError`] with [`RecipeErrorKind::InvalidCommand`] at `line` if `key`/`name`,
/// `container`, or `execute` is missing/invalid, `execute` has unclosed quotes or starts with `-`,
/// or `user` is not a valid token.
fn validate_command(key: &str, command: &PodCommand, line: usize) -> Result<Vec<String>, PodError> {
    if !string::is_valid_identifier(key) || key != command.name {
        return Err(PodError::from(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            key,
        )));
    }
    if !string::is_valid_text(&command.container) {
        return Err(PodError::from(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            &command.name,
        )));
    }
    if !string::is_valid_text(&command.execute) || command.execute.trim_start().starts_with('-') {
        return Err(PodError::from(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            &command.name,
        )));
    }
    let Some(tokens) = tokenizer::split_execute_tokens(&command.execute) else {
        return Err(PodError::from(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            &command.name,
        )));
    };
    if tokens
        .first()
        .is_none_or(|program| program.is_empty() || program.starts_with('-'))
    {
        return Err(PodError::from(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            &command.name,
        )));
    }
    if let Some(user) = &command.user
        && !string::is_valid_token(user)
    {
        return Err(PodError::from(RecipeError::at_line_or_none(
            RecipeErrorKind::InvalidCommand,
            line,
            user,
        )));
    }
    Ok(tokens)
}

#[cfg(test)]
#[path = "exec_tests.rs"]
mod tests;
