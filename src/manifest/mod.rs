//! `batfiles.toml`: the one file in a repository with intrinsic meaning.

pub(crate) mod action;
pub(crate) mod default_disabled;

use std::collections::BTreeMap;
use std::path::{Component, Path};

use serde::Deserialize;
use thiserror::Error;

use crate::error::Error as CrateError;
use crate::item::ItemId;
use crate::manifest::action::Action;
use crate::manifest::default_disabled::DefaultDisabled;
use crate::tomlfile;
use crate::var::VarName;

/// A parsed `batfiles.toml`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct Manifest {
    /// The ordered action list. Order is significant, so this is the one
    /// section that is a sequence rather than a map.
    #[serde(default)]
    pub actions: Vec<Action>,

    /// Static variable values, keyed by name. Every value is a string: serde
    /// settles both rules as the document is read, so a name that breaks the
    /// rule and a value that is not a string each fail the document at the line
    /// they are written on, and nothing here re-checks either.
    #[expect(dead_code, reason = "merged at 5.4")]
    #[serde(default)]
    pub vars: BTreeMap<VarName, String>,

    /// What a fresh machine starts with switched off. Accepted and checked as
    /// the document is read; nothing acts on it.
    #[expect(
        dead_code,
        reason = "walked at 5.6, to reject an entry setting both conditions"
    )]
    #[serde(default)]
    pub default_disabled: DefaultDisabled,
}

impl Manifest {
    /// The manifest's name within a repository root.
    pub const FILE_NAME: &'static str = "batfiles.toml";

    /// Read, parse, and check one manifest.
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

    /// The rules serde cannot express, checked as the document is read.
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

            // Which of a record's fields are paths, and which rule each one
            // follows, is the record's own answer.
            action.validate(action_number)?;
        }
        Ok(())
    }
}

/// A manifest that parsed but breaks one of [`Manifest::validate`]'s rules.
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

    // A fetching action's source names somewhere off this machine, and its
    // digest names what should arrive from there.
    #[error("action {action}: source `{value}` is not an http:// or https:// URL")]
    SourceNotAUrl { action: usize, value: String },

    /// A `file://` source, which the format reserves but nothing fetches yet.
    /// Named apart from any other unusable scheme because it is the one a
    /// reader of `docs/future/repoformat.md` has reason to expect to work.
    // CARRY(9.3): file and archive remotes are where a `file://` source starts
    // being fetched; delete this variant and its check then.
    #[error(
        "action {action}: source `{value}` is a `file://` URL, which arrives with \
         file remotes at step 9.3; use a `copy` action for a path on this machine"
    )]
    SourceIsFileUrl { action: usize, value: String },

    #[error("action {action}: sha256 `{value}` is not 64 hexadecimal digits")]
    DigestNotSha256 { action: usize, value: String },

    // A `git-clone` source names a repository, in any of the several ways git
    // spells one. Emptiness is the only thing decidable from the value alone.
    #[error("action {action}: source is empty; a source names a repository for git to clone")]
    GitSourceEmpty { action: usize },

    /// A `ref` written with nothing in it. Refused rather than read as an
    /// absent one, which follows whatever branch the clone is on and is not
    /// what a record asking for a ref meant.
    #[error("action {action}: ref is empty; a ref names a branch, tag, or commit to follow")]
    GitRefEmpty { action: usize },

    /// An `archive-root` no entry batfiles would unpack could ever match. An
    /// escaping entry is refused as the archive is read, so a prefix that only
    /// selects escaping entries selects nothing, and saying so here is better
    /// than downloading the archive to find out.
    #[error(
        "action {action}: archive-root `{value}` is not a path inside the archive; \
         write a prefix such as `tool-1.0`, or `*` for the archive's single top-level directory"
    )]
    ArchiveRootNotInside { action: usize, value: String },
}

