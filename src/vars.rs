//! The machine-local variable commands: `vars set`, `vars get`, and
//! `vars unset`.
//!
//! Three commands rather than one parameterized entry point, because they do
//! not share a shape: one writes, one reads, one deletes.
//!
//! They read and write `vars.toml` and nothing else. They do not load the leaf
//! repository, evaluate dynamic variables, read the cache, apply precedence, or
//! consult `BATFILES_VAR_*`, so `vars get` answers with the persisted string or
//! with nothing at all.
//!
//! A mutation's outcome line **names the key and never the value**. This is a
//! deliberate divergence from [`crate::toggle`], whose lines quote the address
//! because the address *is* the state. A variable's value is user data — it may
//! be a token, or a path that identifies a machine — and an informational line
//! puts it in terminal scrollback and in the logs of whatever wrapper script
//! called batfiles. `vars get` is the sanctioned way to read a value back, and
//! it prints to standard output where a caller asked for it.

use std::collections::BTreeMap;
use std::fmt;

use crate::config::Roots;
use crate::output::Reporter;
use crate::state::MachineVars;
use crate::tomlfile;
use crate::var::{VarName, VarNameError};

/// Store `value` under `key`, replacing any previous machine-local value.
///
/// The value is stored verbatim, the empty string included: values are opaque
/// strings, so there is no separate value rule to check.
pub(crate) fn set(key: &str, value: &str, roots: &Roots, reporter: &Reporter) -> Result<(), Error> {
    // The key is validated before the document is touched, so a bad name reads
    // and writes nothing.
    let key = parse(key)?;

    let path = roots.machine_vars();
    let mut vars = MachineVars::load(&path)?;

    let outcome = store(&mut vars.values, &key, value);
    reporter.info(&describe(&key, outcome));

    if outcome.changed() {
        vars.save(&path)?;
    }
    Ok(())
}

/// Print the machine-local value of `key`, or fail because it has none.
pub(crate) fn get(key: &str, roots: &Roots, reporter: &Reporter) -> Result<(), Error> {
    let key = parse(key)?;
    let vars = MachineVars::load(&roots.machine_vars())?;

    // An absent key fails rather than printing an empty line: the empty string
    // is a value `vars set` accepts, and the two must stay distinguishable.
    match vars.values.get(&key) {
        Some(value) => {
            reporter.data(value);
            Ok(())
        }
        None => Err(Error::Missing(key)),
    }
}

/// Remove the machine-local value of `key`, if it has one.
pub(crate) fn unset(key: &str, roots: &Roots, reporter: &Reporter) -> Result<(), Error> {
    let key = parse(key)?;

    let path = roots.machine_vars();
    let mut vars = MachineVars::load(&path)?;

    let outcome = remove(&mut vars.values, &key);
    reporter.info(&describe(&key, outcome));

    // Removing an absent key changes nothing, so it does not rewrite the
    // document — or create a `vars.toml` that was not there before.
    if outcome.changed() {
        vars.save(&path)?;
    }
    Ok(())
}

/// Validate a key from the command line, keeping the text the user wrote for
/// the diagnostic.
fn parse(key: &str) -> Result<VarName, Error> {
    VarName::new(key).map_err(|source| Error::Name {
        key: key.to_owned(),
        source,
    })
}

/// What one mutation did to the machine-local map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The key had no value and now has one.
    Set,
    /// The key held a different value.
    Changed,
    /// The key already held exactly this value.
    Kept,
    /// The key was present and is now gone.
    Removed,
    /// There was nothing to remove.
    Absent,
}

impl Outcome {
    /// Whether the document has to be rewritten. This is the answer that keeps
    /// an idempotent mutation from touching the file.
    fn changed(self) -> bool {
        matches!(self, Self::Set | Self::Changed | Self::Removed)
    }
}

/// Store one value, reporting whether — and how — the map moved.
fn store(values: &mut BTreeMap<VarName, String>, key: &VarName, value: &str) -> Outcome {
    match values.get(key) {
        Some(existing) if existing == value => Outcome::Kept,
        existing => {
            let outcome = if existing.is_some() {
                Outcome::Changed
            } else {
                Outcome::Set
            };
            values.insert(key.clone(), value.to_owned());
            outcome
        }
    }
}

/// Remove one key, reporting whether the map moved.
fn remove(values: &mut BTreeMap<VarName, String>, key: &VarName) -> Outcome {
    if values.remove(key).is_some() {
        Outcome::Removed
    } else {
        Outcome::Absent
    }
}

/// The line describing what one mutation did.
///
/// Every line names the key and none of them names the value; see the module
/// documentation for why that divergence from `toggle` is deliberate.
fn describe(key: &VarName, outcome: Outcome) -> String {
    match outcome {
        Outcome::Set => format!("set `{key}`"),
        Outcome::Changed => format!("changed `{key}` (it had a different value)"),
        Outcome::Kept => format!("`{key}` was already set to that value"),
        Outcome::Removed => format!("unset `{key}`"),
        Outcome::Absent => format!("`{key}` was not set"),
    }
}

