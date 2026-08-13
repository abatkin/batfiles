//! Environment capture and location-root resolution.
//!
//! This module turns the captured process environment and the parsed
//! command-line options into the concrete values commands work with, and owns
//! the precedence rules that combine them.

mod env;
mod paths;

use std::fmt;
use std::path::PathBuf;

pub(crate) use env::Environment;
pub(crate) use paths::{Roots, detect_os_home, resolve_roots};

/// The four location options, as parsed from the command line.
#[derive(Debug, Default, Clone)]
pub(crate) struct LocationInputs {
    pub batfiles_dir: Option<PathBuf>,
    pub home_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
}

/// Configuration diagnostics.
///
/// Deliberately small because much of the validation is done elsewhere
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigError {
    /// No home directory could be determined for a command that needs one.
    HomeUnavailable,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeUnavailable => {
                write!(f, "could not determine a home directory")
            }
        }
    }
}

impl std::error::Error for ConfigError {}
