//! `sync`: bring the home directory to the state a repository's actions
//! describe.
//!
//! One loop over one ordered list, each action inspecting the filesystem as the
//! previous one left it. Resolving what a `source` and a `dest` mean happens
//! here rather than inside the records, so there is one place that decides.
//!
//! What a path means, and what is already *at* one, is [`crate::paths`]':
//! every action asks it the same questions and they are not asked twice.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// Making a symlink is the one platform-specific call here. Windows needs
// `symlink_file` against `symlink_dir` and a privilege check, with no CI runner
// and no user to prove it against, so it is not built —
// `require_symlink_support` refuses the action instead, and the stand-in below
// keeps the crate compiling there.
#[cfg(unix)]
use std::os::unix::fs::symlink;

use crate::error::Error;
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::manifest::action::{
    Action, CopyAction, CopyDirAction, CreateDirAction, SymlinkAction, SymlinkDirAction,
};
use crate::output::Reporter;
use crate::paths::{self, Directory, Occupant, Repository};

#[cfg(not(unix))]
fn symlink(_target: &Path, _dest: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Execute every action in declaration order, stopping at the first failure.
///
/// The two roots an action can reach are anchored once, here, because a symlink
/// stores the target it is given: a relative one is read back relative to the
/// link's own directory rather than to wherever batfiles happened to be run,
/// so a relative `--batfiles-dir` would otherwise produce a link that points
/// nowhere and a next run that calls it correct.
pub(crate) fn sync(roots: &Roots, manifest: &Manifest, reporter: &Reporter) -> Result<(), Error> {
    let repository = Repository::at(&roots.batfiles_dir)?;
    let home = paths::anchor(&roots.home)?;
    for action in &manifest.actions {
        match action {
            Action::Symlink(action) => link(action, &repository, &home, reporter)?,
            Action::SymlinkDir(action) => link_dir(action, &repository, &home, reporter)?,
            Action::CreateDir(action) => create_dir(action, &home, reporter)?,
            Action::Copy(action) => copy(action, &repository, &home, reporter)?,
            Action::CopyDir(action) => copy_dir(action, &repository, &home, reporter)?,
        }
    }
    Ok(())
}

/// Refuse an action type that makes symlinks where batfiles cannot make one.
///
/// Called on sight, before anything is inspected or removed. Repairing a link
/// deletes the old one first, so a platform check made at the moment of writing
/// would fail with the destination already gone.
fn require_symlink_support(action_type: &'static str) -> Result<(), Error> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err(Error::Unsupported { action_type })
    }
}

/// Carry out one `symlink` action: the whole of it is one link.
fn link(
    action: &SymlinkAction,
    repository: &Repository,
    home: &Path,
    reporter: &Reporter,
) -> Result<(), Error> {
    require_symlink_support("symlink")?;

    let target = resolve_source(repository, &action.source)?;
    let dest = resolve_destination(home, &action.dest);
    link_one(&target, &dest, repository, reporter)
}

/// Carry out one `symlink-dir` action: one link per direct child of a
/// directory, all of them in one destination directory.
///
/// Not recursive. A child that is itself a directory becomes one link like any
/// other, so what is under it is reached through that link and a file added
/// there later needs no further sync.
fn link_dir(
    action: &SymlinkDirAction,
    repository: &Repository,
    home: &Path,
    reporter: &Reporter,
) -> Result<(), Error> {
    require_symlink_support("symlink-dir")?;

    let source_dir = resolve_source(repository, &action.source_dir)?;
    // Followed, unlike a destination: a `source-dir` that is a symlink to a
    // directory inside the repository is something the repository put there
    // deliberately, and its children are what the action is asking for.
    if !fs::metadata(&source_dir)
        .map_err(|source| Error::Read {
            path: source_dir.clone(),
            source,
        })?
        .is_dir()
    {
        return Err(Error::SourceNotADirectory { path: source_dir });
    }

    let dest_dir = resolve_destination(home, &action.dest_dir);
    ensure_directory(&dest_dir, reporter)?;

    let children = children_of(&source_dir)?;
    if children.is_empty() {
        reporter.detail(
            1,
            &format!("no children to link in {}", source_dir.display()),
        );
    }
    for child in children {
        let installed = installed_name(&child, action.dot_prefix)?;
        link_one(
            &source_dir.join(&child),
            &dest_dir.join(installed),
            repository,
            reporter,
        )?;
    }
    Ok(())
}

