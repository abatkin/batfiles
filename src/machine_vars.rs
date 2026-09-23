//! Read and edit machine-local `vars.toml` with `vars set`, `get`, and `unset`.
//! These commands do not read the repository or apply environment/CLI overrides.
//!
//! Mutation reports name the key, never the value: values may contain tokens or
//! identifying paths that must not leak into terminal scrollback or script logs.
//! `vars get` explicitly requests the persisted value on standard output.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::location::StateRoots;
use crate::output::Reporter;
use crate::tomlfile;
use crate::var::VarName;

/// The parsed `vars.toml`.
///
/// The map is exposed directly: the document has no other structure.
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

    /// Rewrite the document.
    ///
    /// An empty map writes an empty file: removing the last key leaves a valid
    /// `vars.toml` behind rather than deleting it.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        tomlfile::write(path, self)
    }
}

/// Store `value` under `key`, replacing any previous machine-local value.
///
/// The value is stored verbatim, the empty string included: values are opaque
/// strings, so there is no separate value rule to check.
pub(crate) fn set(
    key: &str,
    value: &str,
    roots: &StateRoots,
    reporter: &Reporter,
) -> Result<(), Error> {
    // The key is validated before the document is touched, so a bad name reads
    // and writes nothing.
    let key = parse(key)?;

    let path = roots.machine_vars();
    let mut vars = MachineVars::load(&path)?;

    let outcome = store(&mut vars.values, &key, value);
    if outcome.changed() {
        vars.save(&path)?;
    }

    reporter.info(&describe(&key, outcome));
    Ok(())
}

/// Print the machine-local value of `key`, or fail because it has none.
pub(crate) fn get(key: &str, roots: &StateRoots, reporter: &Reporter) -> Result<(), Error> {
    let key = parse(key)?;
    let vars = MachineVars::load(&roots.machine_vars())?;

    // An absent key fails rather than printing an empty line: the empty string
    // is a value `vars set` accepts, and the two must stay distinguishable.
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

    let path = roots.machine_vars();
    let mut vars = MachineVars::load(&path)?;

    // Removing an absent key changes nothing, so it does not rewrite the
    // document — or create a `vars.toml` that was not there before.
    let outcome = remove(&mut vars.values, &key);
    if outcome.changed() {
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
/// documentation for why that divergence from `disabled` is deliberate.
fn describe(key: &VarName, outcome: Outcome) -> String {
    match outcome {
        Outcome::Set => format!("set `{key}`"),
        Outcome::Changed => format!("changed `{key}` (it had a different value)"),
        Outcome::Kept => format!("`{key}` was already set to that value"),
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

    // The document.

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
        // `work = true` is the slip a TOML author makes, and it is the manifest's
        // `[vars]` rule too: every variable value is a string.
        let error = parse_document("work = true\n").expect_err("booleans are not values");
        assert!(
            error.to_string().contains("invalid type: boolean"),
            "{error}"
        );
    }

    #[test]
    fn a_key_must_be_a_valid_variable_name() {
        // A hand-edited file cannot introduce a name no manifest could declare.
        let error = parse_document("has-dash = 'x'\n").expect_err("dashes are not names");
        assert!(
            error.to_string().contains("a variable name must"),
            "{error}"
        );

        let error = parse_document("env = 'x'\n").expect_err("reserved");
        assert!(error.to_string().contains("reserved"), "{error}");
    }

    /// A path in a fresh directory, named the way the config directory would
    /// name it.
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
        // The file survives its last key and reads back as the empty document;
        // a CLI test asserts that its bytes are empty.
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

    // The commands that edit it.

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
        // The point of the divergence from `disabled`, and the thing a later
        // edit aiming for consistency would silently undo.
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
        // `VarNameError` states only the rule, so the diagnostic adds the key.
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
