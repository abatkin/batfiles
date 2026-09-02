//! Carrying one `[[actions]]` record out.
//!
//! One file per action type, plus the two things every action needs: the
//! [`RunContext`] it runs against, and — for the two `-dir` types — the
//! [`children`] loop they share. Nothing else lives here, so which file holds an
//! action is answered by its name.

mod children;
mod context;
mod copy;
mod create_dir;
mod fetch_archive;
mod fetch_file;
mod symlink;

pub(crate) use context::RunContext;

use crate::error::Error;
use crate::manifest::action::Action;

/// Carry out one action, whichever kind it is.
pub(crate) fn run(action: &Action, context: &RunContext) -> Result<(), Error> {
    match action {
        Action::Symlink(action) => symlink::link(action, context),
        Action::SymlinkDir(action) => symlink::link_dir(action, context),
        Action::CreateDir(action) => create_dir::create_dir(action, context),
        Action::Copy(action) => copy::copy(action, context),
        Action::CopyDir(action) => copy::copy_dir(action, context),
        Action::FetchFile(action) => fetch_file::fetch_file(action, context),
        Action::FetchArchive(action) => fetch_archive::fetch_archive(action, context),
    }
}
