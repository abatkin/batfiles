//! Validate manifest values without filesystem access.
//! [`Manifest::validate`](super::Manifest::validate) applies cross-record rules. See [manifest
//! validation](../../docs/repoformat.md#reading-the-manifest).

use std::collections::BTreeMap;
use std::path::{Component, Path};

use thiserror::Error;

use super::DeclarationName;
use super::actions::IncludeRemoteAction;
use super::remote::Remote;
use crate::manifest::entry_filter::GlobFilter;
use crate::manifest::item::ItemId;
use crate::manifest::repo_path::{REMOTE_PREFIX, RepoPath};
use crate::source::fetch::{FileUrlError, file_url_path};

/// How a message names the tree a path is read from.
fn tree_of(written: &RepoPath) -> String {
    match written.remote() {
        None => "the repository".to_owned(),
        Some(id) => format!("remote `{id}`"),
    }
}

/// How a message names that tree's root.
fn root_of(written: &RepoPath) -> String {
    match written.remote() {
        None => "the repository root".to_owned(),
        Some(id) => format!("the root of remote `{id}`"),
    }
}

/// Describe the entire repository or remote materialization for a source-path diagnostic.
fn whole_of(written: &RepoPath) -> String {
    match written.remote() {
        None => "the whole repository".to_owned(),
        Some(id) => format!("the whole of remote `{id}`"),
    }
}

/// The bare ID of the remote a path names, for the message that spells a
/// declaration. Empty for a path naming no remote.
fn named_remote_of(written: &RepoPath) -> String {
    written.remote().map(ItemId::to_string).unwrap_or_default()
}

/// Manifest validation failures, including value and cross-record constraints.
#[derive(Debug, Error)]
pub(crate) enum ManifestError {
    #[error("action {second} repeats the id `{id}`, which action {first} already uses")]
    DuplicateActionId {
        id: ItemId,
        first: usize,
        second: usize,
    },

