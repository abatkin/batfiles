//! `disabled.toml`: the actions and groups switched off on this machine, and
//! the four commands that edit it — `disable-action`, `enable-action`,
//! `disable-group`, and `enable-group`.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::item::{ItemAddress, ItemKind};
use crate::location::StateRoots;
use crate::output::Reporter;
use crate::tomlfile;

/// The parsed `disabled.toml`.
#[derive(Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DisabledItems {
    /// Disabled action addresses.
    #[serde(default)]
    pub actions: BTreeSet<ItemAddress>,
    /// Disabled group addresses.
    #[serde(default)]
    pub groups: BTreeSet<ItemAddress>,
}

impl DisabledItems {
    /// The document's file name; [`StateRoots`] decides its directory.
    pub const FILE_NAME: &'static str = "disabled.toml";

    /// Load the document, treating a missing file as an empty disabled set.
    pub fn load(path: &Path) -> Result<Self, Error> {
        tomlfile::read_or_default(path)
    }

    /// Rewrite the document.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        tomlfile::write(path, self)
    }

    /// The list holding disabled names of `kind`.
    pub fn list_mut(&mut self, kind: ItemKind) -> &mut BTreeSet<ItemAddress> {
        match kind {
            ItemKind::Action => &mut self.actions,
            ItemKind::Group => &mut self.groups,
        }
    }
}

/// Whether to add an address to disabled state or remove it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Disable,
    Enable,
}

/// Add the names to, or remove them from, the machine-local disabled list.
pub(crate) fn run(
    names: &[String],
    kind: ItemKind,
    change: Change,
    roots: &StateRoots,
    reporter: &Reporter,
) -> Result<(), Error> {
    // Validate all names before reading or writing state.
    let names = parse_all(names, reporter)?;

    let path = roots.disabled_path();
    let mut disabled = DisabledItems::load(&path)?;
    let set = disabled.list_mut(kind);

    let mut changed = false;
    let mut lines: Vec<String> = Vec::with_capacity(names.len());
    for name in &names {
        let mutated = apply(set, change, name);
        changed |= mutated;
        lines.push(outcome(kind, change, name, mutated));
    }

    if changed {
        disabled.save(&path)?;
    }

    for line in &lines {
        reporter.info(line);
    }
    Ok(())
}

/// Validate every supplied name, keeping the first of each.
fn parse_all(names: &[String], reporter: &Reporter) -> Result<Vec<ItemAddress>, Error> {
    let mut parsed: Vec<ItemAddress> = Vec::with_capacity(names.len());
    for name in names {
        let name = ItemAddress::try_from(name.clone())?;
        if parsed.contains(&name) {
            reporter.warn(&format!("`{name}` was given more than once"));
        } else {
            parsed.push(name);
        }
    }
    Ok(parsed)
}

/// Enable or disable an address, returning whether the set changed.
pub(crate) fn apply(set: &mut BTreeSet<ItemAddress>, change: Change, name: &ItemAddress) -> bool {
    match change {
        Change::Disable => set.insert(name.clone()),
        Change::Enable => set.remove(name),
    }
}

