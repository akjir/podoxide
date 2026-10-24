//! Core library for the Podoxide CLI tool (`pods`).

#![forbid(unsafe_code)]

pub mod core;
pub mod error;
pub mod io;

pub use crate::core::{Context, cli, config, context, log, modes};
