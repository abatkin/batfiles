//! The persistent enable/disable commands: `disable-action`, `enable-action`,
//! `disable-group`, and `enable-group`.
//!
//! One implementation with four entry points, differing only in which list they
//! edit and which way they move an address.
//!
//! These commands read and write `disabled.toml` and nothing else. They do not
//! load the leaf repository, so an unreadable or invalid `batfiles.toml` cannot
//! fail one, and they therefore validate a supplied address for *syntax* alone.
//! Recording an address that names nothing today is the point rather than a
//! mistake to warn about: a later branch change or Git update may introduce it,
//! and manifest entries are not known until their action executes.
//!
//! They run no synchronization and remove no installed content.

use std::collections::BTreeSet;
use std::fmt;

use crate::config::Roots;
use crate::item::{ItemAddress, ItemAddressError};
use crate::output::Reporter;
use crate::state::Disabled;
use crate::tomlfile;

/// Which of the two disabled lists a command edits.
///
/// Nothing in the address syntax distinguishes an action from a group, so this
/// is the command's choice alone: `disable-action` will happily record what is
/// really a group address. That is inherent to validating syntax only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum List {
    Actions,
    Groups,
}

/// Which way a command moves an address through its list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Disable,
    Enable,
}

/// Add the addresses to, or remove them from, the machine-local disabled list.
pub(crate) fn run(
    addresses: &[String],
    list: List,
    direction: Direction,
    roots: &Roots,
    reporter: &Reporter,
) -> Result<(), Error> {
    // Every address is validated before the document is touched, so an
    // invocation either applies in full or changes nothing.
    let addresses = parse_all(addresses, reporter)?;

    let path = roots.disabled();
    let mut disabled = Disabled::load(&path)?;
    let set = list.set(&mut disabled);

    let mut changed = false;
    for address in &addresses {
        let mutated = apply(set, direction, address);
        changed |= mutated;
        reporter.info(&outcome(list, direction, address, mutated));
    }

    // A mutation that changed nothing must not rewrite the document merely to
    // sort or deduplicate it — and a command that changed nothing does not
    // create a `disabled.toml` that was not there before.
    if changed {
        disabled.save(&path)?;
    }
    Ok(())
}

/// Validate every supplied address, keeping the first of each.
///
/// A repeated address warns rather than failing: it names one thing however
/// many times it was written, so the invocation still has an unambiguous
/// meaning. Order is preserved so the output follows the command line.
fn parse_all(addresses: &[String], reporter: &Reporter) -> Result<Vec<ItemAddress>, Error> {
    let mut parsed: Vec<ItemAddress> = Vec::with_capacity(addresses.len());
    for address in addresses {
        let address = ItemAddress::new(address)?;
        if parsed.contains(&address) {
            reporter.warn(&format!("`{address}` was given more than once"));
        } else {
            parsed.push(address);
        }
    }
    Ok(parsed)
}

/// Move one address, reporting whether the set actually changed.
///
/// `BTreeSet` answers that directly, and the answer is what keeps an idempotent
/// mutation from rewriting the file.
fn apply(set: &mut BTreeSet<ItemAddress>, direction: Direction, address: &ItemAddress) -> bool {
    match direction {
        Direction::Disable => set.insert(address.clone()),
        Direction::Enable => set.remove(address),
    }
}

/// The line describing what one address's mutation did.
///
/// A real state change is spelled out — re-enabling something that was actually
/// off is the outcome that must never be silent or ambiguous.
fn outcome(list: List, direction: Direction, address: &ItemAddress, changed: bool) -> String {
    let noun = list.noun();
    match (direction, changed) {
        (Direction::Disable, true) => format!("disabled {noun} `{address}`"),
        (Direction::Disable, false) => format!("{noun} `{address}` was already disabled"),
        (Direction::Enable, true) => format!("enabled {noun} `{address}` (was disabled)"),
        (Direction::Enable, false) => format!("{noun} `{address}` was already enabled"),
    }
}

