//! `copy` and `copy-dir`: seed destinations from a source node or its direct children.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::RunContext;
use super::children::{ChildInstall, for_each_child};
use crate::entry_filter::{EntryFilter, Verdict};
use crate::error::Error;
use crate::install::{self, ContentKind};
use crate::item::ItemId;
use crate::manifest::action::{CopyAction, CopyDirAction};
use crate::output::Verb;
use crate::paths;

/// The entries an [`EntryFilter`] selects under a source directory, by their path relative to
/// it, together with every directory holding one.
type Selection = BTreeSet<PathBuf>;

/// Carry out one `copy` action: one file or one directory, at one destination.
pub(super) fn copy(
    action: &CopyAction,
    remote: Option<&ItemId>,
    context: &RunContext,
) -> Result<(), Error> {
    let source = context.source(remote, &action.source)?;
    let dest = context.destination(&action.dest);
    let kind = kind_of_source(&source)?;
    let Some(mut filter) = EntryFilter::new(action.include.as_ref(), action.exclude.as_ref())
    else {
        return seed(&source, kind, &dest, &Scope::Everything, context);
    };
    if let ContentKind::File = kind {
        return Err(Error::FilteredSourceIsAFile { path: source });
    }
    let selection = select(&source, &mut filter)?;
    seed(&source, kind, &dest, &Scope::root(&selection), context)?;
    filter.report_unmatched(&source.display().to_string(), context.reporter());
    Ok(())
}

/// Carry out one `copy-dir` action: one copy per direct child of a directory,
/// all of them in one destination directory.
pub(super) fn copy_dir(
    action: &CopyDirAction,
    remote: Option<&ItemId>,
    context: &RunContext,
) -> Result<(), Error> {
    let source_dir = context.source_directory(remote, &action.source_dir)?;
    let dest_dir = context.destination(&action.dest_dir);
    paths::refuse_destination_inside_source(&source_dir, &dest_dir)?;

    let mut filter = EntryFilter::new(action.include.as_ref(), action.exclude.as_ref());
    let selection = match &mut filter {
        Some(filter) => Some(select(&source_dir, filter)?),
        None => None,
    };
    for_each_child(
        context,
        &ChildInstall {
            source_dir: &source_dir,
            dest_dir: &dest_dir,
            dot_prefix: action.dot_prefix,
            verb: Verb::Copy,
        },
        |child| {
            selection
                .as_ref()
                .is_none_or(|selection| selection.contains(Path::new(child)))
        },
        |source, dest| {
            let scope = match &selection {
                Some(selection) => Scope::Selected {
                    selection,
                    at: source.file_name().map(PathBuf::from).unwrap_or_default(),
                },
                None => Scope::Everything,
            };
            seed(source, kind_of_child(source)?, dest, &scope, context)
        },
    )?;
    if let Some(filter) = &filter {
        filter.report_unmatched(&source_dir.display().to_string(), context.reporter());
    }
    Ok(())
}

/// Which of a copied directory's contents are copied.
enum Scope<'a> {
    /// All of them.
    Everything,
    /// Those in `selection`, where the directory is at `at` relative to the selection's root.
    Selected {
        selection: &'a Selection,
        at: PathBuf,
    },
}

impl<'a> Scope<'a> {
    /// The scope of the directory a selection was made in.
    fn root(selection: &'a Selection) -> Self {
        Self::Selected {
            selection,
            at: PathBuf::new(),
        }
    }

    /// The scope of `child`, within the directory this is the scope of, or `None` where the
    /// child is not copied.
    fn of(&self, child: &Path) -> Option<Self> {
        match self {
            Self::Everything => Some(Self::Everything),
            Self::Selected { selection, at } => {
                let at = at.join(child);
                selection
                    .contains(&at)
                    .then_some(Self::Selected { selection, at })
            }
        }
    }
}

/// Decide every entry under `dir` against `filter`, descending into directories but not
/// through symlinks, and return what it selects. Reads the source in either run mode.
fn select(dir: &Path, filter: &mut EntryFilter) -> Result<Selection, Error> {
    let mut selection = Selection::new();
    select_under(dir, Path::new(""), filter, &mut selection)?;
    Ok(selection)
}

