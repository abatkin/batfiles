//! Location-root resolution.

use std::path::PathBuf;

use crate::disabled::Disabled;
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

/// Where batfiles keeps its own bookkeeping: the roots a command needs when it
/// reads or edits machine-local state and installs nothing.
///
/// Holds no repository or destination home, so a command given only these
/// cannot reach either.
#[derive(Debug)]
pub(crate) struct StateRoots {
    /// The directory holding `vars.toml` and `disabled.toml`.
    pub config_dir: PathBuf,
    /// The directory holding the disposable `dynamic-vars.toml` cache.
    pub cache_dir: PathBuf,
}

/// The roots a command needs when it works from the leaf repository: the
/// repository itself, the home its destinations are relative to, and the state
/// roots every command has.
#[derive(Debug)]
pub(crate) struct Roots {
    /// The destination home: the base for home-relative destinations.
    pub home: PathBuf,
    /// The leaf repository.
    pub batfiles_dir: PathBuf,
    /// Batfiles' own bookkeeping, which a repository command reads as well.
    pub state: StateRoots,
}

impl StateRoots {
    /// The machine-local disabled lists. Under the config root rather than the
    /// repository, so selecting a different home does not move them.
    pub fn disabled(&self) -> PathBuf {
        self.config_dir.join(Disabled::FILE_NAME)
    }

    /// The machine-local variable values, alongside the disabled lists and for
    /// the same reason: they describe this machine rather than this repository.
    pub fn machine_vars(&self) -> PathBuf {
        self.config_dir.join(MachineVars::FILE_NAME)
    }
}

impl Roots {
    /// The leaf repository's manifest. A remote's manifest is not here: it lives
    /// in that remote's materialization rather than under a resolved root.
    pub fn manifest(&self) -> PathBuf {
        self.batfiles_dir.join(Manifest::FILE_NAME)
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

/// Resolve the state roots alone, for a command that installs nothing and reads
/// no repository.
///
/// Neither the destination home nor the leaf repository is resolved, so no
/// working-directory discovery runs and a machine with no discoverable home can
/// still edit its own state, provided the `$XDG_*` bases cover both roots.
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
    // What the options, BATFILES_* variables, and XDG bases supply directly,
    // before the home-based last resort.
    let home = cli
        .home_dir
        .clone()
        .or_else(|| env.location("BATFILES_HOME"));
    let batfiles_dir = match cli
        .batfiles_dir
        .clone()
        .or_else(|| env.location("BATFILES_DIR"))
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

    // The selected home anchors the leaf-repository default. It reads `os_home`
    // only where its own value was `None`, which is exactly when `os_home` was
    // resolved above.
    let home = home.unwrap_or_else(|| os_home.as_ref().expect(RESOLVED).clone());
    let batfiles_dir = batfiles_dir.unwrap_or_else(|| home.join("dotfiles"));

    Ok(Roots {
        home,
        batfiles_dir,
        state: state_roots(config_dir, cache_dir, os_home.as_ref()),
    })
}

/// Why an unresolved OS home cannot be reached below.
const RESOLVED: &str = "the OS home is resolved whenever a root needs it";

/// Apply the home-based last resort to whatever the options, `BATFILES_*`
/// variables, and XDG bases did not supply.
///
/// `os_home` must be `Some` wherever a root is `None`, which each caller
/// establishes before calling.
fn state_roots(
    config_dir: Option<PathBuf>,
    cache_dir: Option<PathBuf>,
    os_home: Option<&PathBuf>,
) -> StateRoots {
    // Config and cache are batfiles' own bookkeeping, so their home fallbacks use
    // the OS home, never the selected home.
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
        .or_else(|| env.location("BATFILES_CONFIG_DIR"))
        .or_else(|| xdg_base(env, "XDG_CONFIG_HOME"))
}

/// The cache root an invocation supplied directly, if any.
fn cache_option(cli: &LocationInputs, env: &Environment) -> Option<PathBuf> {
    cli.cache_dir
        .clone()
        .or_else(|| env.location("BATFILES_CACHE_DIR"))
        .or_else(|| xdg_base(env, "XDG_CACHE_HOME"))
}

/// A batfiles directory rooted at an `$XDG_*` base, or `None` when the variable
/// is absent or empty. Whitespace is significant, as it is for the location
/// variables.
fn xdg_base(env: &Environment, key: &str) -> Option<PathBuf> {
    env.location(key).map(|base| base.join("batfiles"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in OS home, so a test that reaches the last-resort fallback has a
    /// deterministic value to assert.
    fn os_home() -> Result<PathBuf, Error> {
        Ok(PathBuf::from("/os-home"))
    }

    /// Stand in for a home-less environment: any attempt to resolve the OS home
    /// fails, so a test that reaches this has wrongly required the fallback.
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
        assert_eq!(roots.batfiles_dir, PathBuf::from("/os-home/dotfiles"));
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
        assert_eq!(roots.batfiles_dir, PathBuf::from("/env-repo"));
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
        assert_eq!(roots.batfiles_dir, PathBuf::from("/opt-repo"));
    }

    #[test]
    fn the_leaf_repository_default_follows_the_selected_home() {
        let cli = LocationInputs {
            home_dir: Some(PathBuf::from("/alt-home")),
            ..LocationInputs::default()
        };
        let roots = resolve(cli, &empty_env());
        assert_eq!(roots.batfiles_dir, PathBuf::from("/alt-home/dotfiles"));
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

        assert_eq!(roots.batfiles_dir, PathBuf::from("/working-repo"));
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

        assert_eq!(roots.batfiles_dir, PathBuf::from("/env-repo"));
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

        assert_eq!(roots.batfiles_dir, PathBuf::from("/option-repo"));
    }

    // The state roots alone. State-only resolution takes no discovery closure
    // and returns no repository, so neither needs a test.

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
        // A machine batfiles cannot find a home on can still edit its own state,
        // because no state root is anchored to the home a destination would use.
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
        // The whole point of the OS-home split: `--home-dir` relocates the leaf
        // repository but leaves batfiles' own state on the OS home.
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
        // `$XDG_*` bases outrank the `~/.config`/`~/.cache` fallbacks and gain a
        // `batfiles` leaf.
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
        assert_eq!(roots.batfiles_dir, PathBuf::from("/os-home/dotfiles"));
    }

    #[test]
    fn supplying_every_root_never_resolves_the_os_home() {
        // Explicit roots make the OS home irrelevant, so resolution must succeed
        // even where no home can be found.
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
        // `--home-dir` plus both `$XDG_*` bases cover every root, so no OS home
        // is needed even though config and cache were not set explicitly.
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
        assert_eq!(roots.batfiles_dir, PathBuf::from("/home/dotfiles"));
        assert_eq!(
            roots.state.config_dir,
            PathBuf::from("/xdg/config/batfiles")
        );
        assert_eq!(roots.state.cache_dir, PathBuf::from("/xdg/cache/batfiles"));
    }

    #[test]
    fn a_base_left_uncovered_still_requires_the_os_home() {
        // Config is covered by XDG, but cache has neither an override nor an XDG
        // base, so the OS home is still needed and its failure is fatal.
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
