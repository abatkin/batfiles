//! Environment capture and location-root resolution.
//!
//! The process environment is read once, here, and turned into the four
//! directories a command works in. Each root follows `option > BATFILES_* >
//! default`; the home and the leaf repository track the *selected* home, while
//! the config and cache directories hold batfiles' own machine-local state and
//! default off the invoking user's OS home, so `--home-dir` does not move them.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::error::Error;
use crate::repo::BatfilesConfig;

/// The captured process environment as a decoded `String` map.
///
/// Names and values are already lossily decoded, so no `OsString` travels
/// further. The map is ordered, which keeps iteration — and therefore tests —
/// deterministic.
#[derive(Debug)]
pub(crate) struct Environment {
    entries: BTreeMap<String, String>,
}

impl Environment {
    /// Capture the process environment exactly once, lossily decoding each name
    /// and value to `String`.
    ///
    /// Uses [`std::env::vars_os`] rather than [`std::env::vars`], which panics
    /// on non-UTF-8 data; lossy decoding is the accepted behavior for the
    /// vanishingly rare non-UTF-8 environment.
    ///
    /// On Windows every key is ASCII-uppercased — the sole case-normalization
    /// point — so lookups are deterministic on a case-insensitive environment.
    /// Values are never folded.
    pub fn capture() -> Self {
        let entries = std::env::vars_os()
            .map(|(key, value)| {
                let key = key.to_string_lossy().into_owned();
                #[cfg(windows)]
                let key = key.to_ascii_uppercase();
                (key, value.to_string_lossy().into_owned())
            })
            .collect();
        Self { entries }
    }

    /// Build an environment from explicit pairs, for tests.
    ///
    /// Keys are stored verbatim; the `#[cfg(windows)]` folding lives only in
    /// [`Environment::capture`], so pass already-uppercased keys to exercise the
    /// Windows behavior rather than branching in tests.
    #[cfg(test)]
    pub fn from_pairs<I, K, V>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            entries: pairs
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        }
    }

    /// Raw lookup of a single variable's value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// A location variable's value as a path.
    ///
    /// An absent or empty variable is treated as unset. The value is not
    /// trimmed: whitespace is part of the path.
    fn location(&self, key: &str) -> Option<PathBuf> {
        match self.get(key) {
            None | Some("") => None,
            Some(value) => Some(PathBuf::from(value)),
        }
    }
}

/// The four location options, as parsed from the command line.
///
/// Named rather than passed as four bare `Option<PathBuf>` arguments, which are
/// indistinguishable at a call site and trivial to transpose.
#[derive(Debug, Default)]
pub(crate) struct LocationInputs {
    pub batfiles_dir: Option<PathBuf>,
    pub home_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub cache_dir: Option<PathBuf>,
}

/// The four resolved root directories a command may need.
#[derive(Debug)]
pub(crate) struct Roots {
    /// The destination home: the base for home-relative destinations.
    pub home: PathBuf,
    /// The leaf repository.
    pub batfiles_dir: PathBuf,
    /// The directory holding `vars.toml` and `disabled.toml`.
    pub config_dir: PathBuf,
    /// The directory holding the disposable `dynamic-vars.toml` cache.
    pub cache_dir: PathBuf,
}

/// The documents each root contains.
///
/// Pairing a file with the root it lives under is location policy — the same
/// policy the fields above resolve — so it stays next to them rather than with
/// the type that parses the file.
impl Roots {
    /// The leaf repository's manifest. A remote's manifest is not here: it lives
    /// in that remote's materialization rather than under a resolved root.
    pub fn batfiles_config(&self) -> PathBuf {
        self.batfiles_dir.join(BatfilesConfig::FILE_NAME)
    }
}

/// Detect the invoking user's OS home.
///
/// Resolution consults this only as a last resort, when the home is unset and
/// no `$XDG_*` base already covers config or cache. Failure is fatal.
pub(crate) fn detect_os_home() -> Result<PathBuf, Error> {
    std::env::home_dir().ok_or(Error::HomeUnavailable)
}

