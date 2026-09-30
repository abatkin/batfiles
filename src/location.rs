//! Location-root resolution.

use std::path::PathBuf;

use crate::disabled::DisabledItems;
use crate::dynamic::DynamicVarCache;
use crate::env::Environment;
use crate::error::Error;
use crate::machine_vars::MachineVars;
use crate::manifest::Manifest;
use crate::paths;

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
mod tests {
    use super::*;

    /// Return a fixed OS home for fallback tests.
    fn os_home() -> Result<PathBuf, Error> {
        Ok(PathBuf::from("/os-home"))
    }

    /// Return a home-resolution error.
    fn unavailable() -> Result<PathBuf, Error> {
        Err(Error::HomeUnavailable)
    }

    fn resolve(cli: LocationInputs, env: &Environment) -> Roots {
        resolve_roots(&cli, env, || Ok(None), os_home).expect("resolution should succeed")
    }

    fn empty_env() -> Environment {
        Environment::from_pairs([] as [(&str, &str); 0])
    }

    #[test]
    fn defaults_come_from_the_os_home() {
        let roots = resolve(LocationInputs::default(), &empty_env());
        assert_eq!(roots.home, PathBuf::from("/os-home"));
        assert_eq!(roots.batfiles_repo, PathBuf::from("/os-home/dotfiles"));
        assert_eq!(
            roots.state.config_dir,
            PathBuf::from("/os-home/.config/batfiles")
        );
        assert_eq!(
            roots.state.cache_dir,
            PathBuf::from("/os-home/.cache/batfiles")
        );
    }

    #[test]
    fn the_environment_overrides_the_defaults() {
        let env = Environment::from_pairs([
            ("BATFILES_HOME", "/env-home"),
            ("BATFILES_DIR", "/env-repo"),
            ("BATFILES_CONFIG_DIR", "/env-config"),
            ("BATFILES_CACHE_DIR", "/env-cache"),
        ]);
        let roots = resolve(LocationInputs::default(), &env);
        assert_eq!(roots.home, PathBuf::from("/env-home"));
        assert_eq!(roots.batfiles_repo, PathBuf::from("/env-repo"));
        assert_eq!(roots.state.config_dir, PathBuf::from("/env-config"));
        assert_eq!(roots.state.cache_dir, PathBuf::from("/env-cache"));
    }

    #[test]
    fn options_outrank_the_environment() {
        let env = Environment::from_pairs([
            ("BATFILES_HOME", "/env-home"),
            ("BATFILES_DIR", "/env-repo"),
        ]);
        let cli = LocationInputs {
            home_dir: Some(PathBuf::from("/opt-home")),
            batfiles_dir: Some(PathBuf::from("/opt-repo")),
            ..LocationInputs::default()
        };
        let roots = resolve(cli, &env);
        assert_eq!(roots.home, PathBuf::from("/opt-home"));
        assert_eq!(roots.batfiles_repo, PathBuf::from("/opt-repo"));
    }

    #[test]
    fn the_leaf_repository_default_follows_the_selected_home() {
        let cli = LocationInputs {
            home_dir: Some(PathBuf::from("/alt-home")),
            ..LocationInputs::default()
        };
        let roots = resolve(cli, &empty_env());
        assert_eq!(roots.batfiles_repo, PathBuf::from("/alt-home/dotfiles"));
    }

    #[test]
    fn a_manifest_in_the_working_directory_selects_that_repository() {
        let roots = resolve_roots(
            &LocationInputs::default(),
            &empty_env(),
            || Ok(Some(PathBuf::from("/working-repo"))),
            os_home,
        )
        .expect("resolution should succeed");

        assert_eq!(roots.batfiles_repo, PathBuf::from("/working-repo"));
    }

    #[test]
    fn the_repository_variable_outranks_a_manifest_in_the_working_directory() {
        let env = Environment::from_pairs([("BATFILES_DIR", "/env-repo")]);

        let roots = resolve_roots(
            &LocationInputs::default(),
            &env,
            || Ok(Some(PathBuf::from("/working-repo"))),
            os_home,
        )
        .expect("resolution should succeed");

        assert_eq!(roots.batfiles_repo, PathBuf::from("/env-repo"));
    }

    #[test]
    fn the_repository_option_outranks_a_manifest_in_the_working_directory() {
        let cli = LocationInputs {
            batfiles_dir: Some(PathBuf::from("/option-repo")),
            ..LocationInputs::default()
        };

        let roots = resolve_roots(
            &cli,
            &empty_env(),
            || Ok(Some(PathBuf::from("/working-repo"))),
            os_home,
        )
        .expect("resolution should succeed");

        assert_eq!(roots.batfiles_repo, PathBuf::from("/option-repo"));
    }

    #[test]
    fn state_roots_resolve_the_way_the_full_set_resolves_them() {
        let env = Environment::from_pairs([
            ("BATFILES_CONFIG_DIR", "/env-config"),
            ("XDG_CACHE_HOME", "/xdg/cache"),
        ]);
        let cli = LocationInputs {
            config_dir: Some(PathBuf::from("/opt-config")),
            ..LocationInputs::default()
        };

        let state = resolve_state_roots(&cli, &env, os_home).expect("resolution should succeed");

        assert_eq!(state.config_dir, PathBuf::from("/opt-config"));
        assert_eq!(state.cache_dir, PathBuf::from("/xdg/cache/batfiles"));
    }

