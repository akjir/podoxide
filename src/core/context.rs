//! Central runtime context holding configuration, parsed CLI flags, mode, and parameters.

use std::fmt::{self, Display, Formatter};
use std::mem;
use std::path::PathBuf;
use std::str::FromStr;

use crate::core::config::Config;
use crate::core::log;
use crate::error::{CliError, CliErrorKind, PodError};

/// Sub-actions supported by [`Mode::Command`] (`command` / `cmd`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CommandAction {
    Exec,
    #[default]
    Help,
    List,
}

impl Display for CommandAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Exec => "exec",
            Self::Help => "help",
            Self::List => "list",
        };
        formatter.write_str(label)
    }
}

impl FromStr for CommandAction {
    type Err = CliError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "exec" => Ok(Self::Exec),
            "help" => Ok(Self::Help),
            "list" => Ok(Self::List),
            _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
        }
    }
}

/// Sub-actions supported by [`Mode::Config`] (`config` / `cfg`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConfigAction {
    Edit,
    Help,
    #[default]
    Show,
}

impl Display for ConfigAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Edit => "edit",
            Self::Help => "help",
            Self::Show => "show",
        };
        formatter.write_str(label)
    }
}

impl FromStr for ConfigAction {
    type Err = CliError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "edit" => Ok(Self::Edit),
            "help" => Ok(Self::Help),
            "show" => Ok(Self::Show),
            _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
        }
    }
}

/// Sub-actions supported by [`Mode::Connect`] (`connect` / `con`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConnectAction {
    #[default]
    Help,
    Shell,
}

impl Display for ConnectAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Help => "help",
            Self::Shell => "shell",
        };
        formatter.write_str(label)
    }
}

impl FromStr for ConnectAction {
    type Err = CliError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "help" => Ok(Self::Help),
            "shell" => Ok(Self::Shell),
            _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
        }
    }
}

/// Application runtime context passed through CLI parsing, mode dispatchers, and action handlers.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Context {
    pub action: ModeAction,
    pub config: Config,
    pub flags: Flags,
    pub mode: Mode,
    pub parameters: Vec<String>,
}

impl Context {
    /// Creates a new [`Context`] initialized with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the resolved [`ModeAction`] for this invocation.
    #[inline]
    #[must_use]
    pub const fn action(&self) -> ModeAction {
        self.action
    }

    /// Returns the [`DefaultAction`] when the selected mode is [`Mode::Default`].
    #[inline]
    #[must_use]
    pub const fn default_action(&self) -> Option<DefaultAction> {
        match self.action {
            ModeAction::Default(action) => Some(action),
            _ => None,
        }
    }

    /// Loads `pods.conf` if required by the selected mode and syncs the `simulate` flag.
    ///
    /// # Errors
    ///
    /// Returns [`PodError`] if loading or validating the configuration fails.
    pub fn load_config_if_required(&mut self) -> Result<(), PodError> {
        if self.requires_config() {
            let mut loaded = Config::load(&self.config.path)?;
            loaded.name = mem::take(&mut self.config.name);
            loaded.path = mem::take(&mut self.config.path);
            loaded.simulate |= self.flags.simulate;
            self.config = loaded;
            if self.config.simulate {
                log::info("Simulate mode is active.");
            }
        }
        Ok(())
    }

    /// Returns the selected [`Mode`].
    #[inline]
    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Returns the positional parameter arguments slice.
    #[inline]
    #[must_use]
    pub fn parameters(&self) -> &[String] {
        &self.parameters
    }

    /// Returns `true` if this context's mode and action require loading `pods.conf`.
    #[must_use]
    pub fn requires_config(&self) -> bool {
        if self.flags.help || self.flags.version || self.action.is_help() {
            return false;
        }
        match self.action {
            ModeAction::Default(DefaultAction::Status) => {
                !self.parameters.first().is_some_and(|parameter| parameter == "help")
            }
            ModeAction::Image(ImageAction::Prune) | ModeAction::Init(_) => false,
            _ => true,
        }
    }
}

/// Lifecycle and status actions supported by [`Mode::Default`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DefaultAction {
    Create,
    Down,
    #[default]
    Help,
    Reload,
    Remove,
    Restart,
    Start,
    Status,
    Stop,
    Up,
    Update,
}

