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

//! Default mode (`pods <action> [targets...]`) action dispatcher.

pub mod create;
pub mod down;
pub mod help;
pub mod reload;
pub mod remove;
pub mod restart;
pub mod start;
pub mod status;
pub mod stop;
pub mod up;
pub mod update;
pub mod version;

use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};

use crate::core::config::Config;
use crate::core::container::Container;
use crate::core::context::{Context, DefaultAction, ModeAction};
use crate::core::log;
use crate::core::pod::Pod;
use crate::core::recipe::Recipe;
use crate::error::{CliError, CliErrorKind, ConfigError, ConfigErrorKind, PodError};
use crate::utilities::collection::{MAX_COLLECTION_PREALLOC, bounded_capacity};
use crate::utilities::string::is_valid_target_segment;

/// Loaded target recipes paired with deduplicated `(recipe_name, container_name)` target tuples.
type ResolvedTargets<'a> = (BTreeMap<&'a str, Recipe>, Vec<(&'a str, String)>);

/// Expands `@<group>`, `<recipe>`, or `<recipe>/<container>` targets from `context.parameters`, loads and validates each targeted `.pod` recipe, and executes `pod_action` or `container_action`.
///
/// # Errors
///
/// Returns [`PodError`] if `context.parameters` is empty, target validation or expansion fails, a targeted `.pod` recipe file cannot be read or validated, a container target cannot be resolved, or the lifecycle action fails.
pub fn execute_lifecycle_action(
    context: &Context,
    pod_action: fn(&Recipe, bool) -> Result<(), PodError>,
    container_action: fn(&Container, &Pod, bool) -> Result<(), PodError>,
) -> Result<(), PodError> {
    let Some(first) = context.parameters.first() else {
        return Err(PodError::from(CliError::new(CliErrorKind::MissingTarget)));
    };
    if first.contains('/') {
        return execute_container_targets(context, container_action);
    }
    execute_pod_targets(context, pod_action)
}

/// Dispatches default mode execution to the resolved lifecycle, status, help, or version action.
///
/// # Workflow
///
/// 1. Log that `default` mode is active when debug logging is enabled.
/// 2. Route [`ModeAction::Default`] in enum declaration order (`Create`, `Down`, `Help`,
///    `Reload`, `Remove`, `Restart`, `Start`, `Status`, `Stop`, `Up`, `Update`, `Version`)
///    to the corresponding action module.
///
/// # Errors
///
/// Returns [`PodError`] if `context.action` does not belong to `default` mode or if the
/// selected action handler fails.
pub fn handle(context: &Context) -> Result<(), PodError> {
    log::debug("Default mode is used.");
    match context.action() {
        ModeAction::Default(DefaultAction::Create) => create::handle(context),
        ModeAction::Default(DefaultAction::Down) => down::handle(context),
        ModeAction::Default(DefaultAction::Help) => help::handle(context),
        ModeAction::Default(DefaultAction::Reload) => reload::handle(context),
        ModeAction::Default(DefaultAction::Remove) => remove::handle(context),
        ModeAction::Default(DefaultAction::Restart) => restart::handle(context),
        ModeAction::Default(DefaultAction::Start) => start::handle(context),
        ModeAction::Default(DefaultAction::Status) => status::handle(context),
        ModeAction::Default(DefaultAction::Stop) => stop::handle(context),
        ModeAction::Default(DefaultAction::Up) => up::handle(context),
        ModeAction::Default(DefaultAction::Update) => update::handle(context),
        ModeAction::Default(DefaultAction::Version) => version::handle(context),
        _ => Err(PodError::from(CliError::with_argument(
            CliErrorKind::UnknownAction,
            context.action().to_string(),
        ))),
    }
}

/// Parses and validates all `<recipe>/<container>` parameters and checks configuration membership.
pub(super) fn collect_container_specs(context: &Context) -> Result<Vec<(&str, &str)>, PodError> {
    let capacity = bounded_capacity(context.parameters.len(), MAX_COLLECTION_PREALLOC);
    let mut specs = Vec::with_capacity(capacity);
    for parameter in &context.parameters {
        let (recipe_name, container_spec) = parse_container_target(parameter)?;
        if !context.config.has_recipe(recipe_name) {
            return Err(PodError::from(ConfigError::without_line(
                ConfigErrorKind::RecipeNotFound,
                recipe_name,
            )));
        }
        specs.push((recipe_name, container_spec));
    }
    Ok(specs)
}

/// Loads target recipes, resolves each container specification, and deduplicates `(recipe, container_name)` pairs in first-appearance order.
pub(super) fn resolve_container_targets<'a>(
    config: &Config,
    specs: &[(&'a str, &'a str)],
) -> Result<ResolvedTargets<'a>, PodError> {
    let capacity = bounded_capacity(specs.len(), MAX_COLLECTION_PREALLOC);
    let mut recipes = BTreeMap::new();
    let mut resolved = Vec::with_capacity(capacity);
    let mut seen = BTreeSet::new();
    for &(recipe_name, container_spec) in specs {
        let recipe = load_or_get_recipe(&mut recipes, config, recipe_name)?;
        let container = recipe.resolve_container(container_spec)?;
        let entry = (recipe_name, container.name.clone());
        if seen.insert(entry.clone()) {
            resolved.push(entry);
        }
    }
    Ok((recipes, resolved))
}