impl List {
    /// The set this command edits.
    fn set(self, disabled: &mut Disabled) -> &mut BTreeSet<ItemAddress> {
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

/// Why an enable or disable failed.
#[derive(Debug)]
pub(crate) enum Error {
    /// An argument was not a well-formed address. Nothing was written.
    Address(ItemAddressError),
    /// `disabled.toml` could not be read or replaced.
    Document(tomlfile::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Both sources already name the offending value or file, so neither
        // needs a prefix here.
        match self {
            Self::Address(error) => error.fmt(f),
            Self::Document(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Address(error) => Some(error),
            Self::Document(error) => Some(error),
        }
    }
}

impl From<ItemAddressError> for Error {
    fn from(error: ItemAddressError) -> Self {
        Self::Address(error)
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
    use crate::output::Verbosity;

    fn address(address: &str) -> ItemAddress {
        ItemAddress::new(address).expect("valid address")
    }

    fn quiet() -> Reporter {
        Reporter::new(false, Verbosity::Quiet)
    }

    fn set(items: [&str; 1]) -> BTreeSet<ItemAddress> {
        items.into_iter().map(address).collect()
    }

    #[test]
    fn disabling_an_absent_address_changes_the_set() {
        let mut actions = BTreeSet::new();
        assert!(apply(&mut actions, Direction::Disable, &address("p10k")));
        assert_eq!(actions, set(["p10k"]));
    }

    #[test]
    fn disabling_a_present_address_changes_nothing() {
        let mut actions = set(["p10k"]);
        assert!(!apply(&mut actions, Direction::Disable, &address("p10k")));
        assert_eq!(actions, set(["p10k"]));
    }

    #[test]
    fn enabling_an_absent_address_changes_nothing() {
        let mut actions = set(["p10k"]);
        assert!(!apply(
            &mut actions,
            Direction::Enable,
            &address("core.zshrc")
        ));
        assert_eq!(actions, set(["p10k"]));
    }

    #[test]
    fn enabling_a_present_address_changes_the_set() {
        let mut actions = set(["p10k"]);
        assert!(apply(&mut actions, Direction::Enable, &address("p10k")));
        assert!(actions.is_empty());
    }

    #[test]
    fn each_list_is_edited_on_its_own() {
        let mut disabled = Disabled::default();
        apply(
            List::Groups.set(&mut disabled),
            Direction::Disable,
            &address("work"),
        );
        assert!(disabled.actions.is_empty());
        assert_eq!(disabled.groups, set(["work"]));
    }

    #[test]
    fn every_outcome_says_whether_the_state_moved() {
        let address = address("p10k");
        let line = |direction, changed| outcome(List::Actions, direction, &address, changed);
        assert_eq!(line(Direction::Disable, true), "disabled action `p10k`");
        assert_eq!(
            line(Direction::Disable, false),
            "action `p10k` was already disabled"
        );
        assert_eq!(
            line(Direction::Enable, true),
            "enabled action `p10k` (was disabled)"
        );
        assert_eq!(
            line(Direction::Enable, false),
            "action `p10k` was already enabled"
        );
    }

    #[test]
    fn the_outcome_names_the_kind_the_command_edits() {
        assert_eq!(
            outcome(List::Groups, Direction::Disable, &address("work"), true),
            "disabled group `work`"
        );
    }

    #[test]
    fn a_repeated_address_is_collapsed_rather_than_applied_twice() {
        let addresses = [
            "p10k".to_owned(),
            "core.zshrc".to_owned(),
            "p10k".to_owned(),
        ];
        assert_eq!(
            parse_all(&addresses, &quiet()).expect("valid addresses"),
            [address("p10k"), address("core.zshrc")]
        );
    }

    #[test]
    fn one_invalid_address_rejects_the_whole_invocation() {
        // Syntax is the only rule here, and `src/item.rs` already tests it; what
        // matters is that a bad argument stops the command before it writes.
        let addresses = ["p10k".to_owned(), "a..b".to_owned()];
        let error = parse_all(&addresses, &quiet()).expect_err("an empty segment is invalid");
        assert!(error.to_string().contains("`a..b`"), "{error}");
    }

    #[test]
    fn an_unresolvable_address_is_still_well_formed() {
        // Deeper than any form the repository model resolves, and accepted
        // anyway: these commands resolve nothing.
        let addresses = ["a.b.c.d.e".to_owned()];
        assert_eq!(
            parse_all(&addresses, &quiet()).expect("valid address"),
            [address("a.b.c.d.e")]
        );
    }
}
