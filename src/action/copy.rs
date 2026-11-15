//! `copy` and `copy-dir`: seed destinations from a source node or its direct children.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::children::{ChildInstall, for_each_child};
use crate::error::Error;
use crate::filesystem::install::{self, ContentKind};
use crate::filesystem::paths;
use crate::manifest::actions::{CopyAction, CopyDirAction};
use crate::manifest::entry_filter::{EntryFilter, Verdict};
use crate::manifest::item::ItemId;
use crate::manifest::remote::Tree;
use crate::output::Verb;
use crate::run::context::RunContext;

/// The entries an [`EntryFilter`] selects under a source directory, by their path relative to
/// it, together with every directory holding one.
type SelectedPaths = BTreeSet<PathBuf>;

/// Carry out one `copy` action: one file or one directory, at one destination.
pub(super) fn copy(
    action: &CopyAction,
    tree: Tree<&ItemId>,
    context: &RunContext,
) -> Result<(), Error> {
    let source = context.source(tree, &action.source)?;
    let dest = context.destination(&action.dest);
    let kind = kind_of_source(&source)?;
    let Some(mut filter) = EntryFilter::new(action.include.as_ref(), action.exclude.as_ref())
    else {
        return seed(&source, kind, &dest, &Copied::All, context);
    };
    if let ContentKind::File = kind {
        return Err(Error::FilteredSourceIsAFile { path: source });
    }
    let selected = select(&source, &mut filter)?;
    seed(&source, kind, &dest, &Copied::root(&selected), context)?;
    filter.report_unmatched(&source.display().to_string(), context.reporter());
    Ok(())
}

/// Carry out one `copy-dir` action: one copy per direct child of a directory,
/// all of them in one destination directory.
pub(super) fn copy_dir(
    action: &CopyDirAction,
    tree: Tree<&ItemId>,
    context: &RunContext,
) -> Result<(), Error> {
    let source_dir = context.source_directory(tree, &action.source_dir)?;
    let dest_dir = context.destination(&action.dest_dir);
    paths::refuse_destination_inside_source(&source_dir, &dest_dir)?;

    let mut filter = EntryFilter::new(action.include.as_ref(), action.exclude.as_ref());
    let selected = match &mut filter {
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
            selected
                .as_ref()
                .is_none_or(|selected| selected.contains(Path::new(child)))
        },
        |source, dest| {
            let copied = match &selected {
                Some(selected) => Copied::Within {
                    selected,
                    at: source.file_name().map(PathBuf::from).unwrap_or_default(),
                },
                None => Copied::All,
            };
            seed(source, kind_of_child(source)?, dest, &copied, context)
        },
    )?;
    if let Some(filter) = &filter {
        filter.report_unmatched(&source_dir.display().to_string(), context.reporter());
    }
    Ok(())
}

/// Which of a copied directory's contents are copied.
enum Copied<'a> {
    /// All of them.
    All,
    /// Those in `selected`, where the directory is at `at` relative to the directory the
    /// paths were selected in.
    Within {
        selected: &'a SelectedPaths,
        at: PathBuf,
    },
}

impl<'a> Copied<'a> {
    /// What is copied of the directory the paths were selected in.
    fn root(selected: &'a SelectedPaths) -> Self {
        Self::Within {
            selected,
            at: PathBuf::new(),
        }
    }

    /// What is copied of `child`, a child of the directory this describes, or `None` where
    /// the child is not copied.
    fn of(&self, child: &Path) -> Option<Self> {
        match self {
            Self::All => Some(Self::All),
            Self::Within { selected, at } => {
                let at = at.join(child);
                selected
                    .contains(&at)
                    .then_some(Self::Within { selected, at })
            }
        }
    }
}

/// Decide every entry under `dir` against `filter`, descending into directories but not
/// through symlinks, and return what it selects. Reads the source in either run mode.
fn select(dir: &Path, filter: &mut EntryFilter) -> Result<SelectedPaths, Error> {
    let mut selected = SelectedPaths::new();
    select_under(dir, Path::new(""), filter, &mut selected)?;
    Ok(selected)
}

/// Add to `selected` what `filter` selects under `dir`, which is at `at` relative to the
/// directory selection started in, and return whether anything was added.
fn select_under(
    dir: &Path,
    at: &Path,
    filter: &mut EntryFilter,
    selected: &mut SelectedPaths,
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
            .map_err(Error::reading(&from))?
            .is_dir();
        let beneath = is_directory && select_under(&from, &path, filter, selected)?;
        if verdict == Verdict::Selected || beneath {
            selected.insert(path);
            added = true;
        }
    }
    Ok(added)
}

/// Seed `dest` with a copy of `source`, using its classified content kind and copying what
/// `copied` says of a directory.
fn seed(
    source: &Path,
    kind: ContentKind,
    dest: &Path,
    copied: &Copied,
    context: &RunContext,
) -> Result<(), Error> {
    let seed = install::SeedDescription {
        verb: Verb::Copy,
        origin: source.display().to_string(),
        source: Some(source),
    };
    match kind {
        ContentKind::File => install::seed_file(seed, dest, &context.seeding(), |into, staging| {
            copy_file(source, into, staging)
        }),
        ContentKind::Directory => {
            install::seed_directory(seed, dest, &context.seeding(), |staging| {
                copy_children(source, staging, copied)
            })
        }
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
    let found = fs::symlink_metadata(source).map_err(Error::reading(source))?;
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
    let mut from = fs::File::open(source).map_err(Error::reading(source))?;
    io::copy(&mut from, &mut into).map_err(Error::writing(built_at))?;
    mirror_permissions(source, built_at)
}

/// Copy what `copied` says of a source directory's children into a directory being built.
fn copy_children(source: &Path, built_at: &Path, copied: &Copied) -> Result<(), Error> {
    for child in paths::children_of(source)? {
        let Some(within) = copied.of(Path::new(&child)) else {
            continue;
        };
        let from = source.join(&child);
        let to = built_at.join(&child);
        match kind_of_child(&from)? {
            ContentKind::File => {
                copy_file(&from, create_new_file(&to)?, &to)?;
            }
            ContentKind::Directory => {
                fs::create_dir(&to).map_err(Error::writing(&to))?;
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
        .map_err(Error::writing(path))
}

/// Apply the source file or directory permissions to the copy.
fn mirror_permissions(source: &Path, built_at: &Path) -> Result<(), Error> {
    let found = fs::metadata(source).map_err(Error::reading(source))?;
    fs::set_permissions(built_at, found.permissions()).map_err(Error::writing(built_at))
}
