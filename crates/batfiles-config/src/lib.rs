//! On-disk configuration and conversion to batfiles domain types.
//!
//! This crate owns the boundary between the CLI (parsed clap arguments plus the
//! captured process environment) and batfiles' domain types. Per
//! `docs/environment.md`, it parses, validates, and merges the environment
//! inputs; the CLI only captures, and the core only holds domain rules. No clap
//! type and no `OsString` cross into this crate.

mod env;
mod paths;

use std::fmt;
use std::path::PathBuf;

pub use env::Environment;
pub use paths::{Roots, resolve_roots};

/// The four location options, copied out of the CLI's clap structs.
///
/// Config cannot depend on clap, so the CLI translates its parsed values into
/// these plain fields.
#[derive(Debug, Default, Clone)]
pub struct LocationInputs {
    pub batfiles_dir: Option<PathBuf>,
    pub home_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
}

/// Everything the CLI hands the configuration layer for a full assembly.
///
/// All fields are copied out of clap types so no clap type crosses the crate
/// boundary. The command-specific lists are empty for commands that do not
/// accept the corresponding options.
#[derive(Debug, Default, Clone)]
pub struct CliInputs {
    pub locations: LocationInputs,
    /// `--var KEY=VALUE`, shape-checked but not name-validated by the CLI.
    pub vars: Vec<(String, String)>,
    pub skip_actions: Vec<String>,
    pub skip_groups: Vec<String>,
    pub enable_actions: Vec<String>,
    pub disable_actions: Vec<String>,
    pub enable_groups: Vec<String>,
    pub disable_groups: Vec<String>,
}

/// Configuration diagnostics.
///
/// Deliberately small: one enum with a handful of variants, grown sparingly
/// rather than a variant per micro-failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A `BATFILES_VAR_<NAME>` suffix or a `--var` key was not a valid
    /// user-variable name (`docs/repoformat.md#names-and-ids`). Carries the
    /// rejected name.
    InvalidVarName(String),
    /// No home directory could be determined for a command that needs one
    /// (`docs/environment.md#location-selection`).
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

/// Assemble the resolved roots a command needs from the CLI inputs and the
/// captured environment.
///
/// Root resolution consults the `$XDG_*` bases directly and defers the OS-home
/// lookup to [`resolve_roots`], which performs it only when a root has no
/// higher-precedence value — so explicit options, `BATFILES_*`, and `$XDG_*`
/// bases can satisfy a command even where no home can be determined.
///
/// Only location-root resolution is live in this pass. The remaining work
/// specified in `docs/environment.md` is stubbed:
///
/// - loading `vars.toml` / `disabled.toml`;
/// - validating one-shot variable names via [`batfiles_core::VarName`] and
///   merging the runtime variable scope
///   (`#runtime-variable-precedence`);
/// - unioning the run-only skips (`#run-only-skips`); and
/// - applying bootstrap enable/disable adoption
///   (`#bootstrap-enable-and-disable-lists`).
///
/// Those merges will be added behind smaller functions where a command needs
/// only part of the work (for example `vars set` touches only `vars.toml`),
/// rather than growing this into one monolith.
pub fn assemble(cli: &CliInputs, env: &Environment) -> Result<Roots, ConfigError> {
    resolve_roots(&cli.locations, env, paths::detect_os_home)
}

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
