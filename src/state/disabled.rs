//! `disabled.toml`: the actions and groups switched off on this machine.
//!
//! A closed record with two address lists. The file validates address *syntax*
//! as it loads and never resolves an address against a repository, precisely so
//! an address can be recorded before the action, group, inclusion, or manifest
//! entry it names exists: a pre-registered entry matches nothing today and may
//! match after a branch change or a Git update.
//!
//! A malformed address is a different thing, and it fails the load like any
//! other malformed TOML here. It can never become live, so tolerating it would
//! carry a permanently dead entry silently, and dropping it on the next save
//! would make an unrelated `disable-action` destructive. Fixing one means
//! editing the file, which is already how this document may be maintained.
//!
//! Both lists are sets. The file promises no duplicates and a stable order, and
//! a mutation that changes nothing must not rewrite the document merely to sort
//! it — a [`BTreeSet`] gives both, and its `insert`/`remove` return whether the
//! set actually changed.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::item::ItemAddress;
use crate::tomlfile;

/// The parsed `disabled.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Disabled {
    /// Action, remote, included-action, and manifest-entry addresses.
    #[serde(default)]
    pub actions: BTreeSet<ItemAddress>,
    /// Group addresses.
    #[serde(default)]
    pub groups: BTreeSet<ItemAddress>,
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

    fn address(address: &str) -> ItemAddress {
        ItemAddress::new(address).expect("valid address")
    }

    fn set(items: [&str; 2]) -> BTreeSet<ItemAddress> {
        items.into_iter().map(address).collect()
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
        // The address is split into segments but never resolved; that is the
        // point of the file surviving a repository it cannot read.
        let disabled = parse("actions = ['core.zsh-plugins.p10k']\n").expect("parse");
        assert!(disabled.actions.contains(&address("core.zsh-plugins.p10k")));
    }

    #[test]
    fn duplicates_and_order_are_canonicalized_on_read() {
        let disabled = parse("actions = ['b', 'a', 'b']\n").expect("parse");
        assert_eq!(
            disabled.actions.iter().collect::<Vec<_>>(),
            [address("a"), address("b")].iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_mutation_reports_whether_it_changed_anything() {
        let mut disabled = parse("actions = ['p10k']\n").expect("parse");
        assert!(!disabled.actions.insert(address("p10k")));
        assert!(disabled.actions.insert(address("core.zshrc")));
        assert!(!disabled.groups.remove(&address("absent")));
    }

    #[test]
    fn a_malformed_address_fails_the_document() {
        // The rest of `state` treats an unparseable file as fatal, and a
        // malformed address can never become live, so it is not junk worth
        // preserving.
        let error = parse("actions = ['core..p10k']\n").expect_err("empty segment");
        assert!(error.to_string().contains("`core..p10k`"), "{error}");

        let error = parse("groups = ['my group']\n").expect_err("spaces are not IDs");
        assert!(error.to_string().contains("`my group`"), "{error}");
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
    fn addresses_keep_their_textual_order() {
        // The order is visible in the written document, so it stays the one the
        // plain-string representation produced: `-` sorts before `.`.
        let dir = tempfile::tempdir().expect("temp dir");
        parse("actions = ['a.b', 'a-c']\n")
            .expect("parse")
            .save(&path(&dir))
            .expect("save");

        assert_eq!(
            std::fs::read_to_string(path(&dir)).expect("read"),
            "actions = [\"a-c\", \"a.b\"]\ngroups = []\n"
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
