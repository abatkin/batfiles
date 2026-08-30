//! The process environment, captured once and decoded to UTF-8.
//!
//! Batfiles reads the environment a single time, when the CLI starts. This
//! module owns that snapshot and the accessors that turn it into the specific
//! inputs the rest of the program merges: a location root, a run-only skip
//! list. Everything here is plain data, so it is testable without the real
//! process environment through [`Environment::from_pairs`].

use std::collections::BTreeMap;
use std::path::PathBuf;

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
    pub fn location(&self, key: &str) -> Option<PathBuf> {
        match self.get(key) {
            None | Some("") => None,
            Some(value) => Some(PathBuf::from(value)),
        }
    }

    /// A comma-separated list variable: split on commas, trim each item, and
    /// drop the empties.
    ///
    /// Trimming is what makes `a, b` mean the same as `a,b`, and it is why an
    /// [`ItemId`](crate::item::ItemId) may not contain a comma or whitespace.
    /// Shared by the run-only skip lists and, at 8.3, the four bootstrap lists.
    pub fn list(&self, key: &str) -> Vec<String> {
        self.get(key)
            .into_iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect()
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
    fn an_empty_list_variable_names_nothing() {
        // Distinct from a location, where empty means unset: here it means an
        // explicitly empty list, and both come out the same way.
        let env = Environment::from_pairs([("BATFILES_SKIP_GROUPS", " , ")]);
        assert!(env.list("BATFILES_SKIP_GROUPS").is_empty());
    }
}
