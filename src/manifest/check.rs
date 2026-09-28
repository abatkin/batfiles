//! Manifest rules TOML cannot express, and their diagnostics.
//!
//! Each `check_*` below validates one value's shape;
//! [`Manifest::validate`](super::Manifest::validate) applies rules spanning
//! records. Both are decided from the document alone, while it is read. See
//! [`docs/repoformat.md`](../../docs/repoformat.md#reading-the-manifest).

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Component, Path};

use thiserror::Error;

use super::action::IncludeRemoteAction;
use super::remote::Remote;
use crate::fetch::{FileUrlError, file_url_path};
use crate::item::ItemId;
use crate::repo_path::{REMOTE_PREFIX, RepoPath};

/// How a load diagnostic names the record that broke the rule; every
/// [`Invalid`] message opens with one.
///
/// Actions and bootstrap candidates are named by position: an action need not
/// have an `id`, and what a candidate names is never looked up. A remote is
/// named by its map key.
#[derive(Debug, Clone)]
pub(crate) enum RecordName {
    /// An action, by its one-based position in `[[actions]]`.
    Action(usize),
    /// A `[default-disabled]` candidate, by its array and one-based position.
    Candidate { noun: &'static str, number: usize },
    /// A `[remotes]` entry, by the ID it was declared under.
    Remote(ItemId),
}

impl fmt::Display for RecordName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Action(number) => write!(f, "action {number}"),
            Self::Candidate { noun, number } => write!(f, "default-disabled {noun} {number}"),
            Self::Remote(id) => write!(f, "remote `{id}`"),
        }
    }
}

// Path rules are the same for every tree; only the noun naming the tree
// changes, with a phrasing for each kind of sentence.

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

/// How a message names all of it.
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

/// A manifest that parsed but breaks one of the rules in this module, or the
/// cross-record ones [`Manifest::validate`](super::Manifest::validate) holds.
///
/// Re-exported as `manifest::Invalid`, which is how the rest of the crate names
/// it.
#[derive(Debug, Error)]
pub(crate) enum Invalid {
    #[error("action {second} repeats the id `{id}`, which action {first} already uses")]
    DuplicateActionId {
        id: ItemId,
        first: usize,
        second: usize,
    },