/// Carry out one `create-dir` action: the whole of it is one directory.
///
/// No source, and no platform check — every platform batfiles builds for makes
/// directories. What is at the destination is [`ensure_directory`]'s question
/// rather than [`Occupant::at`]'s: this action replaces nothing, so a directory
/// already there, however it is reached, is what was asked for.
fn create_dir(action: &CreateDirAction, home: &Path, reporter: &Reporter) -> Result<(), Error> {
    let dest = resolve_destination(home, &action.dest);
    ensure_directory(&dest, reporter)
}

/// Carry out one `copy` action: one file or one directory, at one destination.
///
/// A seed, so the destination decides everything: something there means the
/// action is done, and a directory source is installed whole or not at all.
fn copy(
    action: &CopyAction,
    repository: &Repository,
    home: &Path,
    reporter: &Reporter,
) -> Result<(), Error> {
    let source = resolve_source(repository, &action.source)?;
    let dest = resolve_destination(home, &action.dest);
    seed(&source, named_kind(&source)?, &dest, reporter)
}

/// Carry out one `copy-dir` action: one copy per direct child of a directory,
/// all of them in one destination directory.
///
/// Not recursive, for the reason `symlink-dir` is not: a child that is itself a
/// directory is one thing installed, whole where nothing is there and untouched
/// where something is. Nothing decides entry by entry inside a child, so a
/// directory the user already has is never seeded into.
fn copy_dir(
    action: &CopyDirAction,
    repository: &Repository,
    home: &Path,
    reporter: &Reporter,
) -> Result<(), Error> {
    let source_dir = resolve_source(repository, &action.source_dir)?;
    // Followed, like `symlink-dir`'s: the manifest named this path, and naming
    // a directory through a link the repository stores is naming that
    // directory.
    if !fs::metadata(&source_dir)
        .map_err(|source| Error::Read {
            path: source_dir.clone(),
            source,
        })?
        .is_dir()
    {
        return Err(Error::SourceNotADirectory { path: source_dir });
    }

    let dest_dir = resolve_destination(home, &action.dest_dir);
    // Before the destination is created, because creating it inside the source
    // is what puts it in the list of children about to be copied. Each child is
    // checked again on its own; this one names the two directories the manifest
    // wrote, which is what the author can act on.
    refuse_destination_inside_source(&source_dir, &dest_dir)?;
    ensure_directory(&dest_dir, reporter)?;

    let children = children_of(&source_dir)?;
    if children.is_empty() {
        reporter.detail(
            1,
            &format!("no children to copy in {}", source_dir.display()),
        );
    }
    for child in children {
        let installed = installed_name(&child, action.dot_prefix)?;
        let source = source_dir.join(&child);
        // Each child is a node found rather than a path the manifest wrote, so
        // it is classified without following anything.
        seed(
            &source,
            found_kind(&source)?,
            &dest_dir.join(installed),
            reporter,
        )?;
    }
    Ok(())
}

/// Install one thing where nothing is, or keep what is there, and say which.
///
/// The whole of the missing-only rule as a user sees it: `copy` reaches this
/// once for its `dest`, and `copy-dir` once per child. What occupies a
/// destination is never examined, because nothing here would replace it
/// whatever it turned out to be.
fn seed(source: &Path, kind: Copyable, dest: &Path, reporter: &Reporter) -> Result<(), Error> {
    // Asked before the copy so the ordinary case — everything already seeded —
    // costs one call and copies nothing. The answer that decides is the one
    // taken when the copy is published.
    let installed = if paths::occupied(dest)? {
        false
    } else {
        // After that, not before: this refuses a destination the copy would
        // descend into, which is only a question when there is going to be a
        // copy. Asked first, it turns a destination that is merely *occupied*
        // — by a link of the user's own resolving into the source, say — into
        // an error, and a seed does not fail on an occupied destination.
        if let Copyable::Directory = kind {
            refuse_destination_inside_source(source, dest)?;
        }
        create_parents(dest)?;
        install(source, kind, dest, reporter)?
    };

    if installed {
        reporter.info(&format!(
            "copied {} from {}",
            dest.display(),
            source.display()
        ));
    } else {
        reporter.detail(1, &format!("kept {}", dest.display()));
    }
    Ok(())
}

