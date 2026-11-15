//! The `vars` commands. `set`, `get`, and `unset` edit machine-local `vars.toml`,
//! ignoring repository values and environment/CLI overrides; mutation reports
//! include keys only, and `vars get` prints the requested stored value. `list`
//! shows effective values; [`refresh`] reruns dynamic variables' commands.

pub(crate) mod refresh;

use std::collections::BTreeMap;

use crate::commands::Session;
use crate::env::Environment;
use crate::error::Error;
use crate::location::{Roots, StateRoots};
use crate::manifest::Manifest;
use crate::output::Reporter;
use crate::run::lock::RunLock;
use crate::vars::dynamic::{CachePolicy, DynamicVarResolver};
use crate::vars::machine::MachineVars;
use crate::vars::name::VarName;
use crate::vars::scope::VarScope;

/// Store `value` verbatim under `key`, replacing any previous machine-local value, under the run
/// lock. Empty values are allowed.
pub(crate) fn set(key: &str, value: &str, session: &Session<'_>) -> Result<(), Error> {
    let (roots, _lock) = session.locked_state_roots()?;
    // Reject invalid keys before touching state.
    let key = parse(key)?;

    let path = roots.machine_vars_path();
    let mut vars = MachineVars::load(&path)?;

    let outcome = store(&mut vars.values, &key, value);
    if outcome.requires_save() {
        vars.save(&path)?;
    }

    session.reporter.info(&describe(&key, outcome));
    Ok(())
}

/// Print the machine-local value of `key`, or fail because it has none. Takes no lock: it only
/// reads.
pub(crate) fn get(key: &str, session: &Session<'_>) -> Result<(), Error> {
    let roots = session.state_roots()?;
    let key = parse(key)?;
    let vars = MachineVars::load(&roots.machine_vars_path())?;

    // A missing key must remain distinct from a stored empty string.
    match vars.values.get(&key) {
        Some(value) => {
            session.reporter.data(value);
            Ok(())
        }
        None => Err(Error::VarNotSet { key }),
    }
}

/// Remove the machine-local value of `key`, if it has one, under the run lock.
pub(crate) fn unset(key: &str, session: &Session<'_>) -> Result<(), Error> {
    let (roots, _lock) = session.locked_state_roots()?;
    let key = parse(key)?;

    let path = roots.machine_vars_path();
    let mut vars = MachineVars::load(&path)?;

    let outcome = remove(&mut vars.values, &key);
    if outcome.requires_save() {
        vars.save(&path)?;
    }

    session.reporter.info(&describe(&key, outcome));
    Ok(())
}

/// Validate a key from the command line, keeping the text the user wrote for
/// the diagnostic.
fn parse(key: &str) -> Result<VarName, Error> {
    VarName::try_from(key.to_owned()).map_err(|source| Error::InvalidVarName {
        key: key.to_owned(),
        source,
    })
}

/// What one mutation did to the machine-local map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// The key had no value and now has one.
    Added,
    /// The key held a different value.
    Replaced,
    /// The key already held exactly this value.
    Unchanged,
    /// The key was present and is now gone.
    Removed,
    /// There was nothing to remove.
    Absent,
}

impl Outcome {
    /// Return whether the mutation requires saving the document.
    fn requires_save(self) -> bool {
        matches!(self, Self::Added | Self::Replaced | Self::Removed)
    }
}

/// Store a value and return whether it was added, replaced, or unchanged.
fn store(values: &mut BTreeMap<VarName, String>, key: &VarName, value: &str) -> Outcome {
    match values.get(key) {
        Some(existing) if existing == value => Outcome::Unchanged,
        existing => {
            let outcome = if existing.is_some() {
                Outcome::Replaced
            } else {
                Outcome::Added
            };
            values.insert(key.clone(), value.to_owned());
            outcome
        }
    }
}

/// Remove a key and return whether it was present.
fn remove(values: &mut BTreeMap<VarName, String>, key: &VarName) -> Outcome {
    if values.remove(key).is_some() {
        Outcome::Removed
    } else {
        Outcome::Absent
    }
}

/// Format a mutation result, naming the key but never its value.
fn describe(key: &VarName, outcome: Outcome) -> String {
    match outcome {
        Outcome::Added => format!("set `{key}`"),
        Outcome::Replaced => format!("changed `{key}` (it had a different value)"),
        Outcome::Unchanged => format!("`{key}` was already set to that value"),
        Outcome::Removed => format!("unset `{key}`"),
        Outcome::Absent => format!("`{key}` was not set"),
    }
}