    /// Two remote IDs that a case-folding filesystem cannot tell apart. Refused
    /// on every platform, since one manifest serves all of a person's machines.
    #[error(
        "remotes `{one}` and `{other}` differ only in case; where the filesystem \
         ignores case they are one directory under `remotes/`, and only one of \
         the two repositories would ever be cloned"
    )]
    RemotesShareOneDirectory { one: ItemId, other: ItemId },

    /// A record writing both `when` and `unless`, which has no obvious meaning.
    /// Shared by every kind of record that takes a condition.
    #[error("{record}: writes both `when` and `unless`; a record has one condition or none")]
    BothConditions { record: RecordName },

    // A `source` is a path in the declaring repository or, with `@`, in a
    // remote's materialization. Each variant carries the written path, from
    // which the message renders both the quote and the tree.
    #[error("{record}: source is empty; a source names a path within the repository")]
    SourceEmpty { record: RecordName },

    /// A source naming its own starting point — a leading `/`, a `\`, or a
    /// drive letter — rather than one relative to the tree it is read from.
    #[error("{record}: source `{written}` is not relative to {}", root_of(.written))]
    SourceNotRelative {
        record: RecordName,
        written: RepoPath,
    },

    #[error("{record}: source `{written}` resolves outside {}", tree_of(.written))]
    SourceOutsideTree {
        record: RecordName,
        written: RepoPath,
    },

    /// A source that stays inside its tree but names all of it.
    #[error(
        "{record}: source `{written}` names {}; a source names a path within it",
        whole_of(.written)
    )]
    SourceIsWholeTree {
        record: RecordName,
        written: RepoPath,
    },

    /// A path within a remote reference that itself begins with `@`, which
    /// always introduces a remote.
    #[error(
        "{record}: source `{written}` starts with `@`, which introduces a remote \
         reference and cannot start a path within one"
    )]
    SourceStartsWithRemotePrefix {
        record: RecordName,
        written: RepoPath,
    },

    /// A source naming a remote no `[remotes]` entry declares. The message
    /// points at the missing declaration, not at the path.
    #[error(
        "{record}: source `{written}` names {}, which this manifest does not \
         declare; add a `[remotes.{}]` record",
        tree_of(.written),
        named_remote_of(.written)
    )]
    SourceRemoteUndeclared {
        record: RecordName,
        written: RepoPath,
    },

    /// An action read from an included manifest whose source names a remote,
    /// declared or not. Remote references belong to the leaf, which keeps
    /// inclusion one level deep.
    #[error(
        "{record}: source `{written}` names a remote, which an included action may not do; \
         remote references belong to the leaf repository, and an included action installs \
         from the repository that declared it"
    )]
    IncludedSourceNamesRemote {
        record: RecordName,
        written: RepoPath,
    },

    /// A source naming a path within a file remote, whose materialization is
    /// the one file rather than a tree holding it.
    #[error(
        "{record}: source `{written}` names a path within {}, which is a single file; \
         write `@{}` to name the file itself",
        tree_of(.written),
        named_remote_of(.written)
    )]
    SourceInsideFileRemote {
        record: RecordName,
        written: RepoPath,
    },

    /// A `source-dir` naming a file remote, which is never a directory.
    #[error(
        "{record}: source `{written}` names {}, which is a single file; \
         this action installs from a directory",
        tree_of(.written)
    )]
    SourceDirIsFileRemote {
        record: RecordName,
        written: RepoPath,
    },

    /// An `include-remote` naming a remote that holds no manifest to read.
    #[error(
        "{record}: remote `{remote}` is of type `{kind}`; \
         an include-remote reads the manifest of a `git` remote"
    )]
    InclusionRemoteNotGit {
        record: RecordName,
        remote: ItemId,
        kind: &'static str,
    },

    /// An `include-remote` naming a remote no `[remotes]` entry declares.
    #[error(
        "{record}: remote `{remote}` is not declared by this manifest; \
         add a `[remotes.{remote}]` record"
    )]
    InclusionRemoteUndeclared { record: RecordName, remote: ItemId },

    /// An `include-remote` writing two selection filters that do not compose.
    /// The message names both fields and the combinations that do.
    #[error(
        "{record}: writes both `{one}` and `{other}`; an inclusion names at most one of \
         `install-actions`, `install-groups`, and `exclude-groups`, and `exclude-actions` \
         goes with either group filter or alone"
    )]
    InclusionFiltersConflict {
        record: RecordName,
        one: &'static str,
        other: &'static str,
    },

    // A `dest` names a path on the machine, anchored to the selected home.
    #[error("{record}: dest is empty; write `~` for the home directory itself")]
    DestinationEmpty { record: RecordName },

    #[error(
        "{record}: dest `{value}` names another user's home; \
         `~` expands only to the selected home"
    )]
    DestinationOtherHome { record: RecordName, value: String },

    // Fetching actions and file and archive remotes: a URL and an optional
    // digest.
    #[error("{record}: {field} `{value}` is not an http://, https://, or file:// URL")]
    SourceNotAUrl {
        record: RecordName,
        /// What the record spells it: `source` on an action, `url` on a remote.
        field: &'static str,
        value: String,
    },

    #[error("{record}: {field} `{value}` {source}")]
    FileUrl {
        record: RecordName,
        field: &'static str,
        value: String,
        source: FileUrlError,
    },

    #[error("{record}: sha256 `{value}` is not 64 hexadecimal digits")]
    DigestNotSha256 { record: RecordName, value: String },

    // A repository for git to clone, in any form git accepts; shared by
    // `git-clone` and Git remotes. Only emptiness is checked.
    #[error("{record}: {field} is empty; it names a repository for git to clone")]
    GitSourceEmpty {
        record: RecordName,
        /// What the record spells it: `source` on an action, `url` on a remote.
        field: &'static str,
    },

    /// An empty `ref`. Not read as absent, which would follow the current
    /// branch.
    #[error("{record}: ref is empty; a ref names a branch, tag, or commit to follow")]
    GitRefEmpty { record: RecordName },

    /// An `archive-root` that could only match escaping entries, which unpacking
    /// refuses. Caught before anything is downloaded.
    #[error(
        "{record}: archive-root `{value}` is not a path inside the archive; \
         write a prefix such as `tool-1.0`, or `*` for the archive's single top-level directory"
    )]
    ArchiveRootNotInside { record: RecordName, value: String },
}

