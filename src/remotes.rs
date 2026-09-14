//! `remotes/`: bringing the repositories a manifest declares onto this machine.
//!
//! A declared remote is materialized because it was declared, not because
//! something reaches into it: an action naming one installs from the tree that
//! is already there, and a remote nothing names is brought down all the same.
//! Where each one lands is
//! [`RunContext::materialization`](crate::action::RunContext::materialization).

use std::collections::BTreeMap;

use crate::action::RunContext;
use crate::condition::{Bindings, Exclusion};
use crate::error::Error;
use crate::git;
use crate::item::ItemId;
use crate::manifest::remote::Remote;

/// The tool-owned directory inside the leaf repository that every
/// materialization sits under.
pub(crate) const DIRECTORY: &str = "remotes";

/// What the warning for an undecidable remote condition says the run did about
/// it: the remote is not brought onto the machine, and nothing reads the tree
/// where an earlier run left one.
const NOT_MATERIALIZED: &str = "it is not materialized";

/// Which declared remotes this machine does not have, and why.
///
/// Settled by every command rather than by `sync` alone: only `sync`
/// materializes, but an action resolving a path into a remote is refused
/// wherever it runs, and evaluation is pure. A condition batfiles cannot decide
/// closes the gate here as everywhere else.
pub(crate) fn excluded(
    remotes: &BTreeMap<ItemId, Remote>,
    bindings: &Bindings<'_>,
) -> BTreeMap<ItemId, Exclusion> {
    remotes
        .iter()
        .filter_map(|(id, remote)| {
            let exclusion = remote
                .gate()?
                .verdict(bindings, Some(NOT_MATERIALIZED))
                .exclusion()?;
            Some((id.clone(), exclusion))
        })
        .collect()
}

/// Materialize every declared remote, in ID order, before the run's first
/// action.
///
/// Each is cloned where nothing is and updated where a materialization already
/// is, on a `git-clone` action's terms: the same conservative update policy,
/// the same refusals at a destination holding something else, and the same
/// silence under [`RunMode::DryRun`](crate::mode::RunMode::DryRun), which runs
/// no git for any caller.
///
/// A remote this machine's conditions close is reported and passed over. A
/// materialization an earlier run left is neither updated nor removed; what
/// keeps it from being installed from is path resolution, which refuses an
/// excluded remote.
///
/// A failure stops the run, as an action's does: an action later in the list
/// may install from the tree that is not there, and continuing would mean a
/// `sync` reporting success over a remote it never brought down.
pub(crate) fn materialize(
    remotes: &BTreeMap<ItemId, Remote>,
    context: &RunContext<'_>,
) -> Result<(), Error> {
    for (id, remote) in remotes {
        // The two lines an excluded action gets, under the heading this
        // remote's own work would have printed under.
        if let Some(exclusion) = context.excluded_remote(id) {
            let heading = format!("remote {id}");
            match exclusion {
                Exclusion::Expected(why) => context
                    .reporter()
                    .detail(1, &format!("{heading} - skipped: {why}")),
                Exclusion::EvaluationFailed(why) => {
                    context.reporter().warn(&format!("{heading}: {why}"))
                }
            }
            continue;
        }
        // A `file` or `archive` remote is refused by name as the manifest is
        // read, so a record that reaches here is a Git one.
        // CARRY(9.3): the other two kinds materialize differently — a download
        // and an unpacked archive — so they arrive as arms of their own here.
        let Remote::Git(remote) = remote else {
            continue;
        };
        // The heading an action gets at `-v`, for the work that happens before
        // the first one: the lines below name paths, and this names the record
        // they came from.
        context.reporter().detail(1, &format!("remote {id}"));
        git::clone_or_update(
            &remote.url,
            &context.materialization(id),
            remote.git_ref.as_deref(),
            context.repository(),
            context.mode(),
            context.reporter(),
        )?;
    }
    Ok(())
}
