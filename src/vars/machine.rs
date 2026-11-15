//! Machine-local `vars.toml`: values that repository values and environment/CLI
//! overrides never change. [`crate::commands::vars`] edits it.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::filesystem::tomlfile;
use crate::vars::name::VarName;

/// Machine-local variable values parsed from `vars.toml`.
#[derive(Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct MachineVars {
    pub values: BTreeMap<VarName, String>,
}

impl MachineVars {
    /// The document's file name; [`StateRoots`](crate::location::StateRoots) decides its directory.
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
}