/// What a field installs from: a `source-dir` names a directory, and any
/// other source a file or a directory.
#[derive(Debug, Clone, Copy)]
pub(super) enum SourceShape {
    Any,
    Directory,
}

/// The rules a `source` satisfies as written, including that a named remote is
/// declared in `remotes`, and that a file remote is named whole and never as a
/// directory. Whether anything exists at the path is decided when the action
/// runs.
pub(super) fn check_source(
    written: &RepoPath,
    shape: SourceShape,
    record: &RecordName,
    remotes: &BTreeMap<ItemId, Remote>,
) -> Result<(), Invalid> {
    let path = written.path();
    // Only a plain string can say nothing at all; a reference that names a
    // remote and no path has named a tree, and is refused as naming all of it,
    // unless the remote is a single file.
    if written.remote().is_none() && path.is_empty() {
        return Err(Invalid::SourceEmpty {
            record: record.clone(),
        });
    }
    if let Some(Remote::File(_)) = written.remote().and_then(|id| remotes.get(id)) {
        return match (path.is_empty(), shape) {
            (false, _) => Err(Invalid::SourceInsideFileRemote {
                record: record.clone(),
                written: written.clone(),
            }),
            (true, SourceShape::Directory) => Err(Invalid::SourceDirIsFileRemote {
                record: record.clone(),
                written: written.clone(),
            }),
            (true, SourceShape::Any) => Ok(()),
        };
    }
    if path.starts_with(REMOTE_PREFIX) {
        return Err(Invalid::SourceStartsWithRemotePrefix {
            record: record.clone(),
            written: written.clone(),
        });
    }
    if is_anchored(Path::new(path)) {
        return Err(Invalid::SourceNotRelative {
            record: record.clone(),
            written: written.clone(),
        });
    }
    match depth_within_tree(path) {
        None => {
            return Err(Invalid::SourceOutsideTree {
                record: record.clone(),
                written: written.clone(),
            });
        }
        // Zero components deep is the tree's own root, however it was spelled:
        // `.`, `./`, `shell/..`, and a reference with no path all land there.
        Some(0) => {
            return Err(Invalid::SourceIsWholeTree {
                record: record.clone(),
                written: written.clone(),
            });
        }
        Some(_) => {}
    }
    // Checked after the path rules, which concern what the reader can see in
    // the value.
    if let Some(id) = written.remote()
        && !remotes.contains_key(id)
    {
        return Err(Invalid::SourceRemoteUndeclared {
            record: record.clone(),
            written: written.clone(),
        });
    }
    Ok(())
}

/// An `include-remote`'s `remote` must be a `git` remote declared in the same
/// manifest: only a repository has a manifest to read.
pub(super) fn check_inclusion_remote(
    remote: &ItemId,
    record: &RecordName,
    remotes: &BTreeMap<ItemId, Remote>,
) -> Result<(), Invalid> {
    match remotes.get(remote) {
        Some(Remote::Git(_)) => Ok(()),
        Some(other) => Err(Invalid::InclusionRemoteNotGit {
            record: record.clone(),
            remote: remote.clone(),
            kind: other.kind(),
        }),
        None => Err(Invalid::InclusionRemoteUndeclared {
            record: record.clone(),
            remote: remote.clone(),
        }),
    }
}

