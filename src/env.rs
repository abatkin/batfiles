//! The process environment, captured once and decoded to UTF-8.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

/// A snapshot of the process environment, decoded to UTF-8 strings.
#[derive(Debug)]
pub(crate) struct Environment {
    entries: Rc<BTreeMap<String, String>>,
}

impl Environment {
    /// Capture the process environment exactly once, lossily decoding each name
    /// and value to `String`.
    pub fn capture() -> Self {
        let entries: BTreeMap<String, String> = std::env::vars_os()
            .map(|(key, value)| {
                let key = key.to_string_lossy().into_owned();
                #[cfg(windows)]
                let key = key.to_ascii_uppercase();
                (key, value.to_string_lossy().into_owned())
            })
            .collect();
        Self {
            entries: Rc::new(entries),
        }
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
            entries: Rc::new(
                pairs
                    .into_iter()
                    .map(|(key, value)| (key.into(), value.into()))
                    .collect(),
            ),
        }
    }

    /// Raw lookup of a single variable's value.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Return a shared handle to the captured environment map.
    pub fn entries(&self) -> Rc<BTreeMap<String, String>> {
        Rc::clone(&self.entries)
    }

    /// A location variable's value as a path.
    pub fn path_var(&self, key: &str) -> Option<PathBuf> {
        match self.get(key) {
            None | Some("") => None,
            Some(value) => Some(PathBuf::from(value)),
        }
    }

    /// Prefix for environment variables that override user variables.
    pub const VAR_PREFIX: &'static str = "BATFILES_VAR_";

    /// Yield `(name, value)` overrides from `BATFILES_VAR_<NAME>` variables with nonempty
    /// suffixes. Preserve empty values; do not validate names. Suffixes are case-sensitive on
    /// Unix and uppercased on Windows during capture.
    pub fn var_overrides(&self) -> impl Iterator<Item = (&str, &str)> {
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
        assert_eq!(env.path_var("BATFILES_HOME"), None);
        assert_eq!(env.path_var("MISSING"), None);
    }

    #[test]
    fn a_location_keeps_surrounding_whitespace() {
        let env = Environment::from_pairs([("BATFILES_DIR", "  /has space  ")]);
        assert_eq!(
            env.path_var("BATFILES_DIR"),
            Some(PathBuf::from("  /has space  "))
        );
    }

    #[test]
    fn var_overrides_collects_the_prefixed_keys() {
        let env = Environment::from_pairs([
            ("BATFILES_VAR_EDITOR", "vim"),
            ("BATFILES_VAR_PROFILE", ""),
            ("EDITOR", "emacs"),
        ]);
        assert_eq!(
            env.var_overrides().collect::<Vec<_>>(),
            vec![("EDITOR", "vim"), ("PROFILE", "")]
        );
    }

    #[test]
    fn var_overrides_ignores_a_bare_prefix() {
        let env = Environment::from_pairs([("BATFILES_VAR_", "orphan")]);
        assert_eq!(env.var_overrides().count(), 0);
    }

    #[test]
    fn var_overrides_preserves_unvalidated_suffixes() {
        let env = Environment::from_pairs([("BATFILES_VAR_1up", "x")]);
        assert_eq!(env.var_overrides().collect::<Vec<_>>(), vec![("1up", "x")]);
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
        let env = Environment::from_pairs([("BATFILES_SKIP_GROUPS", " , ")]);
        assert!(env.list("BATFILES_SKIP_GROUPS").is_empty());
    }
}