/// Format the outcome of enabling or disabling an action or group.
pub(crate) fn outcome(kind: ItemKind, change: Change, name: &ItemAddress, changed: bool) -> String {
    match (change, changed) {
        (Change::Disable, true) => format!("disabled {kind} `{name}`"),
        (Change::Disable, false) => format!("{kind} `{name}` was already disabled"),
        (Change::Enable, true) => format!("enabled {kind} `{name}` (was disabled)"),
        (Change::Enable, false) => format!("{kind} `{name}` was already enabled"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(document: &str) -> Result<DisabledItems, toml::de::Error> {
        toml::from_str(document)
    }

    fn id(id: &str) -> ItemAddress {
        ItemAddress::try_from(id.to_owned()).expect("valid address")
    }

    fn set<const N: usize>(items: [&str; N]) -> BTreeSet<ItemAddress> {
        items.into_iter().map(id).collect()
    }

    #[test]
    fn both_lists_parse() {
        let disabled =
            parse("actions = ['p10k', 'zshrc']\ngroups = ['work', 'shell']\n").expect("parse");
        assert_eq!(disabled.actions, set(["p10k", "zshrc"]));
        assert_eq!(disabled.groups, set(["work", "shell"]));
    }

    #[test]
    fn either_list_may_be_absent() {
        assert_eq!(parse("").expect("empty"), DisabledItems::default());
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
    fn duplicates_and_order_are_canonicalized_on_read() {
        let disabled = parse("actions = ['b', 'a', 'b']\n").expect("parse");
        assert_eq!(
            disabled.actions.iter().collect::<Vec<_>>(),
            [id("a"), id("b")].iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_malformed_entry_fails_the_document() {
        let error = parse("groups = ['my group']\n").expect_err("spaces are not IDs");
        assert!(error.to_string().contains("`my group`"), "{error}");
    }

    #[test]
    fn a_qualified_address_is_recorded_without_being_resolved() {
        let disabled = parse("actions = ['core.zshrc', 'a.b.c.d.e']\n").expect("parse");
        assert_eq!(disabled.actions, set(["a.b.c.d.e", "core.zshrc"]));
    }

    /// Return a `disabled.toml` path under the temporary directory.
    fn path(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join(DisabledItems::FILE_NAME)
    }

    /// The document as it would be serialized; [`crate::tomlfile`] writes it.
    fn written(disabled: &DisabledItems) -> String {
        toml::to_string(disabled).expect("serialize")
    }

    #[test]
    fn the_written_document_is_sorted_and_deduplicated() {
        let disabled = parse("actions = ['zshrc', 'p10k', 'zshrc']\n").expect("parse");
        assert_eq!(
            written(&disabled),
            "actions = [\"p10k\", \"zshrc\"]\ngroups = []\n"
        );
    }

    #[test]
    fn an_empty_set_still_leaves_a_canonical_document() {
        assert_eq!(
            written(&DisabledItems::default()),
            "actions = []\ngroups = []\n"
        );
        assert_eq!(
            parse("actions = []\ngroups = []\n").expect("parse"),
            DisabledItems::default()
        );
    }

    #[test]
    fn a_missing_file_disables_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            DisabledItems::load(&path(&dir)).expect("absent is empty"),
            DisabledItems::default()
        );
    }

    #[test]
    fn saving_and_loading_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let disabled = parse("actions = ['p10k']\ngroups = ['gui']\n").expect("parse");

        disabled.save(&path(&dir)).expect("save");
        assert_eq!(DisabledItems::load(&path(&dir)).expect("load"), disabled);
    }

    fn quiet() -> Reporter {
        let mut reporter = Reporter::new(false);
        reporter.set_verbosity(crate::output::Verbosity::Quiet);
        reporter
    }

    #[test]
    fn disabling_an_absent_name_changes_the_set() {
        let mut actions = BTreeSet::new();
        assert!(apply(&mut actions, Change::Disable, &id("p10k")));
        assert_eq!(actions, set(["p10k"]));
    }

    #[test]
    fn disabling_a_present_name_changes_nothing() {
        let mut actions = set(["p10k"]);
        assert!(!apply(&mut actions, Change::Disable, &id("p10k")));
        assert_eq!(actions, set(["p10k"]));
    }

    #[test]
    fn enabling_an_absent_name_changes_nothing() {
        let mut actions = set(["p10k"]);
        assert!(!apply(&mut actions, Change::Enable, &id("zshrc")));
        assert_eq!(actions, set(["p10k"]));
    }

    #[test]
    fn enabling_a_present_name_changes_the_set() {
        let mut actions = set(["p10k"]);
        assert!(apply(&mut actions, Change::Enable, &id("p10k")));
        assert!(actions.is_empty());
    }

    #[test]
    fn a_mutation_reports_whether_it_changed_anything() {
        let mut disabled = parse("actions = ['p10k']\n").expect("parse");
        assert!(!disabled.actions.insert(id("p10k")));
        assert!(disabled.actions.insert(id("zshrc")));
        assert!(!disabled.groups.remove(&id("absent")));
    }

    #[test]
    fn each_list_is_edited_on_its_own() {
        let mut disabled = DisabledItems::default();
        apply(
            disabled.list_mut(ItemKind::Group),
            Change::Disable,
            &id("work"),
        );
        assert!(disabled.actions.is_empty());
        assert_eq!(disabled.groups, set(["work"]));
    }

    #[test]
    fn every_outcome_says_whether_the_state_moved() {
        let name = id("p10k");
        let line = |change, changed| outcome(ItemKind::Action, change, &name, changed);
        assert_eq!(line(Change::Disable, true), "disabled action `p10k`");
        assert_eq!(
            line(Change::Disable, false),
            "action `p10k` was already disabled"
        );
        assert_eq!(
            line(Change::Enable, true),
            "enabled action `p10k` (was disabled)"
        );
        assert_eq!(
            line(Change::Enable, false),
            "action `p10k` was already enabled"
        );
    }

    #[test]
    fn the_outcome_names_the_kind_the_command_edits() {
        assert_eq!(
            outcome(ItemKind::Group, Change::Disable, &id("work"), true),
            "disabled group `work`"
        );
    }

    #[test]
    fn a_repeated_name_is_collapsed_rather_than_applied_twice() {
        let names = ["p10k".to_owned(), "zshrc".to_owned(), "p10k".to_owned()];
        assert_eq!(
            parse_all(&names, &quiet()).expect("valid names"),
            [id("p10k"), id("zshrc")]
        );
    }

    #[test]
    fn one_invalid_name_rejects_the_whole_invocation() {
        let names = ["p10k".to_owned(), "core..p10k".to_owned()];
        let error = parse_all(&names, &quiet()).expect_err("an empty segment is not an address");
        assert!(error.to_string().contains("`core..p10k`"), "{error}");
    }
}