/// Which of an `include-remote`'s selection filters may appear together.
///
/// `install-actions`, `install-groups`, and `exclude-groups` each make a
/// selection, so at most one may be written. `exclude-actions` narrows a group
/// filter's selection; with `install-actions` it could only contradict it.
pub(super) fn check_inclusion_filters(
    action: &IncludeRemoteAction,
    record: &RecordName,
) -> Result<(), Invalid> {
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
        Some((one, other)) => Err(Invalid::InclusionFiltersConflict {
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
    record: &RecordName,
) -> Result<(), Invalid> {
    if written.remote().is_none() {
        Ok(())
    } else {
        Err(Invalid::IncludedSourceNamesRemote {
            record: record.clone(),
            written: written.clone(),
        })
    }
}

/// Whether a path starts from somewhere of its own rather than from wherever it
/// is joined onto.
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

/// The rules a `dest` satisfies as written.
pub(super) fn check_dest(dest: &str, record: &RecordName) -> Result<(), Invalid> {
    if dest.is_empty() {
        return Err(Invalid::DestinationEmpty {
            record: record.clone(),
        });
    }
    // `~` alone and `~/…` mean the selected home. `~other` is another user's,
    // which batfiles does not look up.
    match dest.strip_prefix('~') {
        Some(rest) if !rest.is_empty() && !rest.starts_with('/') => {
            Err(Invalid::DestinationOtherHome {
                record: record.clone(),
                value: dest.to_owned(),
            })
        }
        _ => Ok(()),
    }
}

/// The rules a URL to fetch satisfies as written: a fetching action's `source`
/// or a file or archive remote's `url`, which `field` names.
pub(super) fn check_url(
    source: &str,
    field: &'static str,
    record: &RecordName,
) -> Result<(), Invalid> {
    let scheme = |prefix: &str| {
        let (source, prefix) = (source.as_bytes(), prefix.as_bytes());
        source.len() > prefix.len() && source[..prefix.len()].eq_ignore_ascii_case(prefix)
    };
    if scheme("http://") || scheme("https://") {
        return Ok(());
    }
    match file_url_path(source) {
        Some(Ok(_)) => Ok(()),
        Some(Err(error)) => Err(Invalid::FileUrl {
            record: record.clone(),
            field,
            value: source.to_owned(),
            source: error,
        }),
        None => Err(Invalid::SourceNotAUrl {
            record: record.clone(),
            field,
            value: source.to_owned(),
        }),
    }
}

/// The rule a repository for git to clone satisfies as written. `field` is
/// `source` on a `git-clone` action and `url` on a Git remote.
pub(super) fn check_git_source(
    source: &str,
    field: &'static str,
    record: &RecordName,
) -> Result<(), Invalid> {
    if source.trim().is_empty() {
        return Err(Invalid::GitSourceEmpty {
            record: record.clone(),
            field,
        });
    }
    Ok(())
}

/// The rule a `ref` satisfies as written, for `git-clone` and Git remotes.
pub(super) fn check_git_ref(git_ref: Option<&str>, record: &RecordName) -> Result<(), Invalid> {
    if git_ref.is_some_and(|value| value.trim().is_empty()) {
        return Err(Invalid::GitRefEmpty {
            record: record.clone(),
        });
    }
    Ok(())
}

/// The shape a `sha256` has to have to be one.
pub(super) fn check_digest(sha256: Option<&str>, record: &RecordName) -> Result<(), Invalid> {
    match sha256 {
        Some(value) if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
            Err(Invalid::DigestNotSha256 {
                record: record.clone(),
                value: value.to_owned(),
            })
        }
        _ => Ok(()),
    }
}

/// The shape an `archive-root` has to have to name something inside an archive.
pub(super) fn check_archive_root(
    archive_root: Option<&str>,
    record: &RecordName,
) -> Result<(), Invalid> {
    let Some(value) = archive_root else {
        return Ok(());
    };
    if value == "*" {
        return Ok(());
    }
    if names_a_path_inside_an_archive(value) {
        Ok(())
    } else {
        Err(Invalid::ArchiveRootNotInside {
            record: record.clone(),
            value: value.to_owned(),
        })
    }
}

/// Whether a value is spelled the way a path inside an archive is: ordinary
/// components and `.`, with at least one of the former.
fn names_a_path_inside_an_archive(value: &str) -> bool {
    let mut named = 0;
    for component in Path::new(value).components() {
        match component {
            // Dropped rather than counted, the way an entry path drops it.
            Component::CurDir => {}
            Component::Normal(_) => named += 1,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    named > 0
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// The record the checks below report against.
    fn record() -> RecordName {
        RecordName::Action(1)
    }

    /// A manifest's `[remotes]` declaring `ids`; their contents do not matter.
    fn declaring(ids: &[&str]) -> BTreeMap<ItemId, Remote> {
        ids.iter()
            .map(|id| {
                let remote = toml::from_str("type = \"git\"\nurl = \"https://e.example/r.git\"\n")
                    .expect("a well-formed remote");
                (
                    ItemId::try_from((*id).to_owned()).expect("valid ID"),
                    remote,
                )
            })
            .collect()
    }

    /// A path in the declaring repository, built rather than parsed so that a
    /// Windows spelling reaches the check with its backslashes intact.
    fn local(source: &str) -> RepoPath {
        RepoPath::local(source.to_owned())
    }

    fn source_error(source: &str) -> Invalid {
        check_source(&local(source), SourceShape::Any, &record(), &declaring(&[]))
            .expect_err("expected the source to be refused")
    }

    #[test]
    fn every_kind_of_record_is_named_the_way_its_document_names_it() {
        assert_eq!(RecordName::Action(3).to_string(), "action 3");
        assert_eq!(
            RecordName::Candidate {
                noun: "group",
                number: 1
            }
            .to_string(),
            "default-disabled group 1"
        );
        let core = ItemId::try_from("core".to_owned()).expect("valid ID");
        assert_eq!(RecordName::Remote(core).to_string(), "remote `core`");
    }

    #[test]
    fn a_source_names_a_path_within_its_repository() {
        for accepted in ["shell/zshrc", "editor/nvim", "a/../b", "./bin/batgrep"] {
            assert!(
                check_source(
                    &local(accepted),
                    SourceShape::Any,
                    &record(),
                    &declaring(&[])
                )
                .is_ok(),
                "`{accepted}` was refused"
            );
        }
    }

    #[test]
    fn a_source_may_not_leave_its_repository() {
        for escaping in ["/etc/passwd", "../secrets", "files/../../secrets"] {
            assert!(
                matches!(
                    source_error(escaping),
                    Invalid::SourceNotRelative { .. } | Invalid::SourceOutsideTree { .. }
                ),
                "`{escaping}` was accepted"
            );
        }
    }

    #[test]
    fn a_component_that_starts_a_path_over_is_never_ordinary_depth() {
        // Which spellings produce a root or a prefix is platform-specific — see
        // the test below — so this covers the handling, not the parsing.
        assert_eq!(depth_within_tree("/etc/hosts"), None);
        assert_eq!(depth_within_tree("shell/zshrc"), Some(2));
    }

    /// The spellings that reach that arm on Windows and nowhere else: on Unix
    /// each is an ordinary file name, since a file really can be called
    /// `C:config`. No CI runner reaches this, so `task lint`'s Windows target is
    /// what keeps it compiling.
    #[cfg(windows)]
    #[test]
    fn a_windows_rooted_or_drive_relative_source_is_refused() {
        // Absolute on Windows means a drive *and* a root, so `is_absolute` is
        // false for all three of these while `join` still honors each.
        for anchored in [r"\etc\hosts", "/etc/hosts", "C:config"] {
            assert!(
                !Path::new(anchored).is_absolute(),
                "`{anchored}` is absolute after all, so this test proves nothing"
            );
            assert!(
                matches!(source_error(anchored), Invalid::SourceNotRelative { .. }),
                "`{anchored}` was accepted"
            );
        }
        // The one that is absolute is refused by the same check, not a second.
        assert!(matches!(
            source_error(r"C:\config"),
            Invalid::SourceNotRelative { .. }
        ));
    }

    #[test]
    fn a_source_may_not_name_the_whole_repository() {
        assert!(matches!(source_error(""), Invalid::SourceEmpty { .. }));
        for whole in [".", "./", "shell/.."] {
            assert!(
                matches!(source_error(whole), Invalid::SourceIsWholeTree { .. }),
                "`{whole}` was not recognized as the whole repository"
            );
        }
    }

    /// A remote reference, read the way a manifest hands one over.
    fn remote_path(written: &str) -> RepoPath {
        toml::from_str::<BTreeMap<String, RepoPath>>(&format!("source = \"{written}\"\n"))
            .expect("a well-formed reference")
            .remove("source")
            .expect("the value that was just read")
    }

    fn remote_error(written: &str) -> Invalid {
        check_source(
            &remote_path(written),
            SourceShape::Any,
            &record(),
            &declaring(&["core"]),
        )
        .expect_err("expected the source to be refused")
    }

    #[test]
    fn a_remote_reference_follows_the_same_rules_as_a_local_source() {
        for accepted in ["@core/shell/zshrc", "@core/a/../b", "@core/./bin/batgrep"] {
            assert!(
                check_source(
                    &remote_path(accepted),
                    SourceShape::Any,
                    &record(),
                    &declaring(&["core"])
                )
                .is_ok(),
                "`{accepted}` was refused"
            );
        }
    }

    #[test]
    fn a_source_may_only_name_a_remote_the_manifest_declares() {
        let written = remote_path("@core/shell/zshrc");
        assert!(check_source(&written, SourceShape::Any, &record(), &declaring(&["core"])).is_ok());
        let refused = check_source(&written, SourceShape::Any, &record(), &declaring(&["work"]))
            .expect_err("expected an undeclared remote to be refused");
        assert!(matches!(refused, Invalid::SourceRemoteUndeclared { .. }));
        let message = refused.to_string();
        assert!(message.contains("does not declare"), "{message}");
        assert!(message.contains("[remotes.core]"), "{message}");
    }

    #[test]
    fn a_path_that_breaks_a_rule_is_reported_as_that_rather_than_as_an_undeclared_remote() {
        // Both faults at once: the path rules come first.
        assert!(matches!(
            check_source(
                &remote_path("@core/../secrets"),
                SourceShape::Any,
                &record(),
                &declaring(&[])
            )
            .expect_err("expected the source to be refused"),
            Invalid::SourceOutsideTree { .. }
        ));
    }

    #[test]
    fn a_refused_remote_reference_names_the_remote_rather_than_the_repository() {
        // The same three refusals, naming the remote's tree.
        for (refused, expected) in [
            ("@core/../secrets", "resolves outside remote `core`"),
            (
                "@core//etc/passwd",
                "is not relative to the root of remote `core`",
            ),
            ("@core", "names the whole of remote `core`"),
            ("@core/shell/..", "names the whole of remote `core`"),
        ] {
            let message = remote_error(refused).to_string();
            assert!(message.contains(expected), "`{refused}`: {message}");
        }
    }

    #[test]
    fn a_path_within_a_remote_may_not_start_the_reference_over() {
        // `@` introduces a remote and nothing else, in either spelling, so a
        // second one cannot open a path within the first.
        assert!(matches!(
            remote_error("@core/@other/zshrc"),
            Invalid::SourceStartsWithRemotePrefix { .. }
        ));
        let structured: BTreeMap<String, RepoPath> =
            toml::from_str("source = { remote = \"core\", path = \"@other/zshrc\" }\n")
                .expect("a well-formed reference");
        assert!(matches!(
            check_source(
                &structured["source"],
                SourceShape::Any,
                &record(),
                &declaring(&["core"])
            )
            .expect_err("expected the source to be refused"),
            Invalid::SourceStartsWithRemotePrefix { .. }
        ));
    }

    /// A manifest's `[remotes]` declaring one file remote, `pathogen`.
    fn declaring_a_file() -> BTreeMap<ItemId, Remote> {
        let remote = toml::from_str("type = \"file\"\nurl = \"https://e.example/p.vim\"\n")
            .expect("a well-formed remote");
        BTreeMap::from([(
            ItemId::try_from("pathogen".to_owned()).expect("valid ID"),
            remote,
        )])
    }

    #[test]
    fn a_file_remote_is_named_whole_and_never_as_a_directory() {
        let remotes = declaring_a_file();
        let check =
            |written: &str, shape| check_source(&remote_path(written), shape, &record(), &remotes);
        assert!(check("@pathogen", SourceShape::Any).is_ok());
        let structured: BTreeMap<String, RepoPath> =
            toml::from_str("source = { remote = \"pathogen\" }\n")
                .expect("a well-formed reference");
        assert!(check_source(&structured["source"], SourceShape::Any, &record(), &remotes).is_ok());

        let inside = check("@pathogen/autoload/pathogen.vim", SourceShape::Any)
            .expect_err("a file remote has no paths inside it")
            .to_string();
        assert!(inside.contains("which is a single file"), "{inside}");
        assert!(inside.contains("write `@pathogen`"), "{inside}");

        let directory = check("@pathogen", SourceShape::Directory)
            .expect_err("a file remote is not a directory")
            .to_string();
        assert!(
            directory.contains("installs from a directory"),
            "{directory}"
        );
    }

    #[test]
    fn an_inclusion_names_a_git_remote() {
        let pathogen = ItemId::try_from("pathogen".to_owned()).expect("valid ID");
        let message = check_inclusion_remote(&pathogen, &record(), &declaring_a_file())
            .expect_err("a file remote has no manifest")
            .to_string();
        assert!(
            message.contains("remote `pathogen` is of type `file`"),
            "{message}"
        );
        assert!(message.contains("manifest of a `git` remote"), "{message}");
    }

    #[test]
    fn an_inclusion_may_only_name_a_remote_the_manifest_declares() {
        let core = ItemId::try_from("core".to_owned()).expect("valid ID");
        assert!(check_inclusion_remote(&core, &record(), &declaring(&["core"])).is_ok());
        let refused = check_inclusion_remote(&core, &record(), &declaring(&["work"]))
            .expect_err("expected an undeclared remote to be refused");
        let message = refused.to_string();
        assert!(message.contains("is not declared"), "{message}");
        assert!(message.contains("[remotes.core]"), "{message}");
    }

    /// An inclusion carrying the filters named, read as a manifest hands one
    /// over, and checked.
    fn filters(written: &str) -> Result<(), Invalid> {
        let action: IncludeRemoteAction =
            toml::from_str(&format!("id = \"corp\"\nremote = \"core\"\n{written}"))
                .expect("the record should parse");
        check_inclusion_filters(&action, &record())
    }

    #[test]
    fn an_inclusion_may_select_once_and_then_narrow_it() {
        for accepted in [
            "",
            "install-actions = [\"zshrc\"]",
            "install-groups = [\"shell\"]",
            "exclude-groups = [\"gui\"]",
            "exclude-actions = [\"p10k\"]",
            // The two combinations the narrowing half is written for.
            "install-groups = [\"shell\"]\nexclude-actions = [\"p10k\"]",
            "exclude-groups = [\"gui\"]\nexclude-actions = [\"p10k\"]",
        ] {
            assert!(
                filters(accepted).is_ok(),
                "`{accepted}` was refused: {:?}",
                filters(accepted).expect_err("just checked")
            );
        }
    }

    #[test]
    fn an_inclusion_may_not_describe_its_selection_twice() {
        for (refused, one, other) in [
            (
                "install-actions = [\"zshrc\"]\ninstall-groups = [\"shell\"]",
                "install-actions",
                "install-groups",
            ),
            (
                "install-actions = [\"zshrc\"]\nexclude-groups = [\"gui\"]",
                "install-actions",
                "exclude-groups",
            ),
            (
                "install-groups = [\"shell\"]\nexclude-groups = [\"gui\"]",
                "install-groups",
                "exclude-groups",
            ),
            // The allow-list already names every action to take, so a list of
            // actions to leave out could only contradict it.
            (
                "install-actions = [\"zshrc\"]\nexclude-actions = [\"p10k\"]",
                "install-actions",
                "exclude-actions",
            ),
        ] {
            let message = filters(refused)
                .expect_err("the combination should be refused")
                .to_string();
            assert!(message.contains(&format!("`{one}`")), "{message}");
            assert!(message.contains(&format!("`{other}`")), "{message}");
        }
    }

    #[test]
    fn an_empty_filter_is_written_as_much_as_a_full_one() {
        // Absent and empty differ everywhere else, so they differ here: an
        // empty allow-list is a selection, and a second one still conflicts.
        assert!(filters("install-actions = []").is_ok());
        assert!(filters("install-actions = []\ninstall-groups = []").is_err());
    }

    #[test]
    fn an_included_action_installs_from_the_repository_that_declared_it() {
        // Decided from the spelling alone, whatever the included manifest
        // declares.
        assert!(check_included_source(&local("files/zshrc"), &record()).is_ok());
        let message = check_included_source(&remote_path("@shared/vimrc"), &record())
            .expect_err("expected a remote reference to be refused")
            .to_string();
        assert!(message.contains("`@shared/vimrc`"), "{message}");
        assert!(message.contains("names a remote"), "{message}");
        assert!(message.contains("leaf repository"), "{message}");
    }

    #[test]
    fn a_dest_may_be_anywhere_the_selected_home_can_reach() {
        // The home is a base, not a boundary: someone linking onto another
        // volume or into a sibling directory is expressing intent.
        for accepted in ["~", "~/.zshrc", ".zshrc", "/etc/hosts", "~/../shared/rc"] {
            assert!(
                check_dest(accepted, &record()).is_ok(),
                "`{accepted}` was refused"
            );
        }
    }

    #[test]
    fn a_dest_is_written_out_rather_than_left_empty() {
        // `~` says "the home directory itself" and an empty value only looks
        // like it, so the explicit spelling is the one batfiles accepts.
        assert!(matches!(
            check_dest("", &record()).expect_err("expected an empty dest to be refused"),
            Invalid::DestinationEmpty { .. }
        ));
    }

    #[test]
    fn a_fetched_source_names_a_scheme_batfiles_can_fetch() {
        for accepted in [
            "http://example.com/a",
            "https://example.com/a",
            "HTTPS://EXAMPLE.COM/A",
            // Everything after the scheme is the server's business, including
            // what looks like nonsense from here.
            "https://example.com/a b?c=d#e",
        ] {
            assert!(
                check_url(accepted, "source", &record()).is_ok(),
                "`{accepted}` was refused"
            );
        }
    }

    #[test]
    fn a_fetched_source_that_is_not_a_url_is_refused_as_written() {
        for refused in [
            "files/ackrc",
            "example.com/a",
            "",
            "https://",
            "ftp://a/b",
            "💥💥x",
            "日本語のパス",
            "ht💥p://example.com/a",
        ] {
            assert!(
                matches!(
                    check_url(refused, "source", &record())
                        .expect_err("expected the source to be refused"),
                    Invalid::SourceNotAUrl { .. }
                ),
                "`{refused}` was accepted"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_file_url_names_a_file_on_this_machine() {
        assert!(check_url("file:///etc/hosts", "source", &record()).is_ok());
        let message = check_url("file://server/etc/hosts", "url", &record())
            .expect_err("expected a file URL naming a host to be refused")
            .to_string();
        assert!(
            message.contains("url `file://server/etc/hosts` names a host other than `localhost`"),
            "{message}"
        );
    }

    #[test]
    fn a_digest_is_sixty_four_hexadecimal_digits_or_absent() {
        let sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(check_digest(None, &record()).is_ok());
        assert!(check_digest(Some(sha), &record()).is_ok());
        assert!(
            check_digest(Some(&sha.to_uppercase()), &record()).is_ok(),
            "a digest written in capitals is the same digest"
        );
        // Too short, too long, and the right length with a non-digit in it.
        for refused in [&sha[..63], &format!("{sha}0")[..], &sha.replace('e', "g")] {
            assert!(
                matches!(
                    check_digest(Some(refused), &record())
                        .expect_err("expected the digest to be refused"),
                    Invalid::DigestNotSha256 { .. }
                ),
                "`{refused}` was accepted"
            );
        }
    }

    #[test]
    fn an_archive_root_names_a_prefix_or_asks_for_the_only_one() {
        for accepted in [
            None,
            Some("*"),
            Some("tool-1.0"),
            Some("tool-1.0/bin"),
            // The spelling `tar czf x.tgz .` gives every entry, dropped here as
            // it is dropped there.
            Some("./tool-1.0"),
        ] {
            assert!(
                check_archive_root(accepted, &record()).is_ok(),
                "`{accepted:?}` was refused"
            );
        }
    }

    #[test]
    fn an_archive_root_that_could_match_no_entry_is_refused_as_written() {
        for refused in ["", "/tool", "../tool", ".", "tool/..", "releases/../tool"] {
            assert!(
                matches!(
                    check_archive_root(Some(refused), &record())
                        .expect_err("expected the archive root to be refused"),
                    Invalid::ArchiveRootNotInside { .. }
                ),
                "`{refused}` was accepted"
            );
        }
    }

    #[test]
    fn only_the_selected_home_is_spelled_with_a_tilde() {
        assert!(matches!(
            check_dest("~other/.zshrc", &record())
                .expect_err("expected another user's home to be refused"),
            Invalid::DestinationOtherHome { .. }
        ));
    }
}