impl DefaultAction {
    /// Returns `true` if this default action requires at least one target argument.
    #[must_use]
    pub const fn requires_targets(self) -> bool {
        !matches!(self, Self::Help | Self::Status)
    }
}

impl Display for DefaultAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Create => "create",
            Self::Down => "down",
            Self::Help => "help",
            Self::Reload => "reload",
            Self::Remove => "remove",
            Self::Restart => "restart",
            Self::Start => "start",
            Self::Status => "status",
            Self::Stop => "stop",
            Self::Up => "up",
            Self::Update => "update",
        };
        formatter.write_str(label)
    }
}

impl FromStr for DefaultAction {
    type Err = CliError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "create" => Ok(Self::Create),
            "down" => Ok(Self::Down),
            "help" => Ok(Self::Help),
            "reload" => Ok(Self::Reload),
            "remove" => Ok(Self::Remove),
            "restart" => Ok(Self::Restart),
            "start" => Ok(Self::Start),
            "status" => Ok(Self::Status),
            "stop" => Ok(Self::Stop),
            "up" => Ok(Self::Up),
            "update" => Ok(Self::Update),
            _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
        }
    }
}

/// Command-line flags stored in [`Context`].
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Flags {
    pub all: bool,
    pub config: Option<PathBuf>,
    pub debug: bool,
    pub force: bool,
    pub full: bool,
    pub help: bool,
    pub preview: bool,
    pub simulate: bool,
    pub since: Option<String>,
    pub tail: Option<String>,
    pub timestamps: bool,
    pub until: Option<String>,
    pub version: bool,
}

impl Flags {
    /// Creates a new [`Flags`] instance with all flags unset.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Sub-actions supported by [`Mode::Image`] (`image` / `img`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ImageAction {
    #[default]
    Help,
    Prune,
    Update,
}

impl Display for ImageAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Help => "help",
            Self::Prune => "prune",
            Self::Update => "update",
        };
        formatter.write_str(label)
    }
}

impl FromStr for ImageAction {
    type Err = CliError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "help" => Ok(Self::Help),
            "prune" => Ok(Self::Prune),
            "update" => Ok(Self::Update),
            _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
        }
    }
}

/// Sub-actions supported by [`Mode::Init`] (`init`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum InitAction {
    #[default]
    Create,
    Help,
}

impl Display for InitAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Create => "create",
            Self::Help => "help",
        };
        formatter.write_str(label)
    }
}

impl FromStr for InitAction {
    type Err = CliError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "help" => Ok(Self::Help),
            _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
        }
    }
}

/// Sub-actions supported by [`Mode::Logs`] (`logs` / `log`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LogsAction {
    Follow,
    #[default]
    Help,
    Show,
}

impl Display for LogsAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Follow => "follow",
            Self::Help => "help",
            Self::Show => "show",
        };
        formatter.write_str(label)
    }
}

impl FromStr for LogsAction {
    type Err = CliError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "follow" => Ok(Self::Follow),
            "help" => Ok(Self::Help),
            "show" => Ok(Self::Show),
            _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
        }
    }
}

/// Top-level CLI execution modes supported by Podoxide (`pods`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
    Command,
    Config,
    Connect,
    #[default]
    Default,
    Image,
    Init,
    Logs,
    Recipe,
}

impl Mode {
    /// Returns a 0-based index suitable for O(1) static array dispatch.
    #[inline]
    #[must_use]
    pub const fn as_index(self) -> usize {
        match self {
            Self::Command => 0,
            Self::Config => 1,
            Self::Connect => 2,
            Self::Default => 3,
            Self::Image => 4,
            Self::Init => 5,
            Self::Logs => 6,
            Self::Recipe => 7,
        }
    }
}

impl Display for Mode {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Command => "command",
            Self::Config => "config",
            Self::Connect => "connect",
            Self::Default => "default",
            Self::Image => "image",
            Self::Init => "init",
            Self::Logs => "logs",
            Self::Recipe => "recipe",
        };
        formatter.write_str(label)
    }
}

