//! `disabled.toml`: the actions and groups switched off on this machine, and
//! the four commands that edit it — `disable-action`, `enable-action`,
//! `disable-group`, and `enable-group`.
//!
//! The document and its only writer live together. One implementation serves
//! all four commands, differing in nothing but which list it edits and which
//! way it moves a name.
//!
//! A closed record with two lists of addresses. The file validates address
//! *syntax* as it loads and never resolves a name against a repository,
//! precisely so a name can be recorded before the action or group it names
//! exists: a pre-registered entry matches nothing today and may match after a
//! branch change or a Git update. That is also what lets a qualified address be
//! written down before any remote can answer to it — recording is all these
//! commands do, so there is nothing for the extra segments to resolve against
//! either way. The commands validate the same way and for the same reason — they do
//! not load the leaf repository, so an unreadable or invalid `batfiles.toml`
//! cannot fail one. They run no synchronization and remove no installed content.
//!
//! A malformed entry is a different thing, and it fails the load like any other
//! malformed TOML. It can never become live, so tolerating it would carry a
//! permanently dead entry silently, and dropping it on the next save would make
//! an unrelated `disable-action` destructive.
//!
//! Both lists are sets. The file promises no duplicates and a stable order, and
//! a mutation that changes nothing must not rewrite the document merely to sort
//! it — a [`BTreeSet`] gives both, and its `insert`/`remove` return whether the
//! set actually changed.

use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::item::ItemAddress;
use crate::location::Roots;
use crate::output::Reporter;
use crate::tomlfile;

/// The parsed `disabled.toml`.
///
/// Read by [`crate::selection`], which filters a run's action list by both
/// lists and by the run-only skips.
#[derive(Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Disabled {
    /// Disabled action addresses.
    #[serde(default)]
    pub actions: BTreeSet<ItemAddress>,
    /// Disabled group addresses.
    #[serde(default)]
    pub groups: BTreeSet<ItemAddress>,
}

impl Disabled {
    /// The document's name. Which directory it sits in is
    /// [`Roots`](crate::location::Roots)' answer, not this type's.
    pub const FILE_NAME: &'static str = "disabled.toml";

    /// Load the document, treating a missing file as an empty disabled set.
    pub fn load(path: &Path) -> Result<Self, Error> {
        tomlfile::read_or_default(path)
    }

    /// Rewrite the document.
    ///
    /// Both keys are always written, including when their sets are empty: an
    /// empty `disabled.toml` is kept rather than deleted, and spelling out the
    /// two arrays keeps a hand-edited file self-explanatory.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        tomlfile::write(path, self)
    }
}

/// Which of the document's two lists a command edits.
///
/// Nothing in the syntax distinguishes an action from a group, so this is the
/// command's choice alone: `disable-action` will happily record what is really
/// a group name. That is inherent to validating syntax only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisabledList {
    Actions,
    Groups,
}

/// Which way a command moves a name through its list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    Disable,
    Enable,
}

/// Add the names to, or remove them from, the machine-local disabled list.
///
/// Nothing is reported until the document has been rewritten, so every line the
/// command prints describes a change that is on disk.
pub(crate) fn run(
    names: &[String],
    list: DisabledList,
    change: Change,
    roots: &Roots,
    reporter: &Reporter,
) -> Result<(), Error> {
    // Every name is validated before the document is touched, so an invocation
    // either applies in full or changes nothing.
    let names = parse_all(names, reporter)?;

    let path = roots.disabled();
    let mut disabled = Disabled::load(&path)?;
    let set = list.set_in(&mut disabled);

    let mut changed = false;
    let mut lines: Vec<String> = Vec::with_capacity(names.len());
    for name in &names {
        let mutated = apply(set, change, name);
        changed |= mutated;
        lines.push(outcome(list, change, name, mutated));
    }

    // A mutation that changed nothing must not rewrite the document merely to
    // sort or deduplicate it — and a command that changed nothing does not
    // create a `disabled.toml` that was not there before.
    if changed {
        disabled.save(&path)?;
    }

    // Reported only once the rewrite has committed. The account is all-or-nothing
    // for the same reason the edit is: a run whose save failed changed nothing,
    // and saying "disabled action `p10k`" above the error explaining that nothing
    // was written describes a state the machine is not in.
    for line in &lines {
        reporter.info(line);
    }
    Ok(())
}

/// Validate every supplied name, keeping the first of each.
///
/// A repeated name warns rather than failing: it names one thing however many
/// times it was written, so the invocation still has an unambiguous meaning.
/// Order is preserved so the output follows the command line.
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

/// Move one name, reporting whether the set actually changed.
///
/// `BTreeSet` answers that directly, and the answer is what keeps an idempotent
/// mutation from rewriting the file.
fn apply(set: &mut BTreeSet<ItemAddress>, change: Change, name: &ItemAddress) -> bool {
    match change {
        Change::Disable => set.insert(name.clone()),
        Change::Enable => set.remove(name),
    }
}

