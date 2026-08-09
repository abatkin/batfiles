//! `vars.toml`: deliberate, non-regenerable variable overrides for one machine.
//!
//! The whole document is a map from user-variable name to string value.
//! Top-level keys are data rather than schema fields, so the map accepts any key
//! that is a valid variable name — which is what [`VarName`] as the key type
//! enforces, at the point the file is read.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::tomlfile;
use crate::var::VarName;

/// The parsed `vars.toml`.
///
/// The map is exposed directly: this file has no structure beyond it, and the
/// precedence layer it contributes to belongs to variable resolution rather
/// than here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct MachineVars {
    pub values: BTreeMap<VarName, String>,
}

impl MachineVars {
    /// The document's name. Which directory it sits in is
    /// [`Roots`](crate::config::Roots)' answer, not this type's.
    pub const FILE_NAME: &'static str = "vars.toml";

    /// Load the document, treating a missing file as no overrides.
    pub fn load(path: &Path) -> Result<Self, tomlfile::Error> {
        tomlfile::read_or_default(path)
    }

    /// Rewrite the document.
    ///
    /// An empty map writes an empty file: removing the last key leaves a valid
    /// `vars.toml` behind rather than deleting it.
    pub fn save(&self, path: &Path) -> Result<(), tomlfile::Error> {
        tomlfile::write(path, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(name: &str) -> VarName {
        VarName::new(name).expect("valid name")
    }

    fn parse(document: &str) -> Result<MachineVars, toml::de::Error> {
        toml::from_str(document)
    }

    #[test]
    fn the_document_is_a_flat_map_of_strings() {
        let vars = parse("editor = 'nvim'\nprofile = 'work'\nwork = 'true'\n").expect("parse");
        assert_eq!(vars.values.len(), 3);
        assert_eq!(vars.values[&name("editor")], "nvim");
    }

    #[test]
    fn an_empty_document_is_valid() {
        assert_eq!(parse("").expect("empty"), MachineVars::default());
    }

    #[test]
    fn values_are_strings_and_are_never_inferred() {
        let error = parse("work = true\n").expect_err("booleans are not values");
        assert!(
            error.to_string().contains("invalid type: boolean"),
            "{error}"
        );
    }

    #[test]
    fn a_key_must_be_a_valid_variable_name() {
        let error = parse("has-dash = 'x'\n").expect_err("dashes are not variable names");
        assert!(
            error.to_string().contains("a variable name must"),
            "{error}"
        );

        let error = parse("env = 'x'\n").expect_err("reserved");
        assert!(error.to_string().contains("reserved"), "{error}");
    }

    /// A path in a fresh directory, named the way the config directory would
    /// name it.
    fn path(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join(MachineVars::FILE_NAME)
    }

    #[test]
    fn a_missing_file_means_no_overrides() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            MachineVars::load(&path(&dir)).expect("absent is empty"),
            MachineVars::default()
        );
    }

    #[test]
    fn saving_and_loading_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let vars = parse("editor = 'nvim'\nprofile = 'work'\n").expect("parse");

        vars.save(&path(&dir)).expect("save");
        assert_eq!(MachineVars::load(&path(&dir)).expect("load"), vars);
    }

    #[test]
    fn removing_the_last_key_leaves_an_empty_document_behind() {
        let dir = tempfile::tempdir().expect("temp dir");
        parse("editor = 'nvim'\n")
            .expect("parse")
            .save(&path(&dir))
            .expect("save");

        MachineVars::default()
            .save(&path(&dir))
            .expect("save empty");

        assert!(path(&dir).is_file(), "the file should survive");
        assert_eq!(std::fs::read_to_string(path(&dir)).expect("read"), "");
    }

    #[test]
    fn saving_creates_the_config_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let nested = dir
            .path()
            .join("config/batfiles")
            .join(MachineVars::FILE_NAME);

        parse("editor = 'nvim'\n")
            .expect("parse")
            .save(&nested)
            .expect("save");
        assert!(nested.is_file());
    }

    #[test]
    fn a_malformed_document_is_fatal_rather_than_reset() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(path(&dir), "editor = \n").expect("fixture");

        assert!(MachineVars::load(&path(&dir)).is_err());
        assert_eq!(
            std::fs::read_to_string(path(&dir)).expect("read"),
            "editor = \n",
            "a failed load must not touch the file"
        );
    }
}