/// Resolves, deduplicates, and executes `container_action` across explicit `<recipe>/<container>` targets.
fn execute_container_targets(
    context: &Context,
    container_action: fn(&Container, &Pod, bool) -> Result<(), PodError>,
) -> Result<(), PodError> {
    if log::is_debug_enabled() {
        log::debug(&format!("Targets   - {}", context.parameters.join(" ")));
    }
    let specs = collect_container_specs(context)?;
    let (recipes, resolved) = resolve_container_targets(&context.config, &specs)?;
    for (recipe_name, container_name) in &resolved {
        let Some(recipe) = recipes.get(recipe_name) else {
            continue;
        };
        let container = recipe.resolve_container(container_name)?;
        container_action(container, &recipe.pod, context.config.simulate)?;
    }
    Ok(())
}

/// Expands `@<group>` and `<recipe>` targets and executes `pod_action` for each recipe.
fn execute_pod_targets(
    context: &Context,
    pod_action: fn(&Recipe, bool) -> Result<(), PodError>,
) -> Result<(), PodError> {
    for parameter in &context.parameters {
        if parameter.contains('/') {
            return Err(PodError::from(CliError::with_argument(
                CliErrorKind::InvalidTarget,
                parameter,
            )));
        }
    }
    let targets = context.config.expand_targets(&context.parameters)?;
    for target in &targets {
        let mut recipe = Recipe::load(&context.config.recipes.path, target)?;
        recipe.validate(&context.config, target)?;
        pod_action(&recipe, context.config.simulate)?;
    }
    Ok(())
}

/// Returns the cached [`Recipe`] for `recipe_name` or loads, validates, and caches it.
fn load_or_get_recipe<'a, 'b>(
    recipes: &'b mut BTreeMap<&'a str, Recipe>,
    config: &Config,
    recipe_name: &'a str,
) -> Result<&'b Recipe, PodError> {
    match recipes.entry(recipe_name) {
        Entry::Occupied(entry) => Ok(entry.into_mut()),
        Entry::Vacant(entry) => {
            let mut recipe = Recipe::load(&config.recipes.path, recipe_name)?;
            recipe.validate(config, recipe_name)?;
            Ok(entry.insert(recipe))
        }
    }
}

