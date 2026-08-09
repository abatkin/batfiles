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
/// Deliberately small: one enum with a handful of variants, grown sparingly
/// rather than a variant per micro-failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConfigError {
    /// A `BATFILES_VAR_<NAME>` suffix or a `--var` key was not a valid
    /// user-variable name ([`crate::var::VarName`]). Carries the rejected name.
    #[allow(
        dead_code,
        reason = "the variable merge that rejects names is not wired yet"
    )]
    InvalidVarName(String),
    /// No home directory could be determined for a command that needs one.
    HomeUnavailable,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVarName(name) => {
                write!(f, "`{name}` is not a valid variable name")
            }
            Self::HomeUnavailable => {
                write!(f, "could not determine a home directory")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_errors_render_the_offending_name() {
        assert_eq!(
            ConfigError::InvalidVarName("1up".to_owned()).to_string(),
            "`1up` is not a valid variable name"
        );
    }
}