    /// Two remote IDs differ only in case.
    #[error(
        "remotes `{one}` and `{other}` differ only in case; where the filesystem \
         ignores case they are one directory under `remotes/`, and only one of \
         the two repositories would ever be cloned"
    )]
    RemotesShareOneDirectory { one: ItemId, other: ItemId },

    /// A record declares both `when` and `unless`.
    #[error("{record}: writes both `when` and `unless`; a record has one condition or none")]
    BothConditions { record: DeclarationName },

    // Repository source paths.
    #[error("{record}: source is empty; a source names a path within the repository")]
    SourceEmpty { record: DeclarationName },

    /// A source path has a root or platform prefix instead of being tree-relative.
    #[error("{record}: source `{written}` is not relative to {}", root_of(.written))]
    SourceNotRelative {
        record: DeclarationName,
        written: RepoPath,
    },

    #[error("{record}: source `{written}` resolves outside {}", tree_of(.written))]
    SourceOutsideTree {
        record: DeclarationName,
        written: RepoPath,
    },

    /// A source that stays inside its tree but names all of it.
    #[error(
        "{record}: source `{written}` names {}; a source names a path within it",
        whole_of(.written)
    )]
    SourceIsWholeTree {
        record: DeclarationName,
        written: RepoPath,
    },

    /// A path within a remote reference that itself begins with `@`, which
    /// always introduces a remote.
    #[error(
        "{record}: source `{written}` starts with `@`, which introduces a remote \
         reference and cannot start a path within one"
    )]
    SourceStartsWithRemotePrefix {
        record: DeclarationName,
        written: RepoPath,
    },

    /// A source references an undeclared remote.
    #[error(
        "{record}: source `{written}` names {}, which this manifest does not \
         declare; add a `[remotes.{}]` record",
        tree_of(.written),
        named_remote_of(.written)
    )]
    SourceRemoteUndeclared {
        record: DeclarationName,
        written: RepoPath,
    },

    /// An included action references a remote in its source.
    #[error(
        "{record}: source `{written}` names a remote, which an included action may not do; \
         remote references belong to the leaf repository, and an included action installs \
         from the repository that declared it"
    )]
    IncludedSourceNamesRemote {
        record: DeclarationName,
        written: RepoPath,
    },

    /// A source names a path inside a file remote.
    #[error(
        "{record}: source `{written}` names a path within {}, which is a single file; \
         write `@{}` to name the file itself",
        tree_of(.written),
        named_remote_of(.written)
    )]
    SourceInsideFileRemote {
        record: DeclarationName,
        written: RepoPath,
    },

    /// A `source-dir` naming a file remote, which is never a directory.
    #[error(
        "{record}: source `{written}` names {}, which is a single file; \
         this action installs from a directory",
        tree_of(.written)
    )]
    SourceDirIsFileRemote {
        record: DeclarationName,
        written: RepoPath,
    },

    /// An `include-remote` naming a remote that holds no manifest to read.
    #[error(
        "{record}: remote `{remote}` is of type `{kind}`; \
         an include-remote reads the manifest of a `git` remote"
    )]
    InclusionRemoteNotGit {
        record: DeclarationName,
        remote: ItemId,
        kind: &'static str,
    },

    /// An `include-remote` naming a remote no `[remotes]` entry declares.
    #[error(
        "{record}: remote `{remote}` is not declared by this manifest; \
         add a `[remotes.{remote}]` record"
    )]
    InclusionRemoteUndeclared {
        record: DeclarationName,
        remote: ItemId,
    },

    /// An inclusion declares incompatible selection filters.
    #[error(
        "{record}: writes both `{one}` and `{other}`; an inclusion names at most one of \
         `install-actions`, `install-groups`, and `exclude-groups`, and `exclude-actions` \
         goes with either group filter or alone"
    )]
    InclusionFiltersConflict {
        record: DeclarationName,
        one: &'static str,
        other: &'static str,
    },

    // Destination paths.
    #[error("{record}: dest is empty; write `~` for the home directory itself")]
    DestinationEmpty { record: DeclarationName },

    #[error(
        "{record}: dest `{value}` names another user's home; \
         `~` expands only to the selected home"
    )]
    DestinationOtherHome {
        record: DeclarationName,
        value: String,
    },

    // Fetch URLs and digests.
    #[error("{record}: {field} `{value}` is not an http://, https://, or file:// URL")]
    SourceNotAUrl {
        record: DeclarationName,
        /// What the record spells it: `source` on an action, `url` on a remote.
        field: &'static str,
        value: String,
    },

    #[error("{record}: {field} `{value}` {source}")]
    FileUrl {
        record: DeclarationName,
        field: &'static str,
        value: String,
        source: FileUrlError,
    },

    #[error("{record}: sha256 `{value}` is not 64 hexadecimal digits")]
    DigestNotSha256 {
        record: DeclarationName,
        value: String,
    },

    // Git sources and refs.
    #[error("{record}: {field} is empty; it names a repository for git to clone")]
    GitSourceEmpty {
        record: DeclarationName,
        /// What the record spells it: `source` on an action, `url` on a remote.
        field: &'static str,
    },

    /// An explicitly empty Git ref.
    #[error("{record}: ref is empty; a ref names a branch, tag, or commit to follow")]
    GitRefEmpty { record: DeclarationName },

    /// An archive root is not a valid path inside the archive.
    #[error(
        "{record}: archive-root `{value}` is not a path inside the archive; \
         write a prefix such as `tool-1.0`, or `*` for the archive's single top-level directory"
    )]
    ArchiveRootNotInside {
        record: DeclarationName,
        value: String,
    },

    /// A filter over direct children whose pattern could only match below one.
    #[error(
        "{record}: {field} pattern `{pattern}` contains `/`; this action's filters match \
         the names of the direct children of source-dir"
    )]
    ChildFilterNested {
        record: DeclarationName,
        field: &'static str,
        pattern: String,
    },
}

/// Required source kind: any file or directory, or a directory only.
#[derive(Debug, Clone, Copy)]
pub(super) enum SourceShape {
    Any,
    Directory,
}

