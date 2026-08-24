//! `batfiles.toml`: the one file in a repository with intrinsic meaning.
//!
//! The record mirrors the file. A rule serde cannot express — one spanning two
//! records, or one about the shape of a value rather than its type — is checked
//! by [`Manifest::validate`] as the document is read, so that a manifest which
//! cannot be honored is refused before any of it is acted on.

pub(crate) mod action;

use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::Deserialize;
use thiserror::Error;

use crate::error::Error as CrateError;
use crate::item::ItemId;
use crate::manifest::action::Action;
use crate::tomlfile;

/// A parsed `batfiles.toml`.
///
/// Every section is optional, and there is no format-version field. The
/// document is a closed record: a top-level key batfiles does not know is an
/// error rather than something to ignore, so a section belonging to a slice
/// that has not landed — `[vars]`, `[remotes]` — fails outright instead of
/// looking as though it took effect.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct Manifest {
    /// The ordered action list. Order is significant, so this is the one
    /// section that is a sequence rather than a map.
    #[serde(default)]
    pub actions: Vec<Action>,
}

impl Manifest {
    /// The manifest's name within a repository root.
    ///
    /// The name travels with the parser, but *where* a repository is does not:
    /// a leaf comes from the resolved roots and a remote will come from its
    /// materialization, so callers pass a whole path to [`load`](Self::load).
    pub const FILE_NAME: &'static str = "batfiles.toml";

    /// Read, parse, and check one manifest.
    ///
    /// The path is attached here rather than threaded through the rules, which
    /// are about the document's contents and do not care what it is called.
    pub fn load(path: &Path) -> Result<Self, CrateError> {
        let manifest: Self = tomlfile::read(path)?;
        manifest
            .validate()
            .map_err(|source| CrateError::InvalidManifest {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(manifest)
    }

    /// The rules serde cannot express: action IDs being unique across records,
    /// and the shape of values whose type does not constrain them.
    ///
    /// A rule belongs here when it is decidable from the document alone. One
    /// needing a resolved root or the filesystem — whether a `source` exists —
    /// is not, and stays with the action.
    ///
    /// **One problem at a time.** The first offense returns, so a manifest with
    /// two faults reports the earlier one and the next run reports the rest.
    fn validate(&self) -> Result<(), Invalid> {
        let mut seen: BTreeMap<&ItemId, usize> = BTreeMap::new();
        for (index, action) in self.actions.iter().enumerate() {
            // One-based, because the diagnostic is read against a file whose
            // first action is action 1.
            let action_number = index + 1;

            if let Some(id) = action.id()
                && let Some(first) = seen.insert(id, action_number)
            {
                return Err(Invalid::DuplicateActionId {
                    id: id.clone(),
                    first,
                    second: action_number,
                });
            }

            // Each variant names the paths it declares; `source` is not a field
            // all of them have. No wildcard, so one added later fails to compile
            // until someone says which of its fields are paths.
            match action {
                Action::Symlink(symlink) => {
                    check_source(&symlink.source, action_number)?;
                    check_dest(&symlink.dest, action_number)?;
                }
                Action::SymlinkDir(symlink_dir) => {
                    // The same two rules: a `source-dir` is a source and a
                    // `dest-dir` is a destination. Refusing the repository root
                    // matters more here — it would link `batfiles.toml` and
                    // `.git` into the home rather than install one of them.
                    check_source(&symlink_dir.source_dir, action_number)?;
                    check_dest(&symlink_dir.dest_dir, action_number)?;
                }
                // The one action with nothing to install, so the only one whose
                // paths are all destination and no source.
                Action::CreateDir(create_dir) => check_dest(&create_dir.dest, action_number)?,
            }
        }
        Ok(())
    }
}

/// A manifest that parsed but breaks one of [`Manifest::validate`]'s rules.
///
/// The manifest path is not here; [`Manifest::load`] adds it once, so no rule
/// carries it.
#[derive(Debug, Error)]
pub(crate) enum Invalid {
    #[error("action {second} repeats the id `{id}`, which action {first} already uses")]
    DuplicateActionId {
        id: ItemId,
        first: usize,
        second: usize,
    },

    // A `source` names a path within the repository that declared it. Every
    // rule below is decided from the written value alone.
    #[error("action {action}: source is empty; a source names a path within the repository")]
    SourceEmpty { action: usize },

    /// A source naming its own starting point — a leading `/`, a `\`, or a
    /// drive letter — rather than one relative to the repository.
    #[error("action {action}: source `{value}` is not relative to the repository root")]
    SourceNotRelative { action: usize, value: String },

    #[error("action {action}: source `{value}` resolves outside the repository")]
    SourceOutsideRepository { action: usize, value: String },

    /// A source that stays inside the repository but names all of it.
    #[error(
        "action {action}: source `{value}` names the whole repository; \
         a source names a path within it"
    )]
    SourceIsRepositoryRoot { action: usize, value: String },