/// Refuse a destination that lands inside the directory being copied.
///
/// Copying a directory into itself has no reading worth honoring, and it does
/// not simply fail: the destination becomes a child of the source, enumerating
/// the source finds it, and the copy descends into what it is writing until the
/// filesystem refuses a longer path — having written a deep tree into the
/// repository on the way. Judged by where the two resolve rather than how they
/// are spelled, since a destination can reach the source by a route that does
/// not look like it (`guidance.md`, rule 14).
fn refuse_destination_inside_source(source: &Path, dest: &Path) -> Result<(), Error> {
    let source = paths::resolved(source);
    if paths::intended(dest).starts_with(&source) {
        return Err(Error::DestinationInsideSource {
            copied: source,
            dest: dest.to_path_buf(),
        });
    }
    Ok(())
}

/// Build a copy beside its destination, then move it in.
///
/// **Nothing is ever at the destination until the copy is whole.** Not a
/// partial copy, and not a placeholder standing in for one: the destination
/// stays absent, and the last thing this does is one rename that puts a
/// finished copy there.
///
/// That is the only arrangement an *interrupted* run survives. A run that fails
/// returns an error and can tidy up after itself; a run that is killed returns
/// nothing and tidies nothing, and whatever it had put at the destination is
/// still there afterwards — a truncated file or an empty placeholder, either of
/// which the next run finds, keeps, and reports success over. Cleanup cannot be
/// what correctness rests on, and not only because it may not run: taking back
/// a copied tree needs write permission on every directory in it, and the copy
/// carries the source's permissions, so one read-only directory is enough to
/// make a copy batfiles can no longer remove.
///
/// Reports whether it installed: a destination taken while the copy was being
/// made is left alone, like one that was taken before it started.
fn install(source: &Path, kind: Copyable, dest: &Path, reporter: &Reporter) -> Result<bool, Error> {
    let staging = staging_path(dest);
    // Created before anything else can fail, so that everything after it is
    // working on a node this run made. Cleanup that runs on a path this run did
    // not create is how a copy comes to delete somebody's data: the staging
    // path is predictable, and `remove_dir_all` on one that was already there
    // takes the tree with it.
    let staged = create_staging(kind, &staging)?;

    let installed = fill(source, staged, &staging).and_then(|()| publish(&staging, kind, dest));

    // Whatever happened, the staging path is not wanted: a successful rename
    // has already consumed it, a successful link has left a second name for it,
    // and a failure has left a copy that is not going anywhere. Best-effort,
    // and allowed to fail — what survives is beside the destination rather than
    // at it, so the next run copies again rather than mistaking it for
    // finished.
    discard(&staging, kind, reporter);
    installed
}

/// Create the node the copy is built on, and nothing more.
///
/// A staging path that is already taken belongs to somebody — most likely an
/// earlier run of batfiles, but that is a guess, and acting on it would mean
/// deleting a path this run did not create. So it is named and the action
/// stops.
fn create_staging(kind: Copyable, staging: &Path) -> Result<Staged, Error> {
    create_closed(kind, staging).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            Error::StagingPathTaken {
                path: staging.to_path_buf(),
            }
        } else {
            Error::Write {
                path: staging.to_path_buf(),
                source: error,
            }
        }
    })
}

/// The staging node this run created, ready to receive the copy.
enum Staged {
    File(fs::File),
    Directory,
}

/// Create a staging node no one but its owner can reach into.
///
/// The copy's real permissions are the source's, and they are set once it is
/// whole rather than up front — a source directory its owner cannot write into
/// would otherwise lock batfiles out of the copy it is still filling
/// ([`mirror_permissions`]). Creating with the default in the meantime is what
/// that used to mean: a copy of a `0600` file readable by anyone for as long as
/// the copy ran, and, since an interrupted run leaves its staging node behind
/// deliberately, for as long after it as nobody noticed.
///
/// Starting closed and widening at the end costs nothing and covers the whole
/// tree: everything written under a staging directory is reached through it, so
/// one restrictive mode on the root is enough while the copy is in progress.
#[cfg(unix)]
fn create_closed(kind: Copyable, staging: &Path) -> io::Result<Staged> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    match kind {
        Copyable::File => fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(staging)
            .map(Staged::File),
        Copyable::Directory => fs::DirBuilder::new()
            .mode(0o700)
            .create(staging)
            .map(|()| Staged::Directory),
    }
}