/// Add to `selection` what `filter` selects under `dir`, which is at `at` relative to the
/// selection's root, and return whether anything was added.
fn select_under(
    dir: &Path,
    at: &Path,
    filter: &mut EntryFilter,
    selection: &mut Selection,
) -> Result<bool, Error> {
    let mut added = false;
    for child in paths::children_of(dir)? {
        let path = at.join(&child);
        let verdict = filter.verdict(&path);
        if verdict == Verdict::Excluded {
            continue;
        }
        let from = dir.join(&child);
        let is_directory = fs::symlink_metadata(&from)
            .map_err(|error| Error::Read {
                path: from.clone(),
                source: error,
            })?
            .is_dir();
        let beneath = is_directory && select_under(&from, &path, filter, selection)?;
        if verdict == Verdict::Selected || beneath {
            selection.insert(path);
            added = true;
        }
    }
    Ok(added)
}

/// Seed `dest` with a copy of `source`, using its classified content kind and copying what
/// `scope` holds of a directory.
fn seed(
    source: &Path,
    kind: ContentKind,
    dest: &Path,
    scope: &Scope,
    context: &RunContext,
) -> Result<(), Error> {
    let seed = install::SeedDescription {
        verb: Verb::Copy,
        origin: source.display().to_string(),
        source: Some(source),
    };
    match kind {
        ContentKind::File => install::seed_file(seed, dest, context, |into, staging| {
            copy_file(source, into, staging)
        }),
        ContentKind::Directory => install::seed_directory(seed, dest, context, |staging| {
            copy_children(source, staging, scope)
        }),
    }
}

/// Classify a source the manifest named, following a final symlink.
fn kind_of_source(source: &Path) -> Result<ContentKind, Error> {
    let found = fs::metadata(source).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
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
    classify(&found, source)
}

/// Classify a copied directory child without following symlinks.
fn kind_of_child(source: &Path) -> Result<ContentKind, Error> {
    let found = fs::symlink_metadata(source).map_err(|error| Error::Read {
        path: source.to_path_buf(),
        source: error,
    })?;
    if found.is_symlink() {
        return Err(Error::SourceIsSymlink {
            path: source.to_path_buf(),
        });
    }
    classify(&found, source)
}

/// Classify metadata as a file or directory; reject other node types.
fn classify(found: &fs::Metadata, source: &Path) -> Result<ContentKind, Error> {
    if found.is_file() {
        Ok(ContentKind::File)
    } else if found.is_dir() {
        Ok(ContentKind::Directory)
    } else {
        Err(Error::SourceNotCopyable {
            path: source.to_path_buf(),
        })
    }
}

/// Write a source file's contents into the file already opened for it.
fn copy_file(source: &Path, mut into: fs::File, built_at: &Path) -> Result<(), Error> {
    let mut from = fs::File::open(source).map_err(|error| Error::Read {
        path: source.to_path_buf(),
        source: error,
    })?;
    io::copy(&mut from, &mut into).map_err(|error| Error::Write {
        path: built_at.to_path_buf(),
        source: error,
    })?;
    mirror_permissions(source, built_at)
}

/// Copy what `scope` holds under a source directory into a directory being built.
fn copy_children(source: &Path, built_at: &Path, scope: &Scope) -> Result<(), Error> {
    for child in paths::children_of(source)? {
        let Some(within) = scope.of(Path::new(&child)) else {
            continue;
        };
        let from = source.join(&child);
        let to = built_at.join(&child);
        match kind_of_child(&from)? {
            ContentKind::File => {
                copy_file(&from, create_new_file(&to)?, &to)?;
            }
            ContentKind::Directory => {
                fs::create_dir(&to).map_err(|error| Error::Write {
                    path: to.clone(),
                    source: error,
                })?;
                copy_children(&from, &to, &within)?;
            }
        }
    }
    // Apply directory permissions after filling it; the source may be read-only.
    mirror_permissions(source, built_at)
}

/// Create one of the files inside a copy being built, failing rather than
/// truncating if the path is taken.
fn create_new_file(path: &Path) -> Result<fs::File, Error> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| Error::Write {
            path: path.to_path_buf(),
            source: error,
        })
}

/// Apply the source file or directory permissions to the copy.
fn mirror_permissions(source: &Path, built_at: &Path) -> Result<(), Error> {
    let found = fs::metadata(source).map_err(|error| Error::Read {
        path: source.to_path_buf(),
        source: error,
    })?;
    fs::set_permissions(built_at, found.permissions()).map_err(|error| Error::Write {
        path: built_at.to_path_buf(),
        source: error,
    })
}
