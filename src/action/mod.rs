//! Carrying one `[[actions]]` record out.
//!
//! Resolving what a `source` and a `dest` mean happens here rather than inside
//! the records, so there is one place that decides. What a path means, and what
//! is already *at* one, is [`crate::paths`]': every action asks it the same
//! questions and they are not asked twice.
//!
//! `create-dir` is here rather than in a file of its own — it is the dispatch
//! plus two lines, and the two directory-wide actions each live with the
//! single-item action they repeat.

mod copy;
mod symlink;

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::location::Roots;
use crate::manifest::action::{Action, CreateDirAction};
use crate::output::Reporter;
use crate::paths::{self, Directory, Repository};

/// What every action is carried out against: the two roots it can reach, and
/// somewhere to say what it did.
///
/// The roots are anchored once, when this is built, because a symlink stores
/// the target it is given: a relative one is read back relative to the link's
/// own directory rather than to wherever batfiles happened to be run, so a
/// relative `--batfiles-dir` would otherwise produce a link that points nowhere
/// and a next run that calls it correct.
pub(crate) struct Context<'a> {
    repository: Repository,
    home: PathBuf,
    reporter: &'a Reporter,
}

impl<'a> Context<'a> {
    /// Anchor the resolved roots, once, for every action in a run.
    pub fn new(roots: &Roots, reporter: &'a Reporter) -> Result<Self, Error> {
        Ok(Self {
            repository: Repository::at(&roots.batfiles_dir)?,
            home: paths::anchor(&roots.home)?,
            reporter,
        })
    }

    /// An action's `source`, resolved against the repository that declared it.
    ///
    /// The manifest has already settled what a source may say, so this expects
    /// one that is relative and lands strictly inside the repository, and
    /// checks only the rule needing a filesystem: the path has to exist. The
    /// repository is already anchored, so the result is too — this is a path
    /// batfiles writes into a link, not one it classifies, so it keeps the
    /// spelling the user chose.
    ///
    /// The one place a repository path is resolved, which 6.3 widens to take
    /// `@remote/path` (`guidance.md`, "Seams the late slices need").
    fn source(&self, source: &str) -> Result<PathBuf, Error> {
        let resolved = paths::normalize(&self.repository.path().join(source));

        // Presence, not reachability: a source that is itself a dangling
        // symlink is there, and linking to it is what the repository asked for.
        match fs::symlink_metadata(&resolved) {
            Ok(_) => Ok(resolved),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(Error::SourceMissing { path: resolved })
            }
            Err(error) => Err(Error::Read {
                path: resolved,
                source: error,
            }),
        }
    }

    /// An action's `dest`, resolved against this run's selected home.
    fn destination(&self, dest: &str) -> PathBuf {
        destination(&self.home, dest)
    }

    /// The repository an action installs from, for the one question that needs
    /// it: whether a symlink already at a destination points into it.
    fn repository(&self) -> &Repository {
        &self.repository
    }

    fn reporter(&self) -> &Reporter {
        self.reporter
    }

    /// [`paths::ensure_directory`], plus the report it has no reporter to make.
    ///
    /// Every caller says the same thing about the same directory, because they
    /// are all the same operation: `create-dir` asks for one outright, and the
    /// two directory-wide actions need one to install into. A directory that
    /// appeared in the home is worth a line either way.
    fn ensure_directory(&self, dir: &Path) -> Result<(), Error> {
        match paths::ensure_directory(dir)? {
            Directory::Created => self.reporter.info(&format!("created {}", dir.display())),
            Directory::AlreadyThere => self
                .reporter
                .detail(1, &format!("unchanged {}", dir.display())),
        }
        Ok(())
    }
}

/// Carry out one action, whichever kind it is.
pub(crate) fn run(action: &Action, context: &Context) -> Result<(), Error> {
    match action {
        Action::Symlink(action) => symlink::link(action, context),
        Action::SymlinkDir(action) => symlink::link_dir(action, context),
        Action::CreateDir(action) => create_dir(action, context),
        Action::Copy(action) => copy::copy(action, context),
        Action::CopyDir(action) => copy::copy_dir(action, context),
    }
}

/// Carry out one `create-dir` action: the whole of it is one directory.
///
/// No source, and no platform check — every platform batfiles builds for makes
/// directories. What is at the destination is [`paths::ensure_directory`]'s
/// question rather than [`paths::Occupant::at`]'s: this action replaces
/// nothing, so a directory already there, however it is reached, is what was
/// asked for.
fn create_dir(action: &CreateDirAction, context: &Context) -> Result<(), Error> {
    context.ensure_directory(&context.destination(&action.dest))
}