/// Where a mode means something other than it does on unix, this is ordinary
/// exclusive creation: the permissions batfiles carries across are the unix
/// ones, and there is nothing here to narrow.
#[cfg(not(unix))]
fn create_closed(kind: Copyable, staging: &Path) -> io::Result<Staged> {
    match kind {
        Copyable::File => create_new(staging).map(Staged::File),
        Copyable::Directory => fs::create_dir(staging).map(|()| Staged::Directory),
    }
}

/// Make the copy itself, at the path it is built on the way to its destination.
fn fill(source: &Path, staged: Staged, staging: &Path) -> Result<(), Error> {
    match staged {
        Staged::File(into) => copy_file(source, into, staging),
        Staged::Directory => copy_children(source, staging),
    }
}

/// Move a finished copy to its destination, or report that the destination was
/// taken while it was being made.
///
/// A file is published by linking it, which is the one operation the standard
/// library offers that *refuses* to replace: it fails if the destination
/// exists. Renaming has no such promise, so a destination that appeared during
/// a long copy would be overwritten by one — which is content batfiles did not
/// create, and rule 13 does not stop applying because another process was
/// quick.
///
/// A directory has no linkable equivalent and no portable no-replace rename
/// (`renameat2` on Linux, `renamex_np` on macOS, neither on Windows), so it is
/// checked again immediately before the rename. That narrows what can be
/// replaced to a directory created between two adjacent calls, and a rename
/// only replaces an *empty* directory — anything holding content fails the
/// rename instead. No content is at risk either way.
fn publish(staging: &Path, kind: Copyable, dest: &Path) -> Result<bool, Error> {
    if let Copyable::File = kind {
        match fs::hard_link(staging, dest) {
            Ok(()) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
            // Not every filesystem has links. Where there are none, the rename
            // below is what is left.
            Err(_) => {}
        }
    }
    if paths::occupied(dest)? {
        return Ok(false);
    }
    fs::rename(staging, dest).map_err(|error| Error::Write {
        path: dest.to_path_buf(),
        source: error,
    })?;
    Ok(true)
}

/// Remove a staging node this run created, saying so if it cannot.
///
/// Only ever called on a path [`create_staging`] made, which is what makes it
/// safe. A path that is already gone is the ordinary case after a rename.
fn discard(staging: &Path, kind: Copyable, reporter: &Reporter) {
    let removed = match kind {
        Copyable::File => fs::remove_file(staging),
        Copyable::Directory => fs::remove_dir_all(staging),
    };
    match removed {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => reporter.warn(&format!(
            "could not remove the incomplete copy at {}: {error}",
            staging.display()
        )),
    }
}

/// Create a file, failing rather than truncating if the path is taken.
fn create_new(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// [`create_new`], for the paths inside a copy being built.
///
/// Those are all under the staging node this run just made, so a name already
/// taken there is a failure rather than something to keep — keeping one would
/// publish an incomplete copy as a finished one.
fn create_new_file(path: &Path) -> Result<fs::File, Error> {
    create_new(path).map_err(|error| Error::Write {
        path: path.to_path_buf(),
        source: error,
    })
}

/// Where a copy is built while it is still incomplete.
///
/// Beside the destination, so the move into place stays within one filesystem.
/// The name is fixed rather than carrying a process id: batfiles will not
/// remove what it did not create, so a copy left behind by an interrupted run
/// stops the next one with a diagnostic naming the path. That is the point — a
/// name that varied would quietly accumulate leftovers instead, and clearing
/// one to get out of the way is the user's call, not batfiles'.
fn staging_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".batfiles-incomplete");
    dest.with_file_name(name)
}

/// What `copy` reproduces. Nothing else is installed by copying it.
#[derive(Debug, Clone, Copy)]
enum Copyable {
    File,
    Directory,
}

/// Classify a source the manifest named, following a final symlink.
///
/// Naming a thing and reproducing one are different questions, and every
/// action's source resolves through a link — that is how a repository points at
/// something it stores under another name.
fn named_kind(source: &Path) -> Result<Copyable, Error> {
    let found = fs::metadata(source).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            // Reachability, not presence: `resolve_source` already found
            // something here, so this is a link whose target is gone.
            Error::SourceMissing {
                path: source.to_path_buf(),
            }
        } else {
            Error::Read {
                path: source.to_path_buf(),
                source: error,
            }
        }
    })?;
    kind_of(&found, source)
}