impl FromStr for Mode {
    type Err = CliError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "command" | "cmd" => Ok(Self::Command),
            "config" | "cfg" => Ok(Self::Config),
            "connect" | "con" => Ok(Self::Connect),
            "image" | "img" => Ok(Self::Image),
            "init" => Ok(Self::Init),
            "logs" | "log" => Ok(Self::Logs),
            "recipe" | "rec" => Ok(Self::Recipe),
            _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
        }
    }
}

/// Strongly typed mode-and-action pair resolved from positional CLI arguments.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModeAction {
    Command(CommandAction),
    Config(ConfigAction),
    Connect(ConnectAction),
    Default(DefaultAction),
    Image(ImageAction),
    Init(InitAction),
    Logs(LogsAction),
    Recipe(RecipeAction),
}

impl ModeAction {
    /// Returns `true` if the resolved action displays help text.
    #[inline]
    #[must_use]
    pub const fn is_help(self) -> bool {
        matches!(
            self,
            Self::Command(CommandAction::Help)
                | Self::Config(ConfigAction::Help)
                | Self::Connect(ConnectAction::Help)
                | Self::Default(DefaultAction::Help)
                | Self::Image(ImageAction::Help)
                | Self::Init(InitAction::Help)
                | Self::Logs(LogsAction::Help)
                | Self::Recipe(RecipeAction::Help)
        )
    }

    /// Returns the [`Mode`] corresponding to this [`ModeAction`].
    #[inline]
    #[must_use]
    pub const fn mode(self) -> Mode {
        match self {
            Self::Command(_) => Mode::Command,
            Self::Config(_) => Mode::Config,
            Self::Connect(_) => Mode::Connect,
            Self::Default(_) => Mode::Default,
            Self::Image(_) => Mode::Image,
            Self::Init(_) => Mode::Init,
            Self::Logs(_) => Mode::Logs,
            Self::Recipe(_) => Mode::Recipe,
        }
    }
}

impl Default for ModeAction {
    fn default() -> Self {
        Self::Default(DefaultAction::Help)
    }
}

impl Display for ModeAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command(action) => write!(formatter, "{action}"),
            Self::Config(action) => write!(formatter, "{action}"),
            Self::Connect(action) => write!(formatter, "{action}"),
            Self::Default(action) => write!(formatter, "{action}"),
            Self::Image(action) => write!(formatter, "{action}"),
            Self::Init(action) => write!(formatter, "{action}"),
            Self::Logs(action) => write!(formatter, "{action}"),
            Self::Recipe(action) => write!(formatter, "{action}"),
        }
    }
}

/// Sub-actions supported by [`Mode::Recipe`] (`recipe` / `rec`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RecipeAction {
    Edit,
    Help,
    #[default]
    List,
    Show,
}

impl Display for RecipeAction {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Edit => "edit",
            Self::Help => "help",
            Self::List => "list",
            Self::Show => "show",
        };
        formatter.write_str(label)
    }
}

