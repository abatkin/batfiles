//! `batfiles.toml`: the one file in a repository with intrinsic meaning.
//!
//! The record mirrors the file. A rule serde cannot express — one spanning two
//! records, or one about the shape of a value rather than its type — is checked
//! by [`Manifest::validate`] as the document is read, so that a manifest which
//! cannot be honored is refused before any of it is acted on.

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

    /// The rules serde cannot express, checked as the document is read.
    ///
    /// One of them is the manifest's own: IDs are unique across records, which
    /// no single record can check. The rest belong to the records, and
    /// [`Action::check_paths`] is where one answers for its own fields.
    ///
    /// A rule belongs here at all when it is decidable from the document alone.
    /// One needing a resolved root or the filesystem — whether a `source`
    /// exists — is not, and stays with the action as it runs.
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

            // Which of a record's fields are paths, and which rule each one
            // follows, is the record's own answer.
            action.check_paths(action_number)?;
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

/// The rules a fetching action's `source` satisfies as written.
///
/// Only the scheme is checked. What the rest of a URL may say is the server's
/// business, and a client that will parse it properly is already a dependency —
/// so this refuses what batfiles knows it cannot fetch and leaves the rest to
/// fail at the fetch, where the diagnostic can say what the network said.
fn check_url(source: &str, action: usize) -> Result<(), Invalid> {
    // Compared as bytes, not by slicing the string: a `source` is whatever the
    // author typed, so an offset inside a scheme-length prefix can land in the
    // middle of a character, and slicing there panics. A scheme is ASCII, so
    // the bytes answer the question exactly.
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

/// The shape a `sha256` has to have to be one.
///
/// Checked here rather than at the fetch so that a typo fails before anything
/// is downloaded: a digest that cannot match is a repository bug, and finding
/// out after the transfer wastes the transfer and reports the wrong thing.
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
        // A repository path is the mistake worth catching: it would otherwise
        // reach the fetcher and fail as a network error, naming DNS rather than
        // the manifest. The last three are multibyte: a scheme-length offset
        // into one lands mid-character, and deciding this by slicing there
        // panics on a manifest someone wrote by hand.
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
        // `docs/future/repoformat.md` lists `file://` among the schemes, so
        // someone will write one; it gets its own diagnostic rather than being
        // called malformed.
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
    fn only_the_selected_home_is_spelled_with_a_tilde() {
        assert!(matches!(
            check_dest("~other/.zshrc", 1).expect_err("expected another user's home to be refused"),
            Invalid::DestinationOtherHome { .. }
        ));
    }
}
