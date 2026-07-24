//! The process environment, captured once and decoded to UTF-8.
//!
//! Per `docs/environment.md`, batfiles reads the environment a single time when
//! the CLI starts. This module owns that captured snapshot and the typed
//! accessors that turn it into the specific inputs the rest of the
//! configuration layer merges. Everything here is plain data, so it is testable
//! without the real process environment via [`Environment::from_pairs`].

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The keys that name one-shot user variables (`BATFILES_VAR_<NAME>`).
const VAR_PREFIX: &str = "BATFILES_VAR_";

/// The captured process environment as a decoded `String` map.
///
/// Names and values are already lossily decoded, so no `OsString` leaves the
/// CLI. The map is ordered, which keeps iteration (and therefore tests)
/// deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    entries: BTreeMap<String, String>,
}

impl Environment {
    /// Capture the process environment exactly once, lossily decoding each name
    /// and value to `String`.
    ///
    /// Uses [`std::env::vars_os`] rather than [`std::env::vars`], which panics
    /// on non-UTF-8 data; lossy decoding (bad bytes become U+FFFD) is the
    /// accepted behavior for the vanishingly rare non-UTF-8 environment.
    ///
    /// On Windows every key is ASCII-uppercased — the sole case-normalization
    /// point (`docs/environment.md`) — so batfiles' fixed-uppercase lookups and
    /// the `env.*` namespace are deterministic on a case-insensitive
    /// environment. Environment names are ASCII in practice, and ASCII folding
    /// avoids locale surprises. Values are never folded.
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
    pub fn location(&self, key: &str) -> Option<PathBuf> {
        match self.get(key) {
            None | Some("") => None,
            Some(value) => Some(PathBuf::from(value)),
        }
    }

    /// The one-shot user-variable candidates: every `BATFILES_VAR_<NAME>` key
    /// with a non-empty suffix, yielding `(name, value)`.
    ///
    /// The name is the suffix verbatim (case-sensitive on Unix; already
    /// uppercased on Windows). A bare `BATFILES_VAR_` is ignored; an empty value
    /// is kept, as it is significant. Name *validity* is deferred to the merge
    /// step so a rejection can name the offending variable.
    pub fn one_shot_vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().filter_map(|(key, value)| {
            let name = key.strip_prefix(VAR_PREFIX)?;
            (!name.is_empty()).then_some((name, value.as_str()))
        })
    }

    /// A comma-separated list variable: split on commas, trim each item, and
    /// drop the empties. Shared by the run-only skip lists and the four
    /// bootstrap lists.
    pub fn list(&self, key: &str) -> Vec<String> {
        self.get(key)
            .into_iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect()
    }

    /// The whole captured map, handed to core's condition evaluation as the
    /// read-only `env.*` namespace. It does not participate in user-variable
    /// precedence.
    pub fn entries(&self) -> &BTreeMap<String, String> {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn one_shot_vars_collects_the_prefixed_keys() {
        let env = Environment::from_pairs([
            ("BATFILES_VAR_EDITOR", "vim"),
            ("BATFILES_VAR_PROFILE", ""),
            ("EDITOR", "emacs"),
        ]);
        let mut vars: Vec<_> = env.one_shot_vars().collect();
        vars.sort();
        assert_eq!(vars, vec![("EDITOR", "vim"), ("PROFILE", "")]);
    }

    #[test]
    fn a_bare_var_prefix_is_ignored() {
        let env = Environment::from_pairs([("BATFILES_VAR_", "orphan")]);
        assert_eq!(env.one_shot_vars().count(), 0);
    }

    #[test]
    fn a_var_suffix_is_case_sensitive_in_the_map() {
        let env = Environment::from_pairs([("BATFILES_VAR_editor", "vim")]);
        assert_eq!(
            env.one_shot_vars().collect::<Vec<_>>(),
            vec![("editor", "vim")]
        );
    }

    #[test]
    fn a_list_trims_items_and_drops_empties() {
        let env = Environment::from_pairs([("BATFILES_SKIP_ACTIONS", " a , ,b,  , c ")]);
        assert_eq!(env.list("BATFILES_SKIP_ACTIONS"), vec!["a", "b", "c"]);
    }

    #[test]
    fn an_absent_list_is_empty() {
        let env = Environment::from_pairs([] as [(&str, &str); 0]);
        assert!(env.list("BATFILES_SKIP_GROUPS").is_empty());
    }

    #[test]
    fn entries_exposes_the_whole_map() {
        let env = Environment::from_pairs([("PATH", "/usr/bin"), ("HOME", "/home/me")]);
        assert_eq!(env.entries().len(), 2);
        assert_eq!(env.entries().get("PATH"), Some(&"/usr/bin".to_owned()));
    }
}
