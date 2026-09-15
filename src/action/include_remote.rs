//! `include-remote`: the actions another repository declares, read from the
//! materialization this machine has of it.
//!
//! Reading is all this module does, and it happens while the run's list is being
//! assembled rather than while it is executed: what an inclusion brings in has
//! to be in the list before anything can select it. The record itself installs
//! nothing once the list holds what it read.
//!
//! What the reading can and cannot promise: a manifest is read from the tree
//! that is on the machine, however stale the last `sync` left it, and an
//! inclusion with no tree at all says so rather than contributing a list it
//! never saw. That warning is what makes a plan
//! [partial](../../docs/cmdline.md#plan-completeness); a run in which nothing
//! reported one is complete, and has nothing of its own to say.

use crate::action::RunContext;
use crate::error::Error;
use crate::manifest::Manifest;
use crate::manifest::action::{Action, IncludeRemoteAction};
use crate::paths;

/// Read the manifest of the remote this record includes, and return the actions
/// it contributes together with the position each was declared at.
///
/// `None` is an inclusion whose list was never read, because there was no
/// materialization to read it from; `Some([])` is a manifest that was read and
/// declares nothing. Only the second can answer whether a qualified name matches
/// something, which is the distinction a clone list's
/// [entries](crate::manifest::action::GitCloneListAction::entries) also draw.
///
/// A materialization with no manifest in it is the one failure: a remote's
/// manifest is optional, since a remote an action only installs *from* has no
/// use for one, so an inclusion asking for one that is not there is the leaf
/// asking for something absent rather than a tree batfiles has yet to fetch.
///
/// A remote its own condition closed is the caller's to decide, and is settled
/// before asking: that is an exclusion on the record rather than something read
/// from a tree.
pub(super) fn read(
    action: &IncludeRemoteAction,
    context: &RunContext<'_>,
) -> Result<Option<Vec<(usize, Action)>>, Error> {
    let remote = &action.remote;
    let reporter = context.reporter();

    let tree = context.materialization(remote);
    if !paths::occupied(&tree)? {
        // The one thing a missing tree does that a missing source does not: a
        // leaf action reaching an absent materialization is refused, because one
        // action's content is something the rest of the plan can do without. A
        // list of actions is not, so this warns and the run carries on, having
        // said which part of the plan it could not draw.
        reporter.warn(&format!(
            "remote `{remote}` is not materialized at {}, so what it includes \
             cannot be listed; run `batfiles sync` to bring it down",
            tree.display()
        ));
        return Ok(None);
    }

    let manifest = tree.join(Manifest::FILE_NAME);
    if !paths::occupied(&manifest)? {
        return Err(Error::IncludedManifestMissing {
            remote: remote.clone(),
            path: manifest,
        });
    }

    let included = Manifest::load_included(&manifest)?;
    let mut contributed = Vec::with_capacity(included.actions.len());
    for (index, contribution) in included.actions.into_iter().enumerate() {
        // The position the record was declared at, carried rather than
        // recomputed after the filtering below: a record with no `id` is named
        // by where it was written, so the gap a dropped one leaves stays a gap.
        let number = index + 1;
        // Inclusion is one level deep: an included repository does not reach
        // further repositories, which is the same rule that refuses an included
        // action sourcing from a remote. Dropped rather than refused, because
        // the manifest breaking it belongs to someone else and the rest of what
        // it declares is still good — and warned about rather than passed over
        // in silence, because a declaration that is not honored is worth saying.
        // CARRY(7.6): the other half is the included `[remotes]` map, which is
        // read and validated today and has no effect once nothing can name one.
        if let Action::IncludeRemote(_) = contribution {
            reporter.warn(&format!(
                "not included: {}; an included repository does not reach \
                 further repositories",
                contribution.describe(number, action.id.as_ref())
            ));
            continue;
        }
        contributed.push((number, contribution));
    }
    Ok(Some(contributed))
}