/// Validate source syntax, declared remote references, and required source shape. File remotes
/// must be referenced as whole files. Do not check filesystem existence.
pub(super) fn check_source(
    written: &RepoPath,
    shape: SourceShape,
    record: &DeclarationName,
    remotes: &BTreeMap<ItemId, Remote>,
) -> Result<(), ManifestError> {
    let path = written.path();
    // An empty path with a remote ID can name a whole file remote; a plain empty source cannot.
    if written.remote().is_none() && path.is_empty() {
        return Err(ManifestError::SourceEmpty {
            record: record.clone(),
        });
    }
    if let Some(Remote::File(_)) = written.remote().and_then(|id| remotes.get(id)) {
        return match (path.is_empty(), shape) {
            (false, _) => Err(ManifestError::SourceInsideFileRemote {
                record: record.clone(),
                written: written.clone(),
            }),
            (true, SourceShape::Directory) => Err(ManifestError::SourceDirIsFileRemote {
                record: record.clone(),
                written: written.clone(),
            }),
            (true, SourceShape::Any) => Ok(()),
        };
    }
    if path.starts_with(REMOTE_PREFIX) {
        return Err(ManifestError::SourceStartsWithRemotePrefix {
            record: record.clone(),
            written: written.clone(),
        });
    }
    if is_anchored(Path::new(path)) {
        return Err(ManifestError::SourceNotRelative {
            record: record.clone(),
            written: written.clone(),
        });
    }
    match depth_within_tree(path) {
        None => {
            return Err(ManifestError::SourceOutsideTree {
                record: record.clone(),
                written: written.clone(),
            });
        }
        // `.` and paths such as `shell/..` resolve to the tree root.
        Some(0) => {
            return Err(ManifestError::SourceIsWholeTree {
                record: record.clone(),
                written: written.clone(),
            });
        }
        Some(_) => {}
    }
    if let Some(id) = written.remote()
        && !remotes.contains_key(id)
    {
        return Err(ManifestError::SourceRemoteUndeclared {
            record: record.clone(),
            written: written.clone(),
        });
    }
    Ok(())
}

/// Require the inclusion to reference a Git remote declared in the same manifest.
pub(super) fn check_inclusion_remote(
    remote: &ItemId,
    record: &DeclarationName,
    remotes: &BTreeMap<ItemId, Remote>,
) -> Result<(), ManifestError> {
    match remotes.get(remote) {
        Some(Remote::Git(_)) => Ok(()),
        Some(other) => Err(ManifestError::InclusionRemoteNotGit {
            record: record.clone(),
            remote: remote.clone(),
            kind: other.kind(),
        }),
        None => Err(ManifestError::InclusionRemoteUndeclared {
            record: record.clone(),
            remote: remote.clone(),
        }),
    }
}

/// Reject incompatible inclusion filters. At most one of `install-actions`, `install-groups`,
/// and `exclude-groups` may be present; `exclude-actions` cannot accompany `install-actions`.
pub(super) fn check_inclusion_filters(
    action: &IncludeRemoteAction,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    // A diagnostic names the conflicting pair in this order.
    let selectors = [
        ("install-actions", action.install_actions.is_some()),
        ("install-groups", action.install_groups.is_some()),
        ("exclude-groups", action.exclude_groups.is_some()),
    ];
    let mut written = selectors.iter().filter(|(_, present)| *present);
    let conflict = match (written.next(), written.next()) {
        (Some((one, _)), Some((other, _))) => Some((*one, *other)),
        (Some((one, _)), None) if *one == "install-actions" && action.exclude_actions.is_some() => {
            Some((*one, "exclude-actions"))
        }
        _ => None,
    };
    match conflict {
        None => Ok(()),
        Some((one, other)) => Err(ManifestError::InclusionFiltersConflict {
            record: record.clone(),
            one,
            other,
        }),
    }
}

/// An action read from an included manifest must not name a remote in its
/// source. Applied by [`Manifest::validate`](super::Manifest::validate) only
/// when reading an included manifest.
pub(super) fn check_included_source(
    written: &RepoPath,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    if written.remote().is_none() {
        Ok(())
    } else {
        Err(ManifestError::IncludedSourceNamesRemote {
            record: record.clone(),
            written: written.clone(),
        })
    }
}

/// Return whether the path starts with a root or platform prefix.
fn is_anchored(source: &Path) -> bool {
    matches!(
        source.components().next(),
        Some(Component::RootDir | Component::Prefix(_))
    )
}

