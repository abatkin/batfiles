//! `include-remote`: the actions another repository declares, read from the
//! materialization this machine has of it.
//!
//! Reading is all this does. Splicing what it read into the run's declaration
//! order is step 7.2's, so the list is reported and nothing in it is carried
//! out. What the reading can and cannot promise is the part that is settled
//! here: a manifest is read from the tree that is on the machine, however stale
//! the last `sync` left it, and an inclusion with no tree at all says so rather
//! than describing a list it never saw. That warning is what makes a plan
//! [partial](../../docs/cmdline.md#plan-completeness); a run in which nothing
//! reported one is complete, and has nothing of its own to say.

use crate::action::RunContext;
use crate::error::Error;
use crate::manifest::Manifest;
use crate::manifest::action::IncludeRemoteAction;
use crate::paths;

/// Read the manifest of the remote this record includes and report what it
/// declares.
///
/// Four outcomes, and only the third is a failure: a remote this machine's
/// conditions close is passed over with the plan still whole, a remote that is
/// not materialized warns and leaves the plan partial, a materialization with no
/// manifest in it is refused by name, and anything else is read and listed.
// CARRY(7.2): the actions read here are spliced into declaration order, given
// qualified addresses, and executed; this reports them instead.
pub(crate) fn include_remote(
    action: &IncludeRemoteAction,
    context: &RunContext<'_>,
) -> Result<(), Error> {
    let reporter = context.reporter();
    let remote = &action.remote;

    // The manifest declining the inclusion, rather than the run failing to
    // describe it: a plan missing what it was told to leave out is still whole.
    if let Some(exclusion) = context.excluded_remote(remote) {
        exclusion.report(
            reporter,
            &format!("nothing included: {}", exclusion.reason()),
        );
        return Ok(());
    }

    let tree = context.materialization(remote);
    if !paths::occupied(&tree)? {
        // The one thing a missing tree does that a missing source does not: a
        // leaf action reaching an absent materialization is refused, because
        // one action's content is something the rest of the plan can do
        // without. A list of actions is not, so this warns and the run carries
        // on, having said which part of the plan it could not draw.
        reporter.warn(&format!(
            "remote `{remote}` is not materialized at {}, so what it includes \
             cannot be listed; run `batfiles sync` to bring it down",
            tree.display()
        ));
        return Ok(());
    }

    let path = tree.join(Manifest::FILE_NAME);
    if !paths::occupied(&path)? {
        return Err(Error::IncludedManifestMissing {
            remote: remote.clone(),
            path,
        });
    }

    let included = Manifest::load_included(&path)?;
    let count = included.actions.len();
    reporter.info(&format!(
        "read {count} {} from {}",
        if count == 1 { "action" } else { "actions" },
        path.display()
    ));
    for (index, action) in included.actions.iter().enumerate() {
        reporter.detail(1, &format!("  {}", action.describe(index + 1)));
    }
    reporter.info("not run: including them arrives at step 7.2");
    Ok(())
}