/// Classify a node found inside a directory being copied, following nothing.
///
/// A symlink here is refused rather than followed. `copy` reproduces nodes, and
/// a symlink is not one it will reproduce: copying what it reaches silently
/// turns a link the repository chose into a detached file, and recreating it
/// re-reads a relative target from a directory it is no longer in. Refusing is
/// the answer that can be changed later without changing what a working
/// manifest does today.
fn found_kind(source: &Path) -> Result<Copyable, Error> {
    let found = fs::symlink_metadata(source).map_err(|error| Error::Read {
        path: source.to_path_buf(),
        source: error,
    })?;
    if found.is_symlink() {
        return Err(Error::SourceIsSymlink {
            path: source.to_path_buf(),
        });
    }
    kind_of(&found, source)
}

fn kind_of(found: &fs::Metadata, source: &Path) -> Result<Copyable, Error> {
    if found.is_file() {
        Ok(Copyable::File)
    } else if found.is_dir() {
        Ok(Copyable::Directory)
    } else {
        Err(Error::SourceNotCopyable {
            path: source.to_path_buf(),
        })
    }
}

/// Write a source file's contents into the file already opened for it.
///
/// Not [`fs::copy`], which opens the destination itself and would truncate
/// whatever it found. The file is handed in already created exclusively, so the
/// only thing this can write into is one that did not exist a moment ago. The
/// permissions [`fs::copy`] would have carried are set here instead, which is
/// also where a directory gets them.
fn copy_file(source: &Path, mut into: fs::File, dest: &Path) -> Result<(), Error> {
    let mut from = fs::File::open(source).map_err(|error| Error::Read {
        path: source.to_path_buf(),
        source: error,
    })?;
    io::copy(&mut from, &mut into).map_err(|error| Error::Write {
        path: dest.to_path_buf(),
        source: error,
    })?;
    mirror_permissions(source, dest)
}

/// Copy everything under a source directory into a directory being built.
///
/// What it walks, it reproduces, and every node is new: this is inside the
/// staging directory, which nothing else knows about, so a path already taken
/// is a failure rather than a thing to keep. Keeping one here would publish an
/// incomplete copy as a finished one, which is the opposite of what staging is
/// for.
fn copy_children(source: &Path, dest: &Path) -> Result<(), Error> {
    for child in children_of(source)? {
        let from = source.join(&child);
        let to = dest.join(&child);
        match found_kind(&from)? {
            Copyable::File => {
                copy_file(&from, create_new_file(&to)?, &to)?;
            }
            Copyable::Directory => {
                fs::create_dir(&to).map_err(|error| Error::Write {
                    path: to.clone(),
                    source: error,
                })?;
                copy_children(&from, &to)?;
            }
        }
    }
    // Last, and not at creation: a source directory its owner cannot write into
    // would otherwise lock batfiles out of the copy it is still filling.
    mirror_permissions(source, dest)
}

/// Give a copied file or directory the permissions of what it was copied from,
/// so an executable arrives executable and a private directory arrives private.
///
/// Ownership is not copied; the copy belongs to whoever ran the command.
/// Directories batfiles creates only to *reach* a destination are not these,
/// and keep the platform default: they correspond to nothing in the repository.
fn mirror_permissions(source: &Path, dest: &Path) -> Result<(), Error> {
    let found = fs::metadata(source).map_err(|error| Error::Read {
        path: source.to_path_buf(),
        source: error,
    })?;
    fs::set_permissions(dest, found.permissions()).map_err(|error| Error::Write {
        path: dest.to_path_buf(),
        source: error,
    })
}

