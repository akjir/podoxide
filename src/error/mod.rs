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

//! Central domain error and compiler-style diagnostic types for Podoxide.

mod cli;
mod config;
mod diagnostic;
mod psl;
mod quote_scan;
mod recipe;
mod span;

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io;

pub use self::cli::{CliError, CliErrorKind};
pub use self::config::{ConfigError, ConfigErrorKind};
pub use self::diagnostic::{DiagnosticError, build_caret_annotation};
pub use self::psl::{PslError, PslErrorKind};
pub use self::recipe::{RecipeError, RecipeErrorKind};

/// Maximum byte length stored or formatted for an offending CLI argument, PSL token, or line snippet.
pub const MAX_ERROR_TOKEN_BYTES: usize = 256;

/// Top-level error type encompassing all failure modes in Podoxide.
#[derive(Debug)]
pub enum PodError {
    Cli(CliError),
    Config(ConfigError),
    Io(io::Error),
    Psl(PslError),
    Recipe(RecipeError),
    System(SystemError),
}

impl PodError {
    /// Returns the attached [`DiagnosticError`] when this error carries source-code diagnostics.
    #[must_use]
    pub fn diagnostic(&self) -> Option<&DiagnosticError<'static>> {
        match self {
            Self::Config(error) => error.diagnostic.as_deref(),
            Self::Psl(error) => error.diagnostic.as_deref(),
            Self::Recipe(error) => error.diagnostic.as_deref(),
            _ => None,
        }
    }
}

impl Display for PodError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cli(error) => write!(formatter, "CLI error: {error}"),
            Self::Config(error) => write!(formatter, "Config error: {error}"),
            Self::Io(error) => {
                formatter.write_str("I/O error: ")?;
                diagnostic::write_sanitized_io_message(formatter, &error.to_string())
            }
            Self::Psl(error) => write!(formatter, "PSL error: {error}"),
            Self::Recipe(error) => write!(formatter, "Recipe error: {error}"),
            Self::System(error) => write!(formatter, "{error}"),
        }
    }
}

impl Error for PodError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Cli(error) => Some(error),
            Self::Config(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Psl(error) => Some(error),
            Self::Recipe(error) => Some(error),
            Self::System(error) => Some(error),
        }
    }
}

impl From<CliError> for PodError {
    fn from(error: CliError) -> Self {
        Self::Cli(error)
    }
}

impl From<ConfigError> for PodError {
    fn from(error: ConfigError) -> Self {
        Self::Config(error)
    }
}

impl From<PslError> for PodError {
    fn from(error: PslError) -> Self {
        Self::Psl(error)
    }
}

impl From<RecipeError> for PodError {
    fn from(error: RecipeError) -> Self {
        Self::Recipe(error)
    }
}

impl From<SystemError> for PodError {
    fn from(error: SystemError) -> Self {
        Self::System(error)
    }
}

impl From<io::Error> for PodError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Host operating system, runtime dependency, or privilege verification error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SystemError {
    AbortedExecution,
    UnsupportedOs,
    UnsupportedPodmanVersion,
}

impl Display for SystemError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::AbortedExecution => formatter.write_str("Aborting execution."),
            Self::UnsupportedOs => formatter.write_str("Only Linux is supported."),
            Self::UnsupportedPodmanVersion => {
                formatter.write_str("Podman 5.8.0 or higher is required.")
            }
        }
    }
}