/// A `dest` resolved against a selected home.
///
/// `~` and a relative path both resolve from the selected home rather than from
/// an independently discovered one, and an absolute path is used as written.
/// None of this makes the home a boundary: a destination may deliberately point
/// outside it, and only `--home-dir` decides what "home" means.
///
/// Infallible: the manifest has already refused an empty `dest` and a `~other`,
/// so what reaches this is `~`, `~/…`, or an ordinary path.
fn destination(home: &Path, dest: &str) -> PathBuf {
    let path = match dest.strip_prefix('~') {
        // `~` leaves nothing and `~/…` leaves a separator, so trimming covers
        // both without a second arm.
        Some(rest) => home.join(rest.trim_start_matches('/')),
        // `join` returns an absolute `dest` unchanged, which is the rule for
        // one, so the relative and absolute cases are the same line.
        None => home.join(dest),
    };
    paths::normalize(&path)
}

/// An action's `source-dir`, resolved and confirmed to be one.
///
/// Followed, unlike a destination: a `source-dir` that is a symlink to a
/// directory inside the repository is something the repository put there
/// deliberately, and its children are what the action is asking for.
fn source_directory(context: &Context, source_dir: &str) -> Result<PathBuf, Error> {
    let resolved = context.source(source_dir)?;
    if fs::metadata(&resolved)
        .map_err(|source| Error::Read {
            path: resolved.clone(),
            source,
        })?
        .is_dir()
    {
        Ok(resolved)
    } else {
        Err(Error::SourceNotADirectory { path: resolved })
    }
}

/// Do one action's work once per direct child of a source directory, all of it
/// into one destination directory.
///
/// The shape both `-dir` actions are: make the destination, then install every
/// direct child of the source into it under the name [`installed_name`] gives.
/// Not recursive, in either of them — a child that is itself a directory is one
/// thing installed, and what is inside it is reached through what was installed
/// rather than decided entry by entry.
///
/// `verb` is the one word the two differ by, in the one line they both report.
fn install_children(
    context: &Context,
    source_dir: &Path,
    dest_dir: &Path,
    dot_prefix: bool,
    verb: &str,
    install_one: impl Fn(&Path, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    context.ensure_directory(dest_dir)?;

    let children = paths::children_of(source_dir)?;
    if children.is_empty() {
        context.reporter().detail(
            1,
            &format!("no children to {verb} in {}", source_dir.display()),
        );
    }
    for child in children {
        let installed = installed_name(&child, dot_prefix)?;
        install_one(&source_dir.join(&child), &dest_dir.join(installed))?;
    }
    Ok(())
}

/// What a child of a `source-dir` is called once installed.
///
/// The dot-prefix rule and its one refusal, shared by both actions that install
/// a directory's children: a child already starting with `.` would arrive as
/// `..name`, which is a legal file name and never the one that was meant.
fn installed_name(child: &OsString, dot_prefix: bool) -> Result<OsString, Error> {
    if !dot_prefix {
        return Ok(child.clone());
    }
    // Lossy only where a name is not UTF-8, and only for the refusal's message;
    // the paths themselves are joined from the original `OsString`.
    let name = child.to_string_lossy();
    if name.starts_with('.') {
        return Err(Error::DotPrefixOnDotfile {
            child: name.into_owned(),
        });
    }
    let mut dotted = OsString::from(".");
    dotted.push(child);
    Ok(dotted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/user")
    }

    fn dest_of(dest: &str) -> PathBuf {
        destination(&home(), dest)
    }

    // What a `dest` may say is checked by `manifest`, and tested there. These
    // cover the other half: what an accepted one resolves to.

    #[test]
    fn a_destination_resolves_against_the_selected_home() {
        assert_eq!(dest_of("~/.zshrc"), home().join(".zshrc"));
        assert_eq!(dest_of(".zshrc"), home().join(".zshrc"));
        assert_eq!(dest_of("~"), home());
        assert_eq!(dest_of("/etc/hosts"), PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn a_destination_may_deliberately_leave_the_home() {
        // The home is a base, not a boundary. Someone linking into a sibling
        // directory is expressing intent, not making a mistake.
        assert_eq!(dest_of("~/../shared/rc"), PathBuf::from("/home/shared/rc"));
    }
}