/// The line describing what one name's mutation did.
///
/// A real state change is spelled out — re-enabling something that was actually
/// off is the outcome that must never be silent or ambiguous.
fn outcome(list: DisabledList, change: Change, name: &ItemAddress, changed: bool) -> String {
    let noun = list.noun();
    match (change, changed) {
        (Change::Disable, true) => format!("disabled {noun} `{name}`"),
        (Change::Disable, false) => format!("{noun} `{name}` was already disabled"),
        (Change::Enable, true) => format!("enabled {noun} `{name}` (was disabled)"),
        (Change::Enable, false) => format!("{noun} `{name}` was already enabled"),
    }
}

impl DisabledList {
    /// The set this command edits.
    fn set_in(self, disabled: &mut Disabled) -> &mut BTreeSet<ItemAddress> {
        match self {
            Self::Actions => &mut disabled.actions,
            Self::Groups => &mut disabled.groups,
        }
    }

    /// What one entry is called in the command's output.
    fn noun(self) -> &'static str {
        match self {
            Self::Actions => "action",
            Self::Groups => "group",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(document: &str) -> Result<Disabled, toml::de::Error> {
        toml::from_str(document)
    }

    fn id(id: &str) -> ItemAddress {
        ItemAddress::try_from(id.to_owned()).expect("valid address")
    }

    fn set<const N: usize>(items: [&str; N]) -> BTreeSet<ItemAddress> {
        items.into_iter().map(id).collect()
    }

    // The document.

    #[test]
    fn both_lists_parse() {
        let disabled =
            parse("actions = ['p10k', 'zshrc']\ngroups = ['work', 'shell']\n").expect("parse");
        assert_eq!(disabled.actions, set(["p10k", "zshrc"]));
        assert_eq!(disabled.groups, set(["work", "shell"]));
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
    fn duplicates_and_order_are_canonicalized_on_read() {
        let disabled = parse("actions = ['b', 'a', 'b']\n").expect("parse");
        assert_eq!(
            disabled.actions.iter().collect::<Vec<_>>(),
            [id("a"), id("b")].iter().collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_malformed_entry_fails_the_document() {
        // An entry that can never become live is not junk worth preserving, so
        // it fails the load rather than being carried or silently dropped.
        let error = parse("groups = ['my group']\n").expect_err("spaces are not IDs");
        assert!(error.to_string().contains("`my group`"), "{error}");
    }

    #[test]
    fn a_qualified_address_is_recorded_without_being_resolved() {
        // These lists validate syntax and resolve nothing, so an address naming
        // an included remote's action is stored the way any other name is —
        // which is what lets one be written down before the remote exists.
        let disabled = parse("actions = ['core.zshrc', 'a.b.c.d.e']\n").expect("parse");
        assert_eq!(disabled.actions, set(["a.b.c.d.e", "core.zshrc"]));
    }

    /// A path in a fresh directory, named the way the config directory would
    /// name it.
    fn path(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join(Disabled::FILE_NAME)
    }

    /// The document as it would be written. Serialization is this record's
    /// business; getting those bytes onto disk is [`crate::tomlfile`]'s, and is
    /// tested there.
    fn written(disabled: &Disabled) -> String {
        toml::to_string(disabled).expect("serialize")
    }

    #[test]
    fn the_written_document_is_sorted_and_deduplicated() {
        // Declared out of order and repeated, so the canonical form is the
        // record's doing rather than the input's.
        let disabled = parse("actions = ['zshrc', 'p10k', 'zshrc']\n").expect("parse");
        assert_eq!(
            written(&disabled),
            "actions = [\"p10k\", \"zshrc\"]\ngroups = []\n"
        );
    }

    #[test]
    fn an_empty_set_still_leaves_a_canonical_document() {
        // Both keys are written even when empty: an empty `disabled.toml` is
        // kept rather than deleted, and it reads back as what it started as.
        assert_eq!(written(&Disabled::default()), "actions = []\ngroups = []\n");
        assert_eq!(
            parse("actions = []\ngroups = []\n").expect("parse"),
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

    // The commands that edit it.

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
        let mut disabled = Disabled::default();
        apply(
            DisabledList::Groups.set_in(&mut disabled),
            Change::Disable,
            &id("work"),
        );
        assert!(disabled.actions.is_empty());
        assert_eq!(disabled.groups, set(["work"]));
    }

    #[test]
    fn every_outcome_says_whether_the_state_moved() {
        let name = id("p10k");
        let line = |change, changed| outcome(DisabledList::Actions, change, &name, changed);
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
            outcome(DisabledList::Groups, Change::Disable, &id("work"), true),
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
        // Syntax is the only rule here, and `src/item.rs` already tests it; what
        // matters is that a bad argument stops the command before it writes.
        let names = ["p10k".to_owned(), "core..p10k".to_owned()];
        let error = parse_all(&names, &quiet()).expect_err("an empty segment is not an address");
        assert!(error.to_string().contains("`core..p10k`"), "{error}");
    }
}
