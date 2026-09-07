//! The process environment, captured once and decoded to UTF-8.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The captured process environment as a decoded `String` map.
#[derive(Debug)]
pub(crate) struct Environment {
    entries: BTreeMap<String, String>,
}

impl Environment {
    /// Capture the process environment exactly once, lossily decoding each name
    /// and value to `String`.
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
    pub fn location(&self, key: &str) -> Option<PathBuf> {
        match self.get(key) {
            None | Some("") => None,
            Some(value) => Some(PathBuf::from(value)),
        }
    }

    /// The keys that name one-shot user variables (`BATFILES_VAR_<NAME>`).
    ///
    /// Public because a diagnostic about a rejected suffix has to name the whole
    /// environment variable, which is what the user goes and deletes.
    pub const VAR_PREFIX: &'static str = "BATFILES_VAR_";

    /// The one-shot user-variable candidates: every `BATFILES_VAR_<NAME>` key
    /// with a non-empty suffix, yielding `(name, value)`.
    ///
    /// The name is the suffix verbatim — case-sensitive on Unix, already
    /// uppercased on Windows by [`Environment::capture`]. A bare
    /// `BATFILES_VAR_` names nothing and is left out; an empty value is kept,
    /// because it is a value. Whether a suffix is a *usable* name is
    /// [`crate::env_vars`]' question, not this module's.
    pub fn one_shot_vars(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().filter_map(|(key, value)| {
            let name = key.strip_prefix(Self::VAR_PREFIX)?;
            (!name.is_empty()).then_some((name, value.as_str()))
        })
    }

    /// A comma-separated list variable: split on commas, trim each item, and
    /// drop the empties.
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
    fn one_shot_vars_collects_the_prefixed_keys() {
        let env = Environment::from_pairs([
            ("BATFILES_VAR_EDITOR", "vim"),
            ("BATFILES_VAR_PROFILE", ""),
            ("EDITOR", "emacs"),
        ]);
        assert_eq!(
            env.one_shot_vars().collect::<Vec<_>>(),
            // The empty value is carried through: it is what `PROFILE` is set
            // to, not a sign that nothing set it.
            vec![("EDITOR", "vim"), ("PROFILE", "")]
        );
    }

    #[test]
    fn a_bare_var_prefix_names_nothing() {
        let env = Environment::from_pairs([("BATFILES_VAR_", "orphan")]);
        assert_eq!(env.one_shot_vars().count(), 0);
    }

    #[test]
    fn a_var_suffix_is_taken_verbatim() {
        // Including a suffix no name rule would accept: what is *usable* is
        // decided in `env_vars`, where a rejection can warn about it.
        let env = Environment::from_pairs([("BATFILES_VAR_1up", "x")]);
        assert_eq!(env.one_shot_vars().collect::<Vec<_>>(), vec![("1up", "x")]);
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