/// Why a `vars` mutation or lookup failed.
#[derive(Debug)]
pub(crate) enum Error {
    /// The key was not a valid variable name. Nothing was read or written.
    Name { key: String, source: VarNameError },
    /// The key has no machine-local value. (`vars get` only.)
    Missing(VarName),
    /// `vars.toml` could not be read or replaced.
    Document(tomlfile::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // `VarNameError` states the rule only — right for a serde key error,
            // where TOML supplies the position, but it leaves a CLI diagnostic
            // with nothing to point at. So the key is quoted here.
            Self::Name { key, source } => write!(f, "invalid variable name `{key}`: {source}"),
            Self::Missing(key) => write!(f, "`{key}` has no machine-local value"),
            // The document error already names the file.
            Self::Document(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Name { source, .. } => Some(source),
            Self::Missing(_) => None,
            Self::Document(error) => Some(error),
        }
    }
}

impl From<tomlfile::Error> for Error {
    fn from(error: tomlfile::Error) -> Self {
        Self::Document(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(name: &str) -> VarName {
        VarName::new(name).expect("valid name")
    }

    fn values<const N: usize>(pairs: [(&str, &str); N]) -> BTreeMap<VarName, String> {
        pairs
            .into_iter()
            .map(|(key, value)| (name(key), value.to_owned()))
            .collect()
    }

    #[test]
    fn setting_an_absent_key_stores_it() {
        let mut vars = BTreeMap::new();
        assert_eq!(store(&mut vars, &name("editor"), "nvim"), Outcome::Set);
        assert_eq!(vars, values([("editor", "nvim")]));
    }

    #[test]
    fn setting_a_different_value_replaces_it() {
        let mut vars = values([("editor", "nvim")]);
        assert_eq!(store(&mut vars, &name("editor"), "emacs"), Outcome::Changed);
        assert_eq!(vars, values([("editor", "emacs")]));
    }

    #[test]
    fn setting_the_value_already_there_does_not_move_the_map() {
        let mut vars = values([("editor", "nvim")]);
        assert_eq!(store(&mut vars, &name("editor"), "nvim"), Outcome::Kept);
        assert_eq!(vars, values([("editor", "nvim")]));
    }

    #[test]
    fn the_empty_string_is_a_value_like_any_other() {
        // Values are opaque strings, and `vars get`'s absent-key failure is what
        // keeps this distinguishable from having no value at all.
        let mut vars = BTreeMap::new();
        assert_eq!(store(&mut vars, &name("editor"), ""), Outcome::Set);
        assert_eq!(vars, values([("editor", "")]));
        assert_eq!(store(&mut vars, &name("editor"), ""), Outcome::Kept);
    }

    #[test]
    fn unsetting_a_present_key_removes_it() {
        let mut vars = values([("editor", "nvim"), ("profile", "work")]);
        assert_eq!(remove(&mut vars, &name("editor")), Outcome::Removed);
        assert_eq!(vars, values([("profile", "work")]));
    }

    #[test]
    fn unsetting_an_absent_key_does_not_move_the_map() {
        let mut vars = values([("profile", "work")]);
        assert_eq!(remove(&mut vars, &name("editor")), Outcome::Absent);
        assert_eq!(vars, values([("profile", "work")]));
    }

    #[test]
    fn only_a_real_change_rewrites_the_document() {
        assert!(Outcome::Set.changed());
        assert!(Outcome::Changed.changed());
        assert!(Outcome::Removed.changed());
        assert!(!Outcome::Kept.changed());
        assert!(!Outcome::Absent.changed());
    }

    #[test]
    fn every_outcome_says_whether_the_state_moved() {
        let key = name("editor");
        let line = |outcome| describe(&key, outcome);
        assert_eq!(line(Outcome::Set), "set `editor`");
        assert_eq!(
            line(Outcome::Changed),
            "changed `editor` (it had a different value)"
        );
        assert_eq!(
            line(Outcome::Kept),
            "`editor` was already set to that value"
        );
        assert_eq!(line(Outcome::Removed), "unset `editor`");
        assert_eq!(line(Outcome::Absent), "`editor` was not set");
    }

    #[test]
    fn no_outcome_line_echoes_the_value() {
        // The point of the divergence from `toggle`, and the thing a later edit
        // aiming for consistency would silently undo.
        let key = name("editor");
        for outcome in [
            Outcome::Set,
            Outcome::Changed,
            Outcome::Kept,
            Outcome::Removed,
            Outcome::Absent,
        ] {
            let line = describe(&key, outcome);
            assert!(!line.contains("s3cr3t"), "{line}");
            assert!(line.contains("`editor`"), "{line}");
        }
    }

    #[test]
    fn an_invalid_key_is_rejected_and_quoted() {
        let error = parse("1up").expect_err("a leading digit is invalid");
        let message = error.to_string();
        assert!(message.contains("`1up`"), "{message}");
        assert!(message.contains("must start with"), "{message}");
    }

    #[test]
    fn a_reserved_key_is_rejected_and_quoted() {
        let error = parse("env").expect_err("reserved");
        let message = error.to_string();
        assert!(message.contains("`env`"), "{message}");
        assert!(message.contains("reserved"), "{message}");
    }

    #[test]
    fn a_missing_value_names_the_key() {
        assert_eq!(
            Error::Missing(name("editor")).to_string(),
            "`editor` has no machine-local value"
        );
    }
}
