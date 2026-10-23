//! Core domain models, CLI argument parsing, runtime context, and mode orchestration.

pub mod cli;
pub mod config;
pub mod context;
pub mod modes;

pub use self::config::{Config, PodsConfig, RecipesConfig};
pub use self::context::{
    CommandAction, ConfigAction, ConnectAction, Context, DefaultAction, Flags, ImageAction,
    InitAction, LogsAction, Mode, ModeAction, RecipeAction,
};
pub use self::modes::dispatch_mode;