/// [`paths::ensure_directory`], plus the report it has no reporter to make.
///
/// Both callers say the same thing about the same directory, because the two
/// are the same operation: `create-dir` asks for one outright, and
/// `symlink-dir` needs one to link into. A directory that appeared in the home
/// is worth a line either way.
fn ensure_directory(dir: &Path, reporter: &Reporter) -> Result<(), Error> {
    match paths::ensure_directory(dir)? {
        Directory::Created => reporter.info(&format!("created {}", dir.display())),
        Directory::AlreadyThere => reporter.detail(1, &format!("unchanged {}", dir.display())),
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

/// Create the directories a destination sits in, if they are not there.
///
/// These correspond to nothing in the repository — they exist only so the
/// destination can, so they take the platform default rather than any source's
/// permissions.
fn create_parents(dest: &Path) -> Result<(), Error> {
    let Some(parent) = dest.parent() else {
        return Ok(());
    };
    fs::create_dir_all(parent).map_err(|source| Error::Write {
        path: parent.to_path_buf(),
        source,
    })
}

/// The direct children of a directory, sorted by name.
///
/// Sorted because `read_dir` yields whatever order the filesystem holds, and an
/// action that reports its work in a different order on every machine is one
/// nobody can diff.
fn children_of(dir: &Path) -> Result<Vec<OsString>, Error> {
    let read = fs::read_dir(dir).map_err(|source| Error::Read {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut names = Vec::new();
    for entry in read {
        let entry = entry.map_err(|source| Error::Read {
            path: dir.to_path_buf(),
            source,
        })?;
        names.push(entry.file_name());
    }
    names.sort();
    Ok(names)
}

/// Create one symlink, repair it, or leave it alone.
///
/// What is at the destination, and whether it is batfiles' to replace, is
/// [`Occupant::at`]'s answer — see that module for why the question cannot be
/// asked of the written path. This decides only what a `symlink` does with each
/// answer.
///
/// Both action types end here, one call per link either of them installs, so
/// rule 13 is applied in one place regardless of how many links an action is.
fn link_one(
    target: &Path,
    dest: &Path,
    repository: &Repository,
    reporter: &Reporter,
) -> Result<(), Error> {
    match Occupant::at(dest, repository)? {
        // Compared in resolved form on both sides. A link written by an earlier
        // run holds the anchored spelling, which is the same place by a
        // different name wherever a root contains a symlink — and calling that
        // stale would relink it, and every link like it, on every run.
        Occupant::Owned { points_at, .. } if points_at == paths::resolved(target) => {
            reporter.detail(1, &format!("unchanged {}", dest.display()));
        }
        Occupant::Owned { written, .. } => {
            remove(dest)?;
            create(target, dest)?;
            reporter.info(&format!(
                "relinked {} -> {} (was {})",
                dest.display(),
                target.display(),
                written.display()
            ));
        }
        Occupant::Vacant => {
            create_parents(dest)?;
            create(target, dest)?;
            reporter.info(&format!(
                "linked {} -> {}",
                dest.display(),
                target.display()
            ));
        }
        Occupant::Unmanaged(found) => {
            return Err(Error::DestinationExists {
                path: dest.to_path_buf(),
                found,
            });
        }
    }
    Ok(())
}

/// An action's `source`, resolved against the repository that declared it.
///
/// The manifest has already settled what a source may say, so this expects one
/// that is relative and lands strictly inside the repository, and checks only
/// the rule needing a filesystem: the path has to exist. The repository is
/// already anchored, so the result is too — this is a path batfiles writes into
/// a link, not one it classifies, so it keeps the spelling the user chose.
///
/// The one place a repository path is resolved, which 6.3 widens to take
/// `@remote/path` (`guidance.md`, "Seams the late slices need").
fn resolve_source(repository: &Repository, source: &str) -> Result<PathBuf, Error> {
    let resolved = paths::normalize(&repository.path().join(source));

    // Presence, not reachability: a source that is itself a dangling symlink is
    // there, and linking to it is what the repository asked for.
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

/// An action's `dest`, resolved against the selected home.
///
/// `~` and a relative path both resolve from the selected home rather than from
/// an independently discovered one, and an absolute path is used as written.
/// None of this makes the home a boundary: a destination may deliberately point
/// outside it, and only `--home-dir` decides what "home" means.
///
/// Infallible: the manifest has already refused an empty `dest` and a `~other`,
/// so what reaches this is `~`, `~/…`, or an ordinary path.
fn resolve_destination(home: &Path, dest: &str) -> PathBuf {
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

fn create(target: &Path, dest: &Path) -> Result<(), Error> {
    symlink(target, dest).map_err(|source| Error::Write {
        path: dest.to_path_buf(),
        source,
    })
}

fn remove(dest: &Path) -> Result<(), Error> {
    fs::remove_file(dest).map_err(|source| Error::Write {
        path: dest.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/user")
    }

    fn dest_of(dest: &str) -> PathBuf {
        resolve_destination(&home(), dest)
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

    // Composing a path, and classifying what is already at one, moved to
    // `paths`, and their tests went with them.
}
