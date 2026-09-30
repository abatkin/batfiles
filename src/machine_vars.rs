//! Read and edit machine-local `vars.toml`. Ignore repository values and environment/CLI
//! overrides. Mutation reports include keys only; `vars get` prints the requested stored value.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::location::StateRoots;
use crate::output::Reporter;
use crate::tomlfile;
use crate::var::VarName;

/// Machine-local variable values parsed from `vars.toml`.
#[derive(Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct MachineVars {
    pub values: BTreeMap<VarName, String>,
}

impl MachineVars {
    /// The document's file name; [`StateRoots`] decides its directory.
    pub const FILE_NAME: &'static str = "vars.toml";

    /// Load the document, treating a missing file as no values.
    pub fn load(path: &Path) -> Result<Self, Error> {
        tomlfile::read_or_default(path)
    }

    /// Rewrite the document. An empty map writes an empty file.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        tomlfile::write(path, self)
    }
}

/// Store `value` verbatim under `key`, replacing any previous machine-local value. Empty values
/// are allowed.
pub(crate) fn set(
    key: &str,
    value: &str,
    roots: &StateRoots,
    reporter: &Reporter,
) -> Result<(), Error> {
    // Reject invalid keys before touching state.
    let key = parse(key)?;

    let path = roots.machine_vars_path();
    let mut vars = MachineVars::load(&path)?;

    let outcome = store(&mut vars.values, &key, value);
    if outcome.requires_save() {
        vars.save(&path)?;
    }

    reporter.info(&describe(&key, outcome));
    Ok(())
}

/// Print the machine-local value of `key`, or fail because it has none.
pub(crate) fn get(key: &str, roots: &StateRoots, reporter: &Reporter) -> Result<(), Error> {
    let key = parse(key)?;
    let vars = MachineVars::load(&roots.machine_vars_path())?;

    // A missing key must remain distinct from a stored empty string.
    match vars.values.get(&key) {
        Some(value) => {
            reporter.data(value);
            Ok(())
        }
        None => Err(Error::VarNotSet { key }),
    }
}

/// Remove the machine-local value of `key`, if it has one.
pub(crate) fn unset(key: &str, roots: &StateRoots, reporter: &Reporter) -> Result<(), Error> {
    let key = parse(key)?;

    let path = roots.machine_vars_path();
    let mut vars = MachineVars::load(&path)?;

    let outcome = remove(&mut vars.values, &key);
    if outcome.requires_save() {
        vars.save(&path)?;
    }

    reporter.info(&describe(&key, outcome));
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

    fn parse_document(document: &str) -> Result<MachineVars, toml::de::Error> {
        toml::from_str(document)
    }

    #[test]
    fn the_document_is_a_flat_map_of_strings() {
        let vars =
            parse_document("editor = 'nvim'\nprofile = 'work'\nwork = 'true'\n").expect("parse");
        assert_eq!(
            vars.values,
            values([("editor", "nvim"), ("profile", "work"), ("work", "true")])
        );
    }

    #[test]
    fn an_empty_document_is_valid() {
        assert_eq!(parse_document("").expect("empty"), MachineVars::default());
    }

    #[test]
    fn a_value_is_a_string_and_is_never_inferred() {
        let error = parse_document("work = true\n").expect_err("booleans are not values");
        assert!(
            error.to_string().contains("invalid type: boolean"),
            "{error}"
        );
    }

    #[test]
    fn a_key_must_be_a_valid_variable_name() {
        let error = parse_document("has-dash = 'x'\n").expect_err("dashes are not names");
        assert!(
            error.to_string().contains("a variable name must"),
            "{error}"
        );

        let error = parse_document("env = 'x'\n").expect_err("reserved");
        assert!(error.to_string().contains("reserved"), "{error}");
    }

    /// Return a `vars.toml` path under the temporary directory.
    fn path(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join(MachineVars::FILE_NAME)
    }

    #[test]
    fn a_missing_file_holds_no_values() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            MachineVars::load(&path(&dir)).expect("absent is empty"),
            MachineVars::default()
        );
    }

    #[test]
    fn saving_and_loading_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let vars = parse_document("editor = 'nvim'\nprofile = 'work'\n").expect("parse");

        vars.save(&path(&dir)).expect("save");
        assert_eq!(MachineVars::load(&path(&dir)).expect("load"), vars);
    }

    #[test]
    fn removing_the_last_key_leaves_an_empty_document_behind() {
        let dir = tempfile::tempdir().expect("temp dir");
        parse_document("editor = 'nvim'\n")
            .expect("parse")
            .save(&path(&dir))
            .expect("save");

        MachineVars::default()
            .save(&path(&dir))
            .expect("save empty");

        assert!(path(&dir).is_file(), "the file should survive");
        assert_eq!(
            MachineVars::load(&path(&dir)).expect("load"),
            MachineVars::default()
        );
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
