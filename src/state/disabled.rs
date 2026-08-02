//! `disabled.toml`: the actions and groups switched off on this machine.
//!
//! A closed record with two address lists
//! (`docs/state.md#disabledtoml-disabled-actions-and-groups`). Addresses are
//! held as plain strings: enable and disable validate an address for *syntax*
//! only and never load the repository, precisely so an address can be recorded
//! before the action, group, inclusion, or manifest entry it names exists.
//!
//! Both lists are sets. The file promises no duplicates and a stable order, and
//! a mutation that changes nothing must not rewrite the document merely to sort
//! it — a [`BTreeSet`] gives both, and its `insert`/`remove` return whether the
//! set actually changed.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::tomlfile;

/// The parsed `disabled.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Disabled {
    /// Action, remote, included-action, and manifest-entry addresses.
    #[serde(default)]
    pub actions: BTreeSet<String>,
    /// Group addresses.
    #[serde(default)]
    pub groups: BTreeSet<String>,
}

impl Disabled {
    /// The document's name. Which directory it sits in is
    /// [`Roots`](crate::config::Roots)' answer, not this type's.
    pub const FILE_NAME: &'static str = "disabled.toml";

    /// Load the document, treating a missing file as an empty disabled set.
    pub fn load(path: &Path) -> Result<Self, tomlfile::Error> {
        tomlfile::read_or_default(path)
    }

    /// Rewrite the document.
    ///
    /// Both keys are always written, including when their sets are empty: an
    /// empty `disabled.toml` is kept rather than deleted, and spelling out the
    /// two arrays keeps a hand-edited file self-explanatory.
    pub fn save(&self, path: &Path) -> Result<(), tomlfile::Error> {
        tomlfile::write(path, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(document: &str) -> Result<Disabled, toml::de::Error> {
        toml::from_str(document)
    }

    fn set(items: [&str; 2]) -> BTreeSet<String> {
        items.into_iter().map(str::to_owned).collect()
    }

    #[test]
    fn both_lists_parse() {
        let disabled = parse("actions = ['p10k', 'core.zshrc']\ngroups = ['work', 'core.shell']\n")
            .expect("parse");
        assert_eq!(disabled.actions, set(["p10k", "core.zshrc"]));
        assert_eq!(disabled.groups, set(["work", "core.shell"]));
    }

    #[test]
    fn either_list_may_be_absent() {
        assert_eq!(parse("").expect("empty"), Disabled::default());
        assert_eq!(
            parse("actions = ['p10k']\n").expect("actions only").groups,
            BTreeSet::new()
        );
    }

    #[test]
    fn no_other_field_is_allowed() {
        let error = parse("remotes = ['core']\n").expect_err("closed record");
        assert!(
            error.to_string().contains("unknown field `remotes`"),
            "{error}"
        );
    }

    #[test]
    fn a_qualified_address_is_stored_as_written() {
        // Nothing here splits or resolves an address; that is the point of the
        // file surviving a repository it cannot read.
        let disabled = parse("actions = ['core.zsh-plugins.p10k']\n").expect("parse");
        assert!(disabled.actions.contains("core.zsh-plugins.p10k"));
    }

    #[test]
    fn duplicates_and_order_are_canonicalized_on_read() {
        let disabled = parse("actions = ['b', 'a', 'b']\n").expect("parse");
        assert_eq!(
            disabled.actions.iter().collect::<Vec<_>>(),
            ["a", "b"].iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_mutation_reports_whether_it_changed_anything() {
        let mut disabled = parse("actions = ['p10k']\n").expect("parse");
        assert!(!disabled.actions.insert("p10k".to_owned()));
        assert!(disabled.actions.insert("core.zshrc".to_owned()));
        assert!(!disabled.groups.remove("absent"));
    }

    /// A path in a fresh directory, named the way the config directory would
    /// name it.
    fn path(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join(Disabled::FILE_NAME)
    }

    #[test]
    fn the_written_document_is_sorted_and_deduplicated() {
        let dir = tempfile::tempdir().expect("temp dir");
        parse("actions = ['p10k', 'core.zshrc', 'p10k']\n")
            .expect("parse")
            .save(&path(&dir))
            .expect("save");

        assert_eq!(
            std::fs::read_to_string(path(&dir)).expect("read"),
            "actions = [\"core.zshrc\", \"p10k\"]\ngroups = []\n"
        );
    }

    #[test]
    fn an_empty_set_still_leaves_a_canonical_document() {
        let dir = tempfile::tempdir().expect("temp dir");
        Disabled::default().save(&path(&dir)).expect("save");

        assert_eq!(
            std::fs::read_to_string(path(&dir)).expect("read"),
            "actions = []\ngroups = []\n"
        );
        assert_eq!(
            Disabled::load(&path(&dir)).expect("load"),
            Disabled::default()
        );
    }

    #[test]
    fn a_missing_file_disables_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            Disabled::load(&path(&dir)).expect("absent is empty"),
            Disabled::default()
        );
    }

    #[test]
    fn saving_and_loading_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let disabled = parse("actions = ['p10k']\ngroups = ['gui']\n").expect("parse");

        disabled.save(&path(&dir)).expect("save");
        assert_eq!(Disabled::load(&path(&dir)).expect("load"), disabled);
    }
}