/// `vars list`. `machine_only` lists persisted machine variables without a repository.
/// Otherwise list the leaf's variables, refreshing the dynamic cache under the run lock unless
/// `no_refresh` forbids running commands and writing the cache.
pub(crate) fn list(
    machine_only: bool,
    no_refresh: bool,
    session: &Session<'_>,
) -> Result<(), Error> {
    if machine_only {
        return list_machine(&session.state_roots()?, session.reporter);
    }
    let roots = session.repository_roots()?;
    // Only a listing that may refresh the dynamic cache writes state.
    let (policy, _lock) = if no_refresh {
        (CachePolicy::Never, None)
    } else {
        let lock = RunLock::acquire(&roots.state.cache_dir)?;
        (CachePolicy::Auto, Some(lock))
    };
    list_leaf(&roots, session.env, policy, session.reporter)
}

/// List leaf-repository variables without CLI overrides. Resolve dynamic declarations using
/// `policy`, skip those shadowed by machine-local values, and save captured values to the
/// cache.
fn list_leaf(
    roots: &Roots,
    env: &Environment,
    policy: CachePolicy,
    reporter: &Reporter,
) -> Result<(), Error> {
    let manifest = Manifest::load(&roots.manifest_path())?;
    let mut dynamic = DynamicVarResolver::for_listing(&roots.state, policy, reporter);
    let scope = VarScope::resolve(
        &manifest.vars,
        &roots.batfiles_repo,
        &roots.state,
        env,
        &[],
        &mut dynamic,
        reporter,
    )?;
    dynamic.save();
    scope.list(reporter);
    Ok(())
}

/// List only persisted machine variables; do not read the repository or environment.
fn list_machine(state: &StateRoots, reporter: &Reporter) -> Result<(), Error> {
    let machine = MachineVars::load(&state.machine_vars_path())?;
    VarScope::stack(BTreeMap::new(), machine.values, BTreeMap::new(), &[]).list(reporter);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(name: &str) -> VarName {
        VarName::try_from(name.to_owned()).expect("valid name")
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
        assert_eq!(store(&mut vars, &name("editor"), "nvim"), Outcome::Added);
        assert_eq!(vars, values([("editor", "nvim")]));
    }

    #[test]
    fn setting_a_different_value_replaces_it() {
        let mut vars = values([("editor", "nvim")]);
        assert_eq!(
            store(&mut vars, &name("editor"), "emacs"),
            Outcome::Replaced
        );
        assert_eq!(vars, values([("editor", "emacs")]));
    }

    #[test]
    fn setting_the_value_already_there_does_not_move_the_map() {
        let mut vars = values([("editor", "nvim")]);
        assert_eq!(
            store(&mut vars, &name("editor"), "nvim"),
            Outcome::Unchanged
        );
        assert_eq!(vars, values([("editor", "nvim")]));
    }

    #[test]
    fn the_empty_string_is_a_value_like_any_other() {
        let mut vars = BTreeMap::new();
        assert_eq!(store(&mut vars, &name("editor"), ""), Outcome::Added);
        assert_eq!(vars, values([("editor", "")]));
        assert_eq!(store(&mut vars, &name("editor"), ""), Outcome::Unchanged);
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
        assert!(Outcome::Added.requires_save());
        assert!(Outcome::Replaced.requires_save());
        assert!(Outcome::Removed.requires_save());
        assert!(!Outcome::Unchanged.requires_save());
        assert!(!Outcome::Absent.requires_save());
    }

    #[test]
    fn every_outcome_says_whether_the_state_moved() {
        let key = name("editor");
        let line = |outcome| describe(&key, outcome);
        assert_eq!(line(Outcome::Added), "set `editor`");
        assert_eq!(
            line(Outcome::Replaced),
            "changed `editor` (it had a different value)"
        );
        assert_eq!(
            line(Outcome::Unchanged),
            "`editor` was already set to that value"
        );
        assert_eq!(line(Outcome::Removed), "unset `editor`");
        assert_eq!(line(Outcome::Absent), "`editor` was not set");
    }

    #[test]
    fn no_outcome_line_echoes_the_value() {
        // Mutation diagnostics must not disclose stored values.
        let key = name("editor");
        for outcome in [
            Outcome::Added,
            Outcome::Replaced,
            Outcome::Unchanged,
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
    fn a_key_with_no_value_names_itself() {
        assert_eq!(
            Error::VarNotSet {
                key: name("editor")
            }
            .to_string(),
            "`editor` has no machine-local value"
        );
    }
}
