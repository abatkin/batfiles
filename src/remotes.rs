//! Evaluate remote conditions and materialize declared Git repositories.
//! Sync materializes all non-excluded declarations, including unreferenced ones.
//! Apply commands read existing materializations through `RunContext`.

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

/// Evaluate declared remote conditions without I/O.
/// Returns excluded remotes and their reasons, even when an old tree exists.
/// Used by sync and apply commands to prevent reads from excluded remotes.
pub(crate) fn excluded(
    remotes: &BTreeMap<ItemId, Remote>,
    bindings: &Bindings<'_>,
) -> BTreeMap<ItemId, Exclusion> {
    remotes
        .iter()
        .filter_map(|(id, remote)| {
            let exclusion = remote.gate()?.exclusion(bindings, Some(NOT_MATERIALIZED))?;
            Some((id.clone(), exclusion))
        })
        .collect()
}

/// Clone or update non-excluded remotes in ID order using the Git update policy.
/// Report excluded remotes without modifying their existing trees.
/// Dry runs report intent without launching Git. A failure stops the run.
pub(crate) fn materialize(
    remotes: &BTreeMap<ItemId, Remote>,
    context: &RunContext<'_>,
) -> Result<(), Error> {
    for (id, remote) in remotes {
        if let Some(exclusion) = context.excluded_remote(id) {
            let heading = format!("remote {id}");
            exclusion.report_heading(context.reporter(), &heading);
            continue;
        }
        // A `file` or `archive` remote is refused by name as the manifest is
        // read, so a record that reaches here is a Git one.
        // CARRY(9.3): the other two kinds materialize differently — a download
        // and an unpacked archive — so they arrive as arms of their own here.
        let Remote::Git(remote) = remote else {
            continue;
        };
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
