//! The rules a manifest has to satisfy that TOML cannot express, and what
//! batfiles says when one is broken.
//!
//! Two kinds of rule, per
//! [`docs/repoformat.md`](../../docs/repoformat.md#reading-the-manifest): one
//! about the shape of a single value that its type does not capture, which is
//! every `check_*` below, and one spanning more than one record, which is
//! [`Manifest::validate`](super::Manifest::validate)'s. Both are decidable from
//! the document alone, so both are settled while it is being read.

use std::fmt;
use std::path::{Component, Path};

use thiserror::Error;

use crate::item::ItemId;
use crate::repo_path::{REMOTE_PREFIX, RepoPath};

/// How a load diagnostic names the record that broke the rule.
///
/// Every message [`Invalid`] renders opens with one, so this is what a reader
/// looks at first: which of the things in the file is the one to go and fix.
/// The three spellings are three different answers to that, because the three
/// kinds of record are identified differently. An action need not have an `id`,
/// so it is named by its position. A bootstrap candidate is named by its
/// position within its own array, since what it *names* is deliberately never
/// looked up and so cannot identify it. A remote is named outright, its map key
/// being its ID.
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

// The path rules below are the same rules whichever tree a path is read from;
// only the noun changes, and a path says for itself which one that is. Three
// spellings of the one noun, because a sentence about a root and a sentence
// about a whole tree each read better with their own phrasing.

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

    /// Two remote IDs that a case-folding filesystem cannot tell apart.
    ///
    /// Refused on every platform, not only the ones that would fold them: a
    /// manifest describes one repository across all of a person's machines, and
    /// a rule that held on Linux alone would move the failure to the machine
    /// least able to explain it.
    #[error(
        "remotes `{one}` and `{other}` differ only in case; where the filesystem \
         ignores case they are one directory under `remotes/`, and only one of \
         the two repositories would ever be cloned"
    )]
    RemotesShareOneDirectory { one: ItemId, other: ItemId },

    /// A record writing both spellings of a condition. Refused rather than
    /// resolved, because the two are not one rule and its negation and there is
    /// no reading of the pair that is obviously the one that was meant.
    ///
    /// Every kind of record that takes a condition enforces it, which is why
    /// this is one variant over a [`RecordName`] rather than one variant per kind.
    #[error("{record}: writes both `when` and `unless`; a record has one condition or none")]
    BothConditions { record: RecordName },

    // A `source` names a path within one repository: the one that declared it,
    // or the materialization of a remote it names with `@`. Every rule below is
    // decided from the written value alone, and holds the same way for both.
    //
    // Each carries the path rather than a rendering of it, so that what a
    // message quotes and what it says about the tree are one value and cannot
    // come apart.
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

    /// A path under a remote reference, or the `path` of a structured one, that
    /// begins with the character a remote reference begins with. Reserved on
    /// both halves of the rule, so that `@` at the start of a repository path
    /// means one thing wherever it is written.
    #[error(
        "{record}: source `{written}` starts with `@`, which introduces a remote \
         reference and cannot start a path within one"
    )]
    SourceStartsWithRemotePrefix {
        record: RecordName,
        written: RepoPath,
    },

    // A `dest` names a path on the machine, anchored to the selected home.
    #[error("{record}: dest is empty; write `~` for the home directory itself")]
    DestinationEmpty { record: RecordName },

    #[error(
        "{record}: dest `{value}` names another user's home; \
         `~` expands only to the selected home"
    )]
    DestinationOtherHome { record: RecordName, value: String },

    // A fetching action's source names somewhere off this machine, and its
    // digest names what should arrive from there.
    #[error("{record}: source `{value}` is not an http:// or https:// URL")]
    SourceNotAUrl { record: RecordName, value: String },

    /// A `file://` source, which the format reserves but nothing fetches yet.
    /// Named apart from any other unusable scheme because it is the one a
    /// reader of `docs/future/repoformat.md` has reason to expect to work.
    // CARRY(9.3): file and archive remotes are where a `file://` source starts
    // being fetched; delete this variant and its check then.
    #[error(
        "{record}: source `{value}` is a `file://` URL, which arrives with \
         file remotes at step 9.3; use a `copy` action for a path on this machine"
    )]
    SourceIsFileUrl { record: RecordName, value: String },

    #[error("{record}: sha256 `{value}` is not 64 hexadecimal digits")]
    DigestNotSha256 { record: RecordName, value: String },

    // A repository for git to clone, named in any of the several ways git
    // spells one. Emptiness is the only thing decidable from the value alone.
    // A `git-clone` action and a Git remote share both rules below.
    #[error("{record}: {field} is empty; it names a repository for git to clone")]
    GitSourceEmpty {
        record: RecordName,
        /// What the record spells it: `source` on an action, `url` on a remote.
        field: &'static str,
    },

    /// A `ref` written with nothing in it. Refused rather than read as an
    /// absent one, which follows whatever branch the clone is on and is not
    /// what a record asking for a ref meant.
    #[error("{record}: ref is empty; a ref names a branch, tag, or commit to follow")]
    GitRefEmpty { record: RecordName },

    /// An `archive-root` no entry batfiles would unpack could ever match. An
    /// escaping entry is refused as the archive is read, so a prefix that only
    /// selects escaping entries selects nothing, and saying so here is better
    /// than downloading the archive to find out.
    #[error(
        "{record}: archive-root `{value}` is not a path inside the archive; \
         write a prefix such as `tool-1.0`, or `*` for the archive's single top-level directory"
    )]
    ArchiveRootNotInside { record: RecordName, value: String },

    /// A remote type the schema reserves and nothing materializes yet. Named
    /// apart from a type that is not in the schema at all, for the reason
    /// [`Self::SourceIsFileUrl`] is: this is one a reader of
    /// `docs/future/repoformat.md` has reason to expect to work.
    // CARRY(9.3): file and archive remotes are what these two types become;
    // delete this variant and the arms that raise it then.
    #[error(
        "{record}: type `{kind}` arrives with file and archive remotes at step 9.3; \
         declare a `git` remote, or fetch the content with a fetch-file or fetch-archive action"
    )]
    RemoteTypeUnbuilt {
        record: RecordName,
        kind: &'static str,
    },
}