/// Splits and validates a `<recipe>/<container>` CLI target specification.
fn parse_container_target(target: &str) -> Result<(&str, &str), PodError> {
    let Some((recipe_name, container_spec)) = target.split_once('/') else {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidTarget,
            target,
        )));
    };
    if !is_valid_target_segment(recipe_name) || !is_valid_target_segment(container_spec) {
        return Err(PodError::from(CliError::with_argument(
            CliErrorKind::InvalidTarget,
            target,
        )));
    }
    Ok((recipe_name, container_spec))
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs as std_fs;
    use std::path::Path;
    use std::process;
    use std::str::FromStr;

    use crate::core::config::{Config, DEFAULT_CONFIG_PATH};
    use crate::core::container;
    use crate::core::context::{CommandAction, Context, DefaultAction, Mode, ModeAction};
    use crate::core::pod;
    use crate::error::{CliErrorKind, ConfigErrorKind, PodError, RecipeErrorKind};
    use crate::io::system::fs;
    use super::{execute_lifecycle_action, handle};

    const ALPHA_RECIPE_PSL: &str = "\
pod alpha_pod {
    name \"Alpha Pod\"
    registry docker.io
}
container *app {
    image alpine:latest
}
";

    const BETA_RECIPE_PSL: &str = "\
pod beta_pod {
    name \"Beta Pod\"
    registry docker.io
}
container *app {
    image alpine:latest
}
";

    #[test]
    fn execute_lifecycle_action_distinguishes_group_and_recipe_sharing_name() {
        let temp_dir = env::temp_dir().join(format!(
            "podoxide_default_same_name_{}",
            process::id()
        ));
        let _ = std_fs::remove_dir_all(&temp_dir);
        let recipes_dir = temp_dir.join("recipes");
        std_fs::create_dir_all(&recipes_dir).unwrap();

        let config_input = "\
general {
    editor vim
    simulate false
}

pods {
    path /pods
}