    #[test]
    fn state_roots_fall_back_to_the_os_home() {
        let state = resolve_state_roots(&LocationInputs::default(), &empty_env(), os_home)
            .expect("resolution should succeed");

        assert_eq!(state.config_dir, PathBuf::from("/os-home/.config/batfiles"));
        assert_eq!(state.cache_dir, PathBuf::from("/os-home/.cache/batfiles"));
    }

    #[test]
    fn covered_state_roots_never_need_a_home_at_all() {
        let env = Environment::from_pairs([
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_CACHE_HOME", "/xdg/cache"),
        ]);

        let state = resolve_state_roots(&LocationInputs::default(), &env, unavailable)
            .expect("covered state roots should not need the OS home");

        assert_eq!(state.config_dir, PathBuf::from("/xdg/config/batfiles"));
    }

    #[test]
    fn a_state_root_left_uncovered_still_requires_the_os_home() {
        let env = Environment::from_pairs([("XDG_CONFIG_HOME", "/xdg/config")]);
        assert!(matches!(
            resolve_state_roots(&LocationInputs::default(), &env, unavailable),
            Err(Error::HomeUnavailable)
        ));
    }

    #[test]
    fn selecting_a_home_does_not_move_config_or_cache() {
        let cli = LocationInputs {
            home_dir: Some(PathBuf::from("/alt-home")),
            ..LocationInputs::default()
        };
        let env = Environment::from_pairs([("BATFILES_HOME", "/env-home")]);
        let roots = resolve(cli, &env);
        assert_eq!(
            roots.state.config_dir,
            PathBuf::from("/os-home/.config/batfiles")
        );
        assert_eq!(
            roots.state.cache_dir,
            PathBuf::from("/os-home/.cache/batfiles")
        );
    }

    #[test]
    fn xdg_bases_default_config_and_cache() {
        let env = Environment::from_pairs([
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_CACHE_HOME", "/xdg/cache"),
        ]);
        let roots = resolve(LocationInputs::default(), &env);
        assert_eq!(
            roots.state.config_dir,
            PathBuf::from("/xdg/config/batfiles")
        );
        assert_eq!(roots.state.cache_dir, PathBuf::from("/xdg/cache/batfiles"));
    }

    #[test]
    fn an_empty_location_variable_falls_through_to_the_default() {
        let env = Environment::from_pairs([("BATFILES_DIR", "")]);
        let roots = resolve(LocationInputs::default(), &env);
        assert_eq!(roots.batfiles_repo, PathBuf::from("/os-home/dotfiles"));
    }

    #[test]
    fn supplying_every_root_never_resolves_the_os_home() {
        let cli = LocationInputs {
            batfiles_dir: Some(PathBuf::from("/repo")),
            home_dir: Some(PathBuf::from("/home")),
            config_dir: Some(PathBuf::from("/config")),
            cache_dir: Some(PathBuf::from("/cache")),
        };
        let roots = resolve_roots(&cli, &empty_env(), || Ok(None), unavailable)
            .expect("explicit roots should not need the OS home");
        assert_eq!(roots.state.config_dir, PathBuf::from("/config"));
        assert_eq!(roots.state.cache_dir, PathBuf::from("/cache"));
    }

    #[test]
    fn xdg_bases_remove_the_need_for_an_os_home() {
        let cli = LocationInputs {
            home_dir: Some(PathBuf::from("/home")),
            ..LocationInputs::default()
        };
        let env = Environment::from_pairs([
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_CACHE_HOME", "/xdg/cache"),
        ]);
        let roots = resolve_roots(&cli, &env, || Ok(None), unavailable)
            .expect("XDG bases should remove the need for an OS home");
        assert_eq!(roots.home, PathBuf::from("/home"));
        assert_eq!(roots.batfiles_repo, PathBuf::from("/home/dotfiles"));
        assert_eq!(
            roots.state.config_dir,
            PathBuf::from("/xdg/config/batfiles")
        );
        assert_eq!(roots.state.cache_dir, PathBuf::from("/xdg/cache/batfiles"));
    }

    #[test]
    fn a_base_left_uncovered_still_requires_the_os_home() {
        // Only the cache root still needs the unavailable OS home.
        let cli = LocationInputs {
            home_dir: Some(PathBuf::from("/home")),
            batfiles_dir: Some(PathBuf::from("/repo")),
            ..LocationInputs::default()
        };
        let env = Environment::from_pairs([("XDG_CONFIG_HOME", "/xdg/config")]);
        assert!(matches!(
            resolve_roots(&cli, &env, || Ok(None), unavailable),
            Err(Error::HomeUnavailable)
        ));
    }

    #[test]
    fn a_missing_home_requires_the_os_home() {
        assert!(matches!(
            resolve_roots(
                &LocationInputs::default(),
                &empty_env(),
                || Ok(None),
                unavailable
            ),
            Err(Error::HomeUnavailable)
        ));
    }
}
