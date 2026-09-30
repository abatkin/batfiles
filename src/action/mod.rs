//! Carrying one `[[actions]]` record out.

mod children;
mod context;
mod copy;
mod create_dir;
mod fetch_archive;
mod fetch_file;
mod git_clone;
mod git_clone_list;
mod symlink;

pub(crate) use context::{Replacement, RunContext};

use crate::clone_list::PreparedList;
use crate::error::Error;
use crate::item::ItemId;
use crate::manifest::action::Action;

/// What carrying one record of the run's list out works from.
///
/// Clone lists must use `CloneList` with their prepared entries. Inclusions are
/// expanded during assembly and are never executable. All other actions use
/// `Declared` with their manifest record.
pub(crate) enum Executable<'a> {
    /// A record whose declaration is the whole of what carrying it out needs.
    Declared(&'a Action),
    /// A clone list, read for this run.
    CloneList(PreparedList<'a>),
}

/// Carry out one record, whichever kind it is.
///
/// `remote` is the materialization an
/// [`include-remote`](crate::manifest::action::IncludeRemoteAction)'s record
/// came from, or `None` for a leaf record. Only the symlink and copy actions
/// consult it; a clone list's path was resolved during preparation.
pub(crate) fn run(
    executable: &Executable<'_>,
    remote: Option<&ItemId>,
    context: &RunContext,
) -> Result<(), Error> {
    let action = match executable {
        Executable::CloneList(list) => return git_clone_list::git_clone_list(list, context),
        Executable::Declared(action) => action,
    };
    match action {
        Action::Symlink(action) => symlink::link(action, remote, context),
        Action::SymlinkDir(action) => symlink::link_dir(action, remote, context),
        Action::CreateDir(action) => create_dir::create_dir(action, context),
        Action::Copy(action) => copy::copy(action, remote, context),
        Action::CopyDir(action) => copy::copy_dir(action, remote, context),
        Action::FetchFile(action) => fetch_file::fetch_file(action, context),
        Action::FetchArchive(action) => fetch_archive::fetch_archive(action, context),
        Action::GitClone(action) => git_clone::git_clone(action, context),
        Action::IncludeRemote(_) => {
            unreachable!("an inclusion is expanded during assembly and never dispatched")
        }
        Action::GitCloneList(_) => {
            unreachable!("an executable clone list is prepared before any action runs")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::location::{Roots, StateRoots};
    use crate::mode::RunMode;
    use crate::output::Reporter;

    /// Dispatch `declaration` as `Declared`, bypassing preparation to exercise
    /// the dispatch boundary's invariants.
    fn dispatch_declared(declaration: &str) {
        let tree = tempfile::tempdir().expect("a temporary root");
        let roots = Roots {
            home: tree.path().join("home"),
            batfiles_dir: tree.path().to_path_buf(),
            state: StateRoots {
                config_dir: tree.path().join("config"),
                cache_dir: tree.path().join("cache"),
            },
        };
        let reporter = Reporter::new(false);
        let context = RunContext::new(
            &roots,
            RunMode::Perform,
            Default::default(),
            Replacement::default(),
            &reporter,
        )
        .expect("the roots resolve");
        let action = toml::from_str(declaration).expect("the declaration parses");

        let _ = run(&Executable::Declared(&action), None, &context);
    }

    #[test]
    #[should_panic(expected = "an executable clone list is prepared before any action runs")]
    fn dispatch_refuses_an_unprepared_clone_list() {
        dispatch_declared(
            r#"type = "git-clone-list"
source = "plugins.txt"
dest-dir = "~/.plugins"
"#,
        );
    }

    #[test]
    #[should_panic(expected = "an inclusion is expanded during assembly and never dispatched")]
    fn dispatch_refuses_an_inclusion() {
        dispatch_declared(
            r#"type = "include-remote"
remote = "corporate"
"#,
        );
    }
}