/// The rules a `source` satisfies as written.
// CARRY(6.4): the records carry a `RepoPath` themselves once actions may source
// from a remote, and this shim goes with the last caller that holds a string.
pub(super) fn check_source(source: &str, record: &RecordName) -> Result<(), Invalid> {
    check_repo_path(&RepoPath::local(source.to_owned()), record)
}

/// The rules a repository path satisfies as written, wherever it is read from.
///
/// Everything here is decided from the value alone. Whether a named remote is
/// one the manifest declares spans two records and so is the document's to
/// check; whether anything is at the path is the run's.
pub(super) fn check_repo_path(written: &RepoPath, record: &RecordName) -> Result<(), Invalid> {
    let path = written.path();
    // Only a plain string can say nothing at all; a reference that names a
    // remote and no path has named a tree, and is refused as naming all of it.
    if written.remote().is_none() && path.is_empty() {
        return Err(Invalid::SourceEmpty {
            record: record.clone(),
        });
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
        None => Err(Invalid::SourceOutsideTree {
            record: record.clone(),
            written: written.clone(),
        }),
        // Zero components deep is the tree's own root, however it was spelled:
        // `.`, `./`, `shell/..`, and a reference with no path all land there.
        Some(0) => Err(Invalid::SourceIsWholeTree {
            record: record.clone(),
            written: written.clone(),
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

/// The rules a fetching action's `source` satisfies as written.
pub(super) fn check_url(source: &str, record: &RecordName) -> Result<(), Invalid> {
    let scheme = |prefix: &str| {
        let (source, prefix) = (source.as_bytes(), prefix.as_bytes());
        source.len() > prefix.len() && source[..prefix.len()].eq_ignore_ascii_case(prefix)
    };
    if scheme("http://") || scheme("https://") {
        return Ok(());
    }
    if scheme("file://") {
        return Err(Invalid::SourceIsFileUrl {
            record: record.clone(),
            value: source.to_owned(),
        });
    }
    Err(Invalid::SourceNotAUrl {
        record: record.clone(),
        value: source.to_owned(),
    })
}

/// The rules a repository for git to clone satisfies as written, of which there
/// is one. `field` is what the record spells it: a `git-clone` action writes
/// `source`, a Git remote writes `url`.
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

/// The rules a `ref` satisfies as written, of which there is one. Shared by a
/// `git-clone` action and a Git remote, which follow a ref the same way.
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

    /// The record the checks below are written against; which record a
    /// diagnostic names is [`RecordName`]'s own test.
    fn record() -> RecordName {
        RecordName::Action(1)
    }

    fn source_error(source: &str) -> Invalid {
        check_source(source, &record()).expect_err("expected the source to be refused")
    }

    #[test]
    fn every_kind_of_record_is_named_the_way_its_document_names_it() {
        // The spellings a load diagnostic opens with. The first two are
        // positions, because neither kind of record is required to have a name;
        // a remote has one by construction, since its map key is its ID.
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
                check_source(accepted, &record()).is_ok(),
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
        check_repo_path(&remote_path(written), &record())
            .expect_err("expected the source to be refused")
    }

    #[test]
    fn a_remote_reference_follows_the_same_rules_as_a_local_source() {
        for accepted in ["@core/shell/zshrc", "@core/a/../b", "@core/./bin/batgrep"] {
            assert!(
                check_repo_path(&remote_path(accepted), &record()).is_ok(),
                "`{accepted}` was refused"
            );
        }
    }

    #[test]
    fn a_refused_remote_reference_names_the_remote_rather_than_the_repository() {
        // The same three refusals, about the tree the path is actually read
        // from: a message naming the repository would send someone to the wrong
        // one of the two.
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
            check_repo_path(&structured["source"], &record())
                .expect_err("expected the source to be refused"),
            Invalid::SourceStartsWithRemotePrefix { .. }
        ));
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
                check_url(accepted, &record()).is_ok(),
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
                    check_url(refused, &record()).expect_err("expected the source to be refused"),
                    Invalid::SourceNotAUrl { .. }
                ),
                "`{refused}` was accepted"
            );
        }
    }

    #[test]
    fn a_file_url_says_which_step_makes_it_work() {
        assert!(matches!(
            check_url("file:///etc/hosts", &record())
                .expect_err("expected a file URL to be refused"),
            Invalid::SourceIsFileUrl { .. }
        ));
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