/// Resolve the four roots from the CLI options and the captured environment.
///
/// `os_home` supplies the OS home directory as the final fallback. It is called
/// at most once, and only when a root still needs it: when the home is unset, or
/// when config or cache must fall back to `~/.config`/`~/.cache` because neither
/// their `BATFILES_*` variable nor the matching `$XDG_*` base is set. A command
/// handed every root it needs therefore never looks up a home and works even
/// where none can be determined. Injecting `os_home` keeps this a pure function
/// that tests exercise without the real environment.
pub(crate) fn resolve_roots(
    cli: &LocationInputs,
    env: &Environment,
    os_home: impl FnOnce() -> Result<PathBuf, Error>,
) -> Result<Roots, Error> {
    // What the options, BATFILES_* variables, and XDG bases supply directly,
    // before the home-based last resort.
    let home = cli
        .home_dir
        .clone()
        .or_else(|| env.location("BATFILES_HOME"));
    let batfiles_dir = cli
        .batfiles_dir
        .clone()
        .or_else(|| env.location("BATFILES_DIR"));
    let config_dir = cli
        .config_dir
        .clone()
        .or_else(|| env.location("BATFILES_CONFIG_DIR"))
        .or_else(|| xdg_base(env, "XDG_CONFIG_HOME"));
    let cache_dir = cli
        .cache_dir
        .clone()
        .or_else(|| env.location("BATFILES_CACHE_DIR"))
        .or_else(|| xdg_base(env, "XDG_CACHE_HOME"));

    // The OS home is the final fallback, so resolve it once and only when a root
    // still lacks a value. `batfiles_dir` defaults to `<selected-home>/dotfiles`,
    // which needs the OS home only when the home itself is unresolved.
    let os_home = if home.is_none() || config_dir.is_none() || cache_dir.is_none() {
        Some(os_home()?)
    } else {
        None
    };
    // Each branch below reads `os_home` only where its value was `None`, which is
    // exactly when `os_home` was resolved above.
    const RESOLVED: &str = "the OS home is resolved whenever a root needs it";

    // The selected home anchors the leaf-repository default.
    let home = home.unwrap_or_else(|| os_home.as_ref().expect(RESOLVED).clone());
    let batfiles_dir = batfiles_dir.unwrap_or_else(|| home.join("dotfiles"));
    // Config and cache are batfiles' own bookkeeping, so their home fallbacks use
    // the OS home, never the selected home.
    let config_dir = config_dir.unwrap_or_else(|| {
        os_home
            .as_ref()
            .expect(RESOLVED)
            .join(".config")
            .join("batfiles")
    });
    let cache_dir = cache_dir.unwrap_or_else(|| {
        os_home
            .as_ref()
            .expect(RESOLVED)
            .join(".cache")
            .join("batfiles")
    });

    Ok(Roots {
        home,
        batfiles_dir,
        config_dir,
        cache_dir,
    })
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
        resolve_roots(&cli, env, os_home).expect("resolution should succeed")
    }

    fn empty_env() -> Environment {
        Environment::from_pairs([] as [(&str, &str); 0])
    }

    #[test]
    fn get_reads_values_verbatim() {
        let env = Environment::from_pairs([("BATFILES_DIR", "/repo")]);
        assert_eq!(env.get("BATFILES_DIR"), Some("/repo"));
        assert_eq!(env.get("MISSING"), None);
    }

    #[test]
    fn a_location_treats_absent_and_empty_alike() {
        let env = Environment::from_pairs([("BATFILES_HOME", "")]);
        assert_eq!(env.location("BATFILES_HOME"), None);
        assert_eq!(env.location("MISSING"), None);
    }

    #[test]
    fn a_location_keeps_surrounding_whitespace() {
        let env = Environment::from_pairs([("BATFILES_DIR", "  /has space  ")]);
        assert_eq!(
            env.location("BATFILES_DIR"),
            Some(PathBuf::from("  /has space  "))
        );
    }

    #[test]
    fn defaults_come_from_the_os_home() {
        let roots = resolve(LocationInputs::default(), &empty_env());
        assert_eq!(roots.home, PathBuf::from("/os-home"));
        assert_eq!(roots.batfiles_dir, PathBuf::from("/os-home/dotfiles"));
        assert_eq!(roots.config_dir, PathBuf::from("/os-home/.config/batfiles"));
        assert_eq!(roots.cache_dir, PathBuf::from("/os-home/.cache/batfiles"));
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
        assert_eq!(roots.config_dir, PathBuf::from("/env-config"));
        assert_eq!(roots.cache_dir, PathBuf::from("/env-cache"));
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
    fn selecting_a_home_does_not_move_config_or_cache() {
        // The whole point of the OS-home split: `--home-dir` relocates the leaf
        // repository but leaves batfiles' own state on the OS home.
        let cli = LocationInputs {
            home_dir: Some(PathBuf::from("/alt-home")),
            ..LocationInputs::default()
        };
        let env = Environment::from_pairs([("BATFILES_HOME", "/env-home")]);
        let roots = resolve(cli, &env);
        assert_eq!(roots.config_dir, PathBuf::from("/os-home/.config/batfiles"));
        assert_eq!(roots.cache_dir, PathBuf::from("/os-home/.cache/batfiles"));
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
        assert_eq!(roots.config_dir, PathBuf::from("/xdg/config/batfiles"));
        assert_eq!(roots.cache_dir, PathBuf::from("/xdg/cache/batfiles"));
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
        let roots = resolve_roots(&cli, &empty_env(), unavailable)
            .expect("explicit roots should not need the OS home");
        assert_eq!(roots.config_dir, PathBuf::from("/config"));
        assert_eq!(roots.cache_dir, PathBuf::from("/cache"));
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
        let roots = resolve_roots(&cli, &env, unavailable)
            .expect("XDG bases should remove the need for an OS home");
        assert_eq!(roots.home, PathBuf::from("/home"));
        assert_eq!(roots.batfiles_dir, PathBuf::from("/home/dotfiles"));
        assert_eq!(roots.config_dir, PathBuf::from("/xdg/config/batfiles"));
        assert_eq!(roots.cache_dir, PathBuf::from("/xdg/cache/batfiles"));
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
            resolve_roots(&cli, &env, unavailable),
            Err(Error::HomeUnavailable)
        ));
    }

    #[test]
    fn a_missing_home_requires_the_os_home() {
        assert!(matches!(
            resolve_roots(&LocationInputs::default(), &empty_env(), unavailable),
            Err(Error::HomeUnavailable)
        ));
    }
}
