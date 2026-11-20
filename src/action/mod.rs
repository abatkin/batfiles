//! Dispatch prepared actions.

mod children;
mod copy;
mod create_dir;
mod fetch_archive;
mod fetch_file;
mod git_clone;
mod git_clone_list;
mod run;
mod symlink;

use crate::error::Error;
use crate::manifest::actions::Action;
use crate::manifest::item::ItemId;
use crate::manifest::remote::Tree;
use crate::manifest::template::Resolver;
use crate::run::clone_list::PreparedList;
use crate::run::context::RunContext;

/// Inputs for executing an action.
///
/// Clone lists require prepared entries. Other executable actions use `Declared`; inclusions
/// must be expanded before dispatch.
pub(crate) enum PreparedAction<'a> {
    /// An action that needs only its manifest declaration.
    Declared(&'a Action),
    /// A clone list, read for this run.
    CloneList(PreparedList<'a>),
}

/// Execute a prepared action.
///
/// `tree` is the tree the record came from. Symlink and copy actions resolve sources against
/// it, and a `run` action's command runs in its root; clone lists are already prepared. Each
/// action resolves its template fields with `fields` once, before it does anything else.
///
/// Return whether the action changed something: it installed or replaced a node, created its
/// directory, cloned or moved a checkout, or ran its command successfully.
pub(crate) fn dispatch(
    prepared: &PreparedAction<'_>,
    tree: Tree<&ItemId>,
    fields: &Resolver<'_, '_>,
    context: &RunContext,
) -> Result<bool, Error> {
    let action = match prepared {
        PreparedAction::CloneList(list) => {
            return git_clone_list::git_clone_list(list, fields, context);
        }
        PreparedAction::Declared(action) => action,
    };
    match action {
        Action::Symlink(action) => symlink::link(action, tree, fields, context),
        Action::SymlinkDir(action) => symlink::link_dir(action, tree, fields, context),
        Action::CreateDir(action) => create_dir::create_dir(action, fields, context),
        Action::Copy(action) => copy::copy(action, tree, fields, context),
        Action::CopyDir(action) => copy::copy_dir(action, tree, fields, context),
        Action::FetchFile(action) => fetch_file::fetch_file(action, fields, context),
        Action::FetchArchive(action) => fetch_archive::fetch_archive(action, fields, context),
        Action::GitClone(action) => git_clone::git_clone(action, fields, context),
        Action::Run(action) => run::run(action, tree, fields, context),
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
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use super::*;
    use crate::env::Environment;
    use crate::filesystem::mode::RunMode;
    use crate::location::{MachineRoots, Roots};
    use crate::manifest::condition::{Bindings, HostNamespaces};
    use crate::manifest::template::EnvAccess;
    use crate::output::Reporter;
    use crate::run::context::DestinationOptions;
    use crate::vars::scope::VarScope;

    /// Dispatch `declaration` as `Declared`, bypassing preparation to exercise
    /// the dispatch boundary's invariants.
    fn dispatch_declared(declaration: &str) {
        let tree = tempfile::tempdir().expect("a temporary root");
        let roots = Roots {
            home: tree.path().join("home"),
            batfiles_repo: tree.path().to_path_buf(),
            machine: MachineRoots {
                config_dir: tree.path().join("config"),
                cache_dir: tree.path().join("cache"),
            },
        };
        let reporter = Reporter::new(false);
        let context = RunContext::new(
            &roots,
            RunMode::Perform,
            Default::default(),
            DestinationOptions::default(),
            &reporter,
        )
        .expect("the roots resolve");
        let action = toml::from_str(declaration).expect("the declaration parses");

        let scope = Rc::new(VarScope::stack(
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            &[],
        ));
        let host =
            HostNamespaces::capture(&Environment::from_pairs(std::iter::empty::<(&str, &str)>()));
        let bindings = Bindings::new(&scope, &host);
        let fields = Resolver::new("the record", &bindings, EnvAccess::Allowed);

        let _ = dispatch(
            &PreparedAction::Declared(&action),
            Tree::Leaf,
            &fields,
            &context,
        );
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
