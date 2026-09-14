//! `remotes/`: bringing the repositories a manifest declares onto this machine.
//!
//! A declared remote is materialized because it was declared, not because
//! something reaches into it: an action naming one installs from the tree that
//! is already there, and a remote nothing names is brought down all the same.
//! Where each one lands is
//! [`RunContext::materialization`](crate::action::RunContext::materialization).

use std::collections::BTreeMap;

use crate::action::RunContext;
use crate::error::Error;
use crate::git;
use crate::item::ItemId;
use crate::manifest::remote::Remote;

/// The tool-owned directory inside the leaf repository that every
/// materialization sits under.
pub(crate) const DIRECTORY: &str = "remotes";

/// Materialize every declared remote, in ID order, before the run's first
/// action.
///
/// Each is cloned where nothing is and updated where a materialization already
/// is, on a `git-clone` action's terms: the same conservative update policy,
/// the same refusals at a destination holding something else, and the same
/// silence under [`RunMode::DryRun`](crate::mode::RunMode::DryRun), which runs
/// no git for any caller.
///
/// A failure stops the run, as an action's does: an action later in the list
/// may install from the tree that is not there, and continuing would mean a
/// `sync` reporting success over a remote it never brought down.
pub(crate) fn materialize(
    remotes: &BTreeMap<ItemId, Remote>,
    context: &RunContext<'_>,
) -> Result<(), Error> {
    for (id, remote) in remotes {
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