/// How many components deep a relative path lands, or `None` if it does not stay
/// within the tree it is relative to.
fn depth_within_tree(source: &str) -> Option<usize> {
    let mut depth: usize = 0;
    for component in Path::new(source).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => depth = depth.checked_sub(1)?,
            Component::Normal(_) => depth += 1,
            // Unreachable behind `is_anchored`, and neither can appear later.
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(depth)
}

/// Reject empty destinations and unsupported `~user` prefixes.
pub(super) fn check_dest(dest: &str, record: &DeclarationName) -> Result<(), ManifestError> {
    if dest.is_empty() {
        return Err(ManifestError::DestinationEmpty {
            record: record.clone(),
        });
    }
    // Allow `~` and `~/...`, but do not expand other users' homes.
    match dest.strip_prefix('~') {
        Some(rest) if !rest.is_empty() && !rest.starts_with('/') => {
            Err(ManifestError::DestinationOtherHome {
                record: record.clone(),
                value: dest.to_owned(),
            })
        }
        _ => Ok(()),
    }
}

/// Validate a fetch URL. `field` identifies `source` on actions or `url` on remotes in
/// diagnostics.
pub(super) fn check_url(
    source: &str,
    field: &'static str,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    let scheme = |prefix: &str| {
        let (source, prefix) = (source.as_bytes(), prefix.as_bytes());
        source.len() > prefix.len() && source[..prefix.len()].eq_ignore_ascii_case(prefix)
    };
    if scheme("http://") || scheme("https://") {
        return Ok(());
    }
    match file_url_path(source) {
        Some(Ok(_)) => Ok(()),
        Some(Err(error)) => Err(ManifestError::FileUrl {
            record: record.clone(),
            field,
            value: source.to_owned(),
            source: error,
        }),
        None => Err(ManifestError::SourceNotAUrl {
            record: record.clone(),
            field,
            value: source.to_owned(),
        }),
    }
}

/// Reject an empty or whitespace-only Git source. `field` identifies `source` on actions or
/// `url` on remotes in diagnostics.
pub(super) fn check_git_source(
    source: &str,
    field: &'static str,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    if source.trim().is_empty() {
        return Err(ManifestError::GitSourceEmpty {
            record: record.clone(),
            field,
        });
    }
    Ok(())
}

/// Reject an empty or whitespace-only Git ref; allow an absent ref.
pub(super) fn check_git_ref(
    git_ref: Option<&str>,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    if git_ref.is_some_and(|value| value.trim().is_empty()) {
        return Err(ManifestError::GitRefEmpty {
            record: record.clone(),
        });
    }
    Ok(())
}

/// Require a declared SHA-256 digest to contain exactly 64 hexadecimal digits.
pub(super) fn check_digest(
    sha256: Option<&str>,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    match sha256 {
        Some(value) if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
            Err(ManifestError::DigestNotSha256 {
                record: record.clone(),
                value: value.to_owned(),
            })
        }
        _ => Ok(()),
    }
}

/// Validate an optional archive-root path or the `*` automatic-root selector.
pub(super) fn check_archive_root(
    archive_root: Option<&str>,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    let Some(value) = archive_root else {
        return Ok(());
    };
    if value == "*" {
        return Ok(());
    }
    if names_a_path_inside_an_archive(value) {
        Ok(())
    } else {
        Err(ManifestError::ArchiveRootNotInside {
            record: record.clone(),
            value: value.to_owned(),
        })
    }
}

/// Refuse a pattern in a filter over direct child names that contains `/`.
pub(super) fn check_child_filter(
    field: &'static str,
    filter: Option<&GlobFilter>,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    match filter.and_then(GlobFilter::first_nested) {
        None => Ok(()),
        Some(pattern) => Err(ManifestError::ChildFilterNested {
            record: record.clone(),
            field,
            pattern: pattern.as_str().to_owned(),
        }),
    }
}

/// Whether a value is spelled the way a path inside an archive is: ordinary
/// components and `.`, with at least one of the former.
fn names_a_path_inside_an_archive(value: &str) -> bool {
    let mut named = 0;
    for component in Path::new(value).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => named += 1,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    named > 0
}

#[cfg(test)]
mod tests;