/// The rules a `source` satisfies as written.
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
    match depth_within_tree(source) {
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

/// The rules a fetching action's `source` satisfies as written.
fn check_url(source: &str, action: usize) -> Result<(), Invalid> {
    let scheme = |prefix: &str| {
        let (source, prefix) = (source.as_bytes(), prefix.as_bytes());
        source.len() > prefix.len() && source[..prefix.len()].eq_ignore_ascii_case(prefix)
    };
    if scheme("http://") || scheme("https://") {
        return Ok(());
    }
    if scheme("file://") {
        return Err(Invalid::SourceIsFileUrl {
            action,
            value: source.to_owned(),
        });
    }
    Err(Invalid::SourceNotAUrl {
        action,
        value: source.to_owned(),
    })
}

/// The rules a `git-clone` source satisfies as written, of which there is one.
fn check_git_source(source: &str, action: usize) -> Result<(), Invalid> {
    if source.trim().is_empty() {
        return Err(Invalid::GitSourceEmpty { action });
    }
    Ok(())
}

/// The rules a `git-clone` ref satisfies as written, of which there is one.
fn check_git_ref(git_ref: Option<&str>, action: usize) -> Result<(), Invalid> {
    if git_ref.is_some_and(|value| value.trim().is_empty()) {
        return Err(Invalid::GitRefEmpty { action });
    }
    Ok(())
}

/// The shape a `sha256` has to have to be one.
fn check_digest(sha256: Option<&str>, action: usize) -> Result<(), Invalid> {
    match sha256 {
        Some(value) if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) => {
            Err(Invalid::DigestNotSha256 {
                action,
                value: value.to_owned(),
            })
        }
        _ => Ok(()),
    }
}

/// The shape an `archive-root` has to have to name something inside an archive.
fn check_archive_root(archive_root: Option<&str>, action: usize) -> Result<(), Invalid> {
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
            action,
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
    fn a_fetched_source_names_a_scheme_batfiles_can_fetch() {
        for accepted in [
            "http://example.com/a",
            "https://example.com/a",
            "HTTPS://EXAMPLE.COM/A",
            // Everything after the scheme is the server's business, including
            // what looks like nonsense from here.
            "https://example.com/a b?c=d#e",
        ] {
            assert!(check_url(accepted, 1).is_ok(), "`{accepted}` was refused");
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
                    check_url(refused, 1).expect_err("expected the source to be refused"),
                    Invalid::SourceNotAUrl { .. }
                ),
                "`{refused}` was accepted"
            );
        }
    }

    #[test]
    fn a_file_url_says_which_step_makes_it_work() {
        assert!(matches!(
            check_url("file:///etc/hosts", 1).expect_err("expected a file URL to be refused"),
            Invalid::SourceIsFileUrl { .. }
        ));
    }

    #[test]
    fn a_digest_is_sixty_four_hexadecimal_digits_or_absent() {
        let sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        assert!(check_digest(None, 1).is_ok());
        assert!(check_digest(Some(sha), 1).is_ok());
        assert!(
            check_digest(Some(&sha.to_uppercase()), 1).is_ok(),
            "a digest written in capitals is the same digest"
        );
        // Too short, too long, and the right length with a non-digit in it.
        for refused in [&sha[..63], &format!("{sha}0")[..], &sha.replace('e', "g")] {
            assert!(
                matches!(
                    check_digest(Some(refused), 1).expect_err("expected the digest to be refused"),
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
                check_archive_root(accepted, 1).is_ok(),
                "`{accepted:?}` was refused"
            );
        }
    }

    #[test]
    fn an_archive_root_that_could_match_no_entry_is_refused_as_written() {
        for refused in ["", "/tool", "../tool", ".", "tool/..", "releases/../tool"] {
            assert!(
                matches!(
                    check_archive_root(Some(refused), 1)
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
            check_dest("~other/.zshrc", 1).expect_err("expected another user's home to be refused"),
            Invalid::DestinationOtherHome { .. }
        ));
    }
}
