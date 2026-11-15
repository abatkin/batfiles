//! The four commands that edit `disabled.toml`: `disable-action`,
//! `enable-action`, `disable-group`, and `enable-group`.

use crate::commands::Session;
use crate::error::Error;
use crate::manifest::disabled::{Change, DisabledItems, apply, outcome};
use crate::manifest::item::{ItemAddress, ItemKind};
use crate::output::Reporter;

/// Add the names to, or remove them from, the machine-local disabled list, under the run lock.
pub(crate) fn run(
    names: &[String],
    kind: ItemKind,
    change: Change,
    session: &Session<'_>,
) -> Result<(), Error> {
    let (roots, _lock) = session.locked_state_roots()?;
    let reporter = session.reporter;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn id(id: &str) -> ItemAddress {
        ItemAddress::try_from(id.to_owned()).expect("valid address")
    }

    fn quiet() -> Reporter {
        let mut reporter = Reporter::new(false);
        reporter.set_verbosity(crate::output::Verbosity::Quiet);
        reporter
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