impl Error for SystemError {}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;

    use super::{
        CliError, CliErrorKind, ConfigError, ConfigErrorKind, MAX_ERROR_TOKEN_BYTES, PodError,
        PslError, PslErrorKind, RecipeError, RecipeErrorKind, SystemError,
    };

    #[test]
    fn pod_error_cli_config_psl_recipe_display_and_source() {
        let cli_error = CliError::with_argument(CliErrorKind::UnknownFlag, "--unknown");
        let pod_cli = PodError::from(cli_error.clone());
        assert!(pod_cli.to_string().contains("CLI error"));
        assert!(pod_cli.to_string().contains("--unknown"));
        assert!(pod_cli.diagnostic().is_none());
        assert!(pod_cli.source().is_some());
        assert!(cli_error.source().is_none());

        let config_error =
            ConfigError::with_token(ConfigErrorKind::UnknownDirective, 7, "unknown_key");
        let pod_config = PodError::from(config_error.clone());
        assert!(pod_config.to_string().contains("Config error"));
        assert!(pod_config.to_string().contains("line 7"));
        assert!(pod_config.to_string().contains("unknown_key"));
        assert!(pod_config.source().is_some());
        assert!(config_error.source().is_none());

        let psl_error = PslError::new(PslErrorKind::UnexpectedRootDirective, 4);
        let pod_psl = PodError::from(psl_error.clone());
        assert!(pod_psl.to_string().contains("PSL error"));
        assert!(pod_psl.to_string().contains("line 4"));
        assert!(pod_psl.to_string().contains("root level"));
        assert!(pod_psl.source().is_some());
        assert!(psl_error.source().is_none());

        let recipe_error =
            RecipeError::with_token(RecipeErrorKind::UnknownDirective, 9, "unknown_key");
        let pod_recipe = PodError::from(recipe_error.clone());
        assert!(pod_recipe.to_string().contains("Recipe error"));
        assert!(pod_recipe.to_string().contains("line 9"));
        assert!(pod_recipe.to_string().contains("unknown_key"));
        assert!(pod_recipe.source().is_some());
        assert!(recipe_error.source().is_none());
    }

    #[test]
    fn pod_error_io_sanitizes_controls_and_preserves_os_reason_for_long_paths() {
        let short_io = PodError::from(io::Error::new(io::ErrorKind::NotFound, "missing file"));
        assert!(short_io.to_string().contains("I/O error"));
        assert!(short_io.to_string().contains("missing file"));
        assert!(short_io.diagnostic().is_none());
        assert!(short_io.source().is_some());

        let injected =
            "bad\nINFO: Forged\rWARNING: Overwrite\x1b[31mANSI\0null\u{009b}CSI\u{202e}RLO";
        let injected_display =
            PodError::from(io::Error::new(io::ErrorKind::InvalidData, injected)).to_string();
        assert_eq!(injected_display.lines().count(), 1);
        assert!(!injected_display.contains('\x1b'));
        assert!(!injected_display.contains('\0'));
        assert!(!injected_display.contains('\u{202e}'));

        let long_path = format!("/{}/pods.conf", "segment/".repeat(40));
        assert!(long_path.len() > 260);
        let long_io = io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("Failed to read '{long_path}': Permission denied (os error 13)"),
        );
        let rendered = PodError::from(long_io).to_string();
        assert!(
            rendered.starts_with("I/O error: Failed to read '/"),
            "long I/O error should keep leading context: {rendered}"
        );
        assert!(
            rendered.contains("..."),
            "long I/O error should middle-truncate with ellipsis: {rendered}"
        );
        assert!(
            rendered.contains("Permission denied (os error 13)"),
            "long I/O error must preserve trailing OS reason: {rendered}"
        );
        assert!(
            rendered.len() <= "I/O error: ".len() + MAX_ERROR_TOKEN_BYTES,
            "long I/O error must remain bounded"
        );

        let utf8_long = format!("{}ä: Permission denied", "ä".repeat(160));
        let utf8_rendered =
            PodError::from(io::Error::new(io::ErrorKind::PermissionDenied, utf8_long)).to_string();
        assert!(utf8_rendered.contains("Permission denied"));
        assert!(utf8_rendered.len() <= "I/O error: ".len() + MAX_ERROR_TOKEN_BYTES);
    }

    #[test]
    fn pod_error_system_display_and_source() {
        let variants = [
            (SystemError::AbortedExecution, "Aborting execution."),
            (SystemError::UnsupportedOs, "Only Linux is supported."),
            (
                SystemError::UnsupportedPodmanVersion,
                "Podman 5.8.0 or higher is required.",
            ),
        ];

        for (system_error, expected_message) in variants {
            assert_eq!(system_error.to_string(), expected_message);
            assert!(system_error.source().is_none());

            let pod_error = PodError::from(system_error);
            assert_eq!(pod_error.to_string(), expected_message);
            assert!(pod_error.diagnostic().is_none());
            assert!(pod_error.source().is_some());
        }
    }
}
