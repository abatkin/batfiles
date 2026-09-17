//! Carrying one `[[actions]]` record out.

mod children;
mod context;
mod copy;
mod create_dir;
mod fetch_archive;
mod fetch_file;
mod git_clone;
mod git_clone_list;
mod include_remote;
mod symlink;

pub(crate) use context::RunContext;
pub(crate) use include_remote::{IncludedAction, InclusionContents};

use crate::error::Error;
use crate::item::ItemId;
use crate::manifest::action::{Action, IncludeRemoteAction};

/// Read the manifest one `include-remote` includes: its `[vars]`, and its
/// records with what that inclusion's filters made of each. `None` where there
/// was no materialization to read.
///
/// Called while the list is assembled rather than while it is executed: what an
/// inclusion brings in has to be in the list before selection is captured or a
/// clone list is prepared.
pub(crate) fn read_inclusion(
    action: &IncludeRemoteAction,
    context: &RunContext<'_>,
) -> Result<Option<InclusionContents>, Error> {
    include_remote::read(action, context)
}

/// How a report names one `include-remote`, for the lines about a record it
/// contributed that are said where the record is rather than where the
/// inclusion is.
pub(crate) fn inclusion_label(action: &IncludeRemoteAction) -> String {
    include_remote::label(action)
}

/// Carry out one action, whichever kind it is.
///
/// `remote` is the one whose materialization contributed the record, for an
/// action an [`include-remote`](crate::manifest::action::IncludeRemoteAction)
/// spliced into the run's list, and `None` for one the leaf declared. Only the
/// four actions that install from a repository path consult it; what the others
/// name is a URL, a repository for git, or nothing at all.
pub(crate) fn run(
    action: &Action,
    remote: Option<&ItemId>,
    context: &RunContext,
) -> Result<(), Error> {
    match action {
        Action::Symlink(action) => symlink::link(action, remote, context),
        Action::SymlinkDir(action) => symlink::link_dir(action, remote, context),
        Action::CreateDir(action) => create_dir::create_dir(action, context),
        Action::Copy(action) => copy::copy(action, remote, context),
        Action::CopyDir(action) => copy::copy_dir(action, remote, context),
        Action::FetchFile(action) => fetch_file::fetch_file(action, context),
        Action::FetchArchive(action) => fetch_archive::fetch_archive(action, context),
        Action::GitClone(action) => git_clone::git_clone(action, context),
        Action::GitCloneList(action) => git_clone_list::git_clone_list(action, context),
        // The inclusion did its work while the run's list was assembled, and
        // what it contributed is in that list under headings of its own. The
        // record itself installs nothing.
        Action::IncludeRemote(_) => Ok(()),
    }
}