recipes {
    path ./recipes

    groups {
        all @alpha @stack
        stack media cloud proxy
        alpha beta alpha
    }
}
";
        let mut config = Config::from_str(config_input).unwrap();
        config.simulate = true;
        config.recipes.path = recipes_dir.clone();

        // Write only `alpha.pod` first: targeting `alpha` (recipe) succeeds without `beta.pod`,
        // whereas targeting `@alpha` (group `beta alpha`) requires `beta.pod` and fails with NotFound.
        fs::write_file(&recipes_dir.join("alpha.pod"), ALPHA_RECIPE_PSL).unwrap();

        let mut context = Context::new();
        context.config = config;

        context.parameters = vec![String::from("alpha")];
        assert!(execute_lifecycle_action(&context, pod::up, container::up).is_ok());

        context.parameters = vec![String::from("@alpha")];
        let missing_beta =
            execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
        match missing_beta {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::NotFound);
            }
            other => panic!("expected PodError::Recipe(NotFound) for beta.pod, got {other:?}"),
        }

        // Once `beta.pod` exists, targeting `@alpha` expands both `beta` and `alpha` and succeeds.
        fs::write_file(&recipes_dir.join("beta.pod"), BETA_RECIPE_PSL).unwrap();
        assert!(execute_lifecycle_action(&context, pod::up, container::up).is_ok());

        // Targeting `stack` (defined only as a group, not a recipe) fails with RecipeNotFound.
        context.parameters = vec![String::from("stack")];
        let stack_as_recipe =
            execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
        match stack_as_recipe {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
                assert_eq!(config_error.token.as_deref(), Some("stack"));
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }

        let _ = std_fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn execute_lifecycle_action_handles_container_targets_and_rejects_invalid_specs() {
        let mut context = Context::new();
        context.config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        // Valid container target forms (index, *suffix, short name, full resolved name) deduplicate and succeed in simulate mode.
        context.parameters = vec![
            String::from("recipe/1"),
            String::from("recipe/*db"),
            String::from("recipe/db"),
            String::from("recipe/example-stack-db"),
            String::from("recipe/app"),
        ];
        assert!(execute_lifecycle_action(&context, pod::up, container::up).is_ok());

        // Unknown recipe in container target fails with ConfigErrorKind::RecipeNotFound.
        context.parameters = vec![String::from("unknown_recipe/1")];
        let unknown_recipe =
            execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
        match unknown_recipe {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }

        // Unknown container in valid recipe fails with RecipeErrorKind::ContainerNotFound.
        context.parameters = vec![String::from("recipe/missing")];
        let unknown_container =
            execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
        match unknown_container {
            PodError::Recipe(recipe_error) => {
                assert_eq!(recipe_error.kind, RecipeErrorKind::ContainerNotFound);
            }
            other => panic!("expected PodError::Recipe(ContainerNotFound), got {other:?}"),
        }

        // Mixing pod/group targets and container targets in either order fails with CliErrorKind::InvalidTarget.
        context.parameters = vec![String::from("recipe"), String::from("recipe/1")];
        let mixed_pod_first =
            execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
        match mixed_pod_first {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::InvalidTarget),
            other => panic!("expected PodError::Cli(InvalidTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("recipe/1"), String::from("recipe")];
        let mixed_container_first =
            execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
        match mixed_container_first {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::InvalidTarget),
            other => panic!("expected PodError::Cli(InvalidTarget), got {other:?}"),
        }

        // Malformed <recipe>/<container> segments fail with CliErrorKind::InvalidTarget.
        let invalid_targets = [
            "recipe/",
            "/1",
            "./1",
            "recipe/..",
            "-recipe/1",
            "recipe/-flag",
            "@group/1",
            "recipe/db/extra",
        ];
        for target in invalid_targets {
            context.parameters = vec![String::from(target)];
            let Err(PodError::Cli(error)) =
                execute_lifecycle_action(&context, pod::up, container::up)
            else {
                panic!("expected PodError::Cli(InvalidTarget) for {target}");
            };
            assert_eq!(error.kind, CliErrorKind::InvalidTarget);
        }
    }

    #[test]
    fn execute_lifecycle_action_rejects_empty_and_unknown_targets() {
        let mut context = Context::new();
        context.config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();

        let empty_error = execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
        match empty_error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::MissingTarget),
            other => panic!("expected PodError::Cli(MissingTarget), got {other:?}"),
        }

        context.parameters = vec![String::from("unknown_recipe")];
        let unknown_error = execute_lifecycle_action(&context, pod::up, container::up).unwrap_err();
        match unknown_error {
            PodError::Config(config_error) => {
                assert_eq!(config_error.kind, ConfigErrorKind::RecipeNotFound);
            }
            other => panic!("expected PodError::Config(RecipeNotFound), got {other:?}"),
        }
    }

    #[test]
    fn handle_routes_default_actions_and_rejects_mismatched_action() {
        let root_config = Config::load(Path::new(DEFAULT_CONFIG_PATH)).unwrap();
        let actions = [
            DefaultAction::Create,
            DefaultAction::Down,
            DefaultAction::Help,
            DefaultAction::Reload,
            DefaultAction::Remove,
            DefaultAction::Restart,
            DefaultAction::Start,
            DefaultAction::Status,
            DefaultAction::Stop,
            DefaultAction::Up,
            DefaultAction::Update,
            DefaultAction::Version,
        ];

        for action in actions {
            let mut context = Context::new();
            context.config = root_config.clone();
            context.mode = Mode::Default;
            context.action = ModeAction::Default(action);
            if action.requires_targets() {
                context.parameters = vec![String::from("recipe")];
            }
            assert!(handle(&context).is_ok());
        }

        let mut invalid_context = Context::new();
        invalid_context.mode = Mode::Default;
        invalid_context.action = ModeAction::Command(CommandAction::Help);
        let error = handle(&invalid_context).unwrap_err();
        match error {
            PodError::Cli(cli_error) => assert_eq!(cli_error.kind, CliErrorKind::UnknownAction),
            other => panic!("expected PodError::Cli(UnknownAction), got {other:?}"),
        }
    }
}