    // A `dest` names a path on the machine, anchored to the selected home.
    #[error("action {action}: dest is empty; write `~` for the home directory itself")]
    DestinationEmpty { action: usize },

    #[error(
        "action {action}: dest `{value}` names another user's home; \
         `~` expands only to the selected home"
    )]
    DestinationOtherHome { action: usize, value: String },
}

/// The rules a `source` satisfies as written.
///
/// No repository is needed: a source is relative, so how far it climbs is a
/// property of its own components.
fn check_source(source: &str, action: usize) -> Result<(), Invalid> {
    if source.is_empty() {
        return Err(Invalid::SourceEmpty { action });
    }
    if is_anchored(Path::new(source)) {
        return Err(Invalid::SourceNotRelative {
            action,
            value: source.to_owned(),
        });
    }
    match depth_within_repository(source) {
        None => Err(Invalid::SourceOutsideRepository {
            action,
            value: source.to_owned(),
        }),
        // Zero components deep is the repository root itself, however it was
        // spelled: `.`, `./`, and `shell/..` all land there.
        Some(0) => Err(Invalid::SourceIsRepositoryRoot {
            action,
            value: source.to_owned(),
        }),
        Some(_) => Ok(()),
    }
}

/// Whether a path starts from somewhere of its own rather than from wherever it
/// is joined onto.
///
/// Not [`Path::is_absolute`]: on Windows that holds only for a path carrying
/// both a drive and a root, so `/etc/hosts` and `C:config` are not absolute
/// there, yet `join` honors each one by discarding what it joined to.
fn is_anchored(source: &Path) -> bool {
    matches!(
        source.components().next(),
        Some(Component::RootDir | Component::Prefix(_))
    )
}

/// How many components deep a relative path lands, or `None` if it does not stay
/// within the tree it is relative to.
///
/// No wildcard arm: counting a root or a prefix as ordinary depth is what a
/// wildcard here does, and it is wrong in the direction that lets a path out.
fn depth_within_repository(source: &str) -> Option<usize> {
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
///
/// Only the `~` forms are constrained: a `dest` is free to be absolute or to
/// traverse out of the home, so there is no containment rule here.
fn check_dest(dest: &str, action: usize) -> Result<(), Invalid> {
    if dest.is_empty() {
        return Err(Invalid::DestinationEmpty { action });
    }
    // `~` alone and `~/…` mean the selected home. `~other` is another user's,
    // which batfiles does not look up.
    match dest.strip_prefix('~') {
        Some(rest) if !rest.is_empty() && !rest.starts_with('/') => {
            Err(Invalid::DestinationOtherHome {
                action,
                value: dest.to_owned(),
            })
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_error(source: &str) -> Invalid {
        check_source(source, 1).expect_err("expected the source to be refused")
    }

    #[test]
    fn a_source_names_a_path_within_its_repository() {
        for accepted in ["shell/zshrc", "editor/nvim", "a/../b", "./bin/batgrep"] {
            assert!(
                check_source(accepted, 1).is_ok(),
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
                    Invalid::SourceNotRelative { .. } | Invalid::SourceOutsideRepository { .. }
                ),
                "`{escaping}` was accepted"
            );
        }
    }

    #[test]
    fn a_component_that_starts_a_path_over_is_never_ordinary_depth() {
        // Which spellings produce a root or a prefix is platform-specific — see
        // the test below — so this covers the handling, not the parsing.
        assert_eq!(depth_within_repository("/etc/hosts"), None);
        assert_eq!(depth_within_repository("shell/zshrc"), Some(2));
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
        // Installing the repository root would put `batfiles.toml` and `.git`
        // in the home. How it was spelled decides only which diagnostic it
        // gets, not whether it is refused.
        assert!(matches!(source_error(""), Invalid::SourceEmpty { .. }));
        for whole in [".", "./", "shell/.."] {
            assert!(
                matches!(source_error(whole), Invalid::SourceIsRepositoryRoot { .. }),
                "`{whole}` was not recognized as the whole repository"
            );
        }
    }

    #[test]
    fn a_dest_may_be_anywhere_the_selected_home_can_reach() {
        // The home is a base, not a boundary: someone linking onto another
        // volume or into a sibling directory is expressing intent.
        for accepted in ["~", "~/.zshrc", ".zshrc", "/etc/hosts", "~/../shared/rc"] {
            assert!(check_dest(accepted, 1).is_ok(), "`{accepted}` was refused");
        }
    }

    #[test]
    fn a_dest_is_written_out_rather_than_left_empty() {
        // `~` says "the home directory itself" and an empty value only looks
        // like it, so the explicit spelling is the one batfiles accepts.
        assert!(matches!(
            check_dest("", 1).expect_err("expected an empty dest to be refused"),
            Invalid::DestinationEmpty { .. }
        ));
    }

    #[test]
    fn only_the_selected_home_is_spelled_with_a_tilde() {
        assert!(matches!(
            check_dest("~other/.zshrc", 1).expect_err("expected another user's home to be refused"),
            Invalid::DestinationOtherHome { .. }
        ));
    }
}