impl FromStr for RecipeAction {
    type Err = CliError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "edit" => Ok(Self::Edit),
            "help" => Ok(Self::Help),
            "list" => Ok(Self::List),
            "show" => Ok(Self::Show),
            _ => Err(CliError::with_argument(CliErrorKind::UnknownAction, input)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::str::FromStr;

    use crate::core::cli::parse_args;
    use crate::core::config::{Config, PodsConfig, RecipesConfig};
    use crate::error::CliErrorKind;
    use super::{
        CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, Flags, ImageAction,
        InitAction, LogsAction, Mode, ModeAction, RecipeAction,
    };

    #[test]
    fn context_defaults_and_cli_population() {
        let default_context = Context::new();
        assert_eq!(default_context.mode, Mode::Default);
        assert_eq!(default_context.mode(), Mode::Default);
        assert_eq!(
            default_context.action,
            ModeAction::Default(DefaultAction::Help)
        );
        assert_eq!(
            default_context.action(),
            ModeAction::Default(DefaultAction::Help)
        );
        assert_eq!(default_context.config, Config::new());
        assert_eq!(default_context.config.editor, "");
        assert_eq!(default_context.config.name, "pods");
        assert_eq!(default_context.config.path, PathBuf::from("./pods.conf"));
        assert_eq!(default_context.config.pods, PodsConfig::new());
        assert_eq!(default_context.config.recipes, RecipesConfig::new());
        assert!(default_context.config.simulate);
        assert_eq!(default_context.flags, Flags::new());
        assert!(!default_context.flags.simulate);
        assert!(default_context.parameters.is_empty());
        assert!(!default_context.requires_config());

        let mut context = Context::new();
        parse_args(
            &mut context,
            ["--config=staging", "--debug", "up", "example-app"],
        )
        .unwrap();
        assert!(context.flags.debug);
        assert!(!context.flags.simulate);
        assert!(context.config.simulate);
        assert_eq!(context.config.name, "staging");
        assert_eq!(context.config.path, PathBuf::from("./staging.conf"));
        assert_eq!(context.parameters, vec!["example-app"]);
        assert_eq!(context.parameters(), &["example-app"]);
        assert!(context.requires_config());

        context.config.simulate = false;
        context.load_config_if_required().unwrap();
        assert_eq!(context.config.name, "staging");
        assert_eq!(context.config.path, PathBuf::from("./staging.conf"));
        assert!(context.config.simulate);

        let mut sim_context = Context::new();
        parse_args(&mut sim_context, ["--simulate", "up", "example-app"]).unwrap();
        assert!(sim_context.flags.simulate);
        assert!(sim_context.config.simulate);
        sim_context.load_config_if_required().unwrap();
        assert!(sim_context.config.simulate);

        let mut init_context = Context::new();
        parse_args(&mut init_context, ["init"]).unwrap();
        assert!(!init_context.requires_config());
        init_context.config.simulate = false;
        init_context.load_config_if_required().unwrap();
        assert!(!init_context.config.simulate);
        assert!(!init_context.flags.simulate);
    }

    #[test]
    fn display_and_from_str_fallbacks() {
        assert_eq!(
            CommandAction::from_str("unknown").unwrap_err().kind,
            CliErrorKind::UnknownAction
        );
        assert_eq!(
            ConfigAction::from_str("unknown").unwrap_err().kind,
            CliErrorKind::UnknownAction
        );
        assert_eq!(
            ConnectAction::from_str("unknown").unwrap_err().kind,
            CliErrorKind::UnknownAction
        );
        assert_eq!(
            DefaultAction::from_str("unknown").unwrap_err().kind,
            CliErrorKind::UnknownAction
        );
        assert_eq!(
            ImageAction::from_str("unknown").unwrap_err().kind,
            CliErrorKind::UnknownAction
        );
        assert_eq!(
            InitAction::from_str("unknown").unwrap_err().kind,
            CliErrorKind::UnknownAction
        );
        assert_eq!(
            LogsAction::from_str("unknown").unwrap_err().kind,
            CliErrorKind::UnknownAction
        );
        assert_eq!(
            Mode::from_str("unknown").unwrap_err().kind,
            CliErrorKind::UnknownAction
        );
        assert_eq!(
            RecipeAction::from_str("unknown").unwrap_err().kind,
            CliErrorKind::UnknownAction
        );

        assert_eq!(Mode::Command.to_string(), "command");
        assert_eq!(Mode::Config.to_string(), "config");
        assert_eq!(Mode::Connect.to_string(), "connect");
        assert_eq!(Mode::Default.to_string(), "default");
        assert_eq!(Mode::Image.to_string(), "image");
        assert_eq!(Mode::Init.to_string(), "init");
        assert_eq!(Mode::Logs.to_string(), "logs");
        assert_eq!(Mode::Recipe.to_string(), "recipe");

        assert_eq!(ModeAction::Command(CommandAction::Exec).to_string(), "exec");
        assert_eq!(ModeAction::Config(ConfigAction::Edit).to_string(), "edit");
        assert_eq!(
            ModeAction::Connect(ConnectAction::Shell).to_string(),
            "shell"
        );
        assert_eq!(ModeAction::Default(DefaultAction::Up).to_string(), "up");
        assert_eq!(ModeAction::Image(ImageAction::Prune).to_string(), "prune");
        assert_eq!(ModeAction::Init(InitAction::Create).to_string(), "create");
        assert_eq!(ModeAction::Logs(LogsAction::Follow).to_string(), "follow");
        assert_eq!(ModeAction::Recipe(RecipeAction::Show).to_string(), "show");
    }
}
