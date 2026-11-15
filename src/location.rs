//! Location-root resolution.

use std::path::PathBuf;

use crate::env::Environment;
use crate::error::Error;
use crate::filesystem::paths;
use crate::manifest::Manifest;
use crate::manifest::disabled::DisabledItems;
use crate::vars::dynamic::DynamicVarCache;
use crate::vars::machine::MachineVars;

/// The four location options, as parsed from the command line.
#[derive(Debug, Default)]
pub(crate) struct LocationInputs {
    pub batfiles_dir: Option<PathBuf>,
    pub home_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
}

/// Config and cache roots for machine-local state.
#[derive(Debug)]
pub(crate) struct StateRoots {
    /// The directory holding `vars.toml` and `disabled.toml`.
    pub config_dir: PathBuf,
    /// The directory holding the disposable `dynamic-vars.toml` cache.
    pub cache_dir: PathBuf,
}

/// Repository, destination home, config, and cache roots for repository commands.
#[derive(Debug)]
pub(crate) struct Roots {
    /// The destination home: the base for home-relative destinations.
    pub home: PathBuf,
    /// The leaf repository.
    pub batfiles_repo: PathBuf,
    /// Batfiles' own bookkeeping, which a repository command reads as well.
    pub state: StateRoots,
}

impl StateRoots {
    /// Path to machine-local `disabled.toml` under the config root.
    pub fn disabled_path(&self) -> PathBuf {
        self.config_dir.join(DisabledItems::FILE_NAME)
    }

    /// Path to machine-local `vars.toml` under the config root.
    pub fn machine_vars_path(&self) -> PathBuf {
        self.config_dir.join(MachineVars::FILE_NAME)
    }

    /// Path to `dynamic-vars.toml` under the cache root.
    pub fn dynamic_vars_cache_path(&self) -> PathBuf {
        self.cache_dir.join(DynamicVarCache::FILE_NAME)
    }
}

impl Roots {
    /// Path to the leaf repository's manifest.
    pub fn manifest_path(&self) -> PathBuf {
        self.batfiles_repo.join(Manifest::FILE_NAME)
    }
}

/// Detect the invoking user's OS home.
pub(crate) fn detect_os_home() -> Result<PathBuf, Error> {
    std::env::home_dir().ok_or(Error::HomeUnavailable)
}

/// Select the working directory when it contains a manifest.
pub(crate) fn discover_working_repository() -> Result<Option<PathBuf>, Error> {
    let directory = std::env::current_dir().map_err(|source| Error::WorkingDirectory { source })?;
    Ok(paths::occupied(&directory.join(Manifest::FILE_NAME))?.then_some(directory))
}

/// Resolve config and cache roots without repository discovery. Consult the OS home only if an
/// unresolved root needs its fallback.
pub(crate) fn resolve_state_roots(
    cli: &LocationInputs,
    env: &Environment,
    os_home: impl FnOnce() -> Result<PathBuf, Error>,
) -> Result<StateRoots, Error> {
    let config_dir = config_option(cli, env);
    let cache_dir = cache_option(cli, env);
    let os_home = if config_dir.is_none() || cache_dir.is_none() {
        Some(os_home()?)
    } else {
        None
    };
    Ok(state_roots(config_dir, cache_dir, os_home.as_ref()))
}

/// Resolve every root, for a command that works from the leaf repository.
pub(crate) fn resolve_roots(
    cli: &LocationInputs,
    env: &Environment,
    working_repository: impl FnOnce() -> Result<Option<PathBuf>, Error>,
    os_home: impl FnOnce() -> Result<PathBuf, Error>,
) -> Result<Roots, Error> {
    let home = cli
        .home_dir
        .clone()
        .or_else(|| env.path_var("BATFILES_HOME"));
    let batfiles_repo = match cli
        .batfiles_dir
        .clone()
        .or_else(|| env.path_var("BATFILES_DIR"))
    {
        Some(path) => Some(path),
        None => working_repository()?,
    };
    let config_dir = config_option(cli, env);
    let cache_dir = cache_option(cli, env);

    let os_home = if home.is_none() || config_dir.is_none() || cache_dir.is_none() {
        Some(os_home()?)
    } else {
        None
    };

    // The repository default follows the selected home.
    let home = home.unwrap_or_else(|| os_home.as_ref().expect(RESOLVED).clone());
    let batfiles_repo = batfiles_repo.unwrap_or_else(|| home.join("dotfiles"));

    Ok(Roots {
        home,
        batfiles_repo,
        state: state_roots(config_dir, cache_dir, os_home.as_ref()),
    })
}

/// Assertion message for a missing home required by a fallback.
const RESOLVED: &str = "the OS home is resolved whenever a root needs it";

/// Fill unresolved config and cache roots from OS-home defaults. `os_home` must be `Some` if
/// either root is `None`.
fn state_roots(
    config_dir: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    os_home: Option<&PathBuf>,
) -> StateRoots {
    // State defaults use the OS home, not the selected destination home.
    StateRoots {
        config_dir: config_dir
            .unwrap_or_else(|| os_home.expect(RESOLVED).join(".config").join("batfiles")),
        cache_dir: cache_dir
            .unwrap_or_else(|| os_home.expect(RESOLVED).join(".cache").join("batfiles")),
    }
}

/// The config root an invocation supplied directly, if any.
fn config_option(cli: &LocationInputs, env: &Environment) -> Option<PathBuf> {
    cli.config_dir
        .clone()
        .or_else(|| env.path_var("BATFILES_CONFIG_DIR"))
        .or_else(|| xdg_base(env, "XDG_CONFIG_HOME"))
}

/// The cache root an invocation supplied directly, if any.
fn cache_option(cli: &LocationInputs, env: &Environment) -> Option<PathBuf> {
    cli.cache_dir
        .clone()
        .or_else(|| env.path_var("BATFILES_CACHE_DIR"))
        .or_else(|| xdg_base(env, "XDG_CACHE_HOME"))
}

/// A batfiles directory rooted at an `$XDG_*` base, or `None` when the variable
/// is absent or empty. Whitespace is significant, as it is for the location
/// variables.
fn xdg_base(env: &Environment, key: &str) -> Option<PathBuf> {
    env.path_var(key).map(|base| base.join("batfiles"))
}

#[cfg(test)]
mod tests;
