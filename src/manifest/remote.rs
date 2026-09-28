//! `[remotes]`: the sources a manifest names so its actions can reach content
//! it does not hold itself — a Git repository, a file, or an unpacked archive.

use serde::Deserialize;

use super::check::{
    Invalid, RecordName, check_archive_root, check_digest, check_git_ref, check_git_source,
    check_url,
};
use crate::condition::{Condition, Gate};
use crate::item::ItemId;

/// One entry of `[remotes]`, selected by its required `type` field.
///
/// The map key is the remote's ID, so a remote is always named: diagnostics
/// and actions refer to it by that key.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum Remote {
    Git(GitRemote),
    File(FileRemote),
    Archive(ArchiveRemote),
}

impl Remote {
    /// The rules serde cannot express, checked as the document is read.
    ///
    /// `id` is the map key the remote was declared under, which is how every
    /// diagnostic here names it.
    pub fn validate(&self, id: &ItemId) -> Result<(), Invalid> {
        let record = RecordName::Remote(id.clone());
        let (when, unless) = self.gate_fields();
        if when.is_some() && unless.is_some() {
            return Err(Invalid::BothConditions { record });
        }
        match self {
            Self::Git(remote) => {
                check_git_source(&remote.url, "url", &record)?;
                check_git_ref(remote.git_ref.as_deref(), &record)
            }
            Self::File(remote) => {
                check_url(&remote.url, "url", &record)?;
                check_digest(remote.sha256.as_deref(), &record)
            }
            Self::Archive(remote) => {
                check_url(&remote.url, "url", &record)?;
                check_digest(remote.sha256.as_deref(), &record)?;
                check_archive_root(remote.archive_root.as_deref(), &record)
            }
        }
    }

    /// The `type` this remote was declared with, as the manifest spells it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Git(_) => "git",
            Self::File(_) => "file",
            Self::Archive(_) => "archive",
        }
    }

    /// Whether an included manifest's dynamic declarations may run their
    /// commands. Only a `git` remote has a manifest to include.
    pub fn allows_dynamic_vars(&self) -> bool {
        match self {
            Self::Git(remote) => remote.allow_dynamic_vars,
            Self::File(_) | Self::Archive(_) => false,
        }
    }

    /// The condition deciding whether this machine has the remote at all, or
    /// `None` where the record declares neither field.
    pub fn gate(&self) -> Option<Gate<'_>> {
        let (when, unless) = self.gate_fields();
        Gate::declared(when, unless)
    }

    /// The record's `when` and `unless`, in that order.
    fn gate_fields(&self) -> (Option<&Condition>, Option<&Condition>) {
        match self {
            Self::Git(remote) => (remote.when.as_ref(), remote.unless.as_ref()),
            Self::File(remote) => (remote.when.as_ref(), remote.unless.as_ref()),
            Self::Archive(remote) => (remote.when.as_ref(), remote.unless.as_ref()),
        }
    }
}

/// `git`: a repository batfiles clones and keeps up to date on its own, so that
/// actions can install from a tree the leaf repository does not hold.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GitRemote {
    /// The repository to clone, passed to git as written: what a
    /// [`GitCloneAction`](super::action::GitCloneAction) calls `source`.
    pub url: String,

    /// The branch, tag, or commit the materialization should be on, following
    /// [`git-clone`](../../docs/repoformat.md#ref-following-one-branch-tag-or-commit)'s
    /// rules. Absent, the materialization follows whatever branch it is on.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,

    /// Whether the manifest an `include-remote` reads from this remote may run
    /// its dynamic variables' commands.
    #[serde(default)]
    pub allow_dynamic_vars: bool,

    /// The condition under which this machine materializes the remote at all.
    pub when: Option<Condition>,
    /// The condition under which it does not.
    pub unless: Option<Condition>,
}

/// `file`: one file fetched from a URL, materialized as `remotes/<id>` itself.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct FileRemote {
    /// An `http://`, `https://`, or `file://` URL.
    pub url: String,
    /// The digest the fetched bytes must have, if the manifest pins one.
    pub sha256: Option<String>,

    /// The condition under which this machine materializes the remote at all.
    pub when: Option<Condition>,
    /// The condition under which it does not.
    pub unless: Option<Condition>,
}

/// `archive`: a tarball fetched from a URL and unpacked into `remotes/<id>`,
/// by the rules a [`fetch-archive`](super::action::FetchArchiveAction) follows.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct ArchiveRemote {
    /// An `http://`, `https://`, or `file://` URL.
    pub url: String,
    /// The digest the fetched archive must have, if the manifest pins one.
    pub sha256: Option<String>,
    /// A prefix every entry is written without, or `*` for the archive's
    /// single top-level directory.
    pub archive_root: Option<String>,

    /// The condition under which this machine materializes the remote at all.
    pub when: Option<Condition>,
    /// The condition under which it does not.
    pub unless: Option<Condition>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(document: &str) -> Result<Remote, toml::de::Error> {
        toml::from_str(document)
    }

    fn checked(document: &str) -> Result<(), Invalid> {
        let remote = parse(document).unwrap_or_else(|error| panic!("{error}"));
        remote.validate(&ItemId::try_from("core".to_owned()).expect("valid ID"))
    }

    fn refused(document: &str) -> String {
        checked(document)
            .expect_err("expected the remote to be refused")
            .to_string()
    }

    /// The whole of a Git remote, which is one required field and three
    /// optional ones.
    const COMPLETE: &str = "type = \"git\"\n\
         url = \"https://e.example/core.git\"\n\
         ref = \"main\"\n\
         when = \"work\"\n";

    #[test]
    fn a_git_remote_takes_a_url_a_ref_and_one_condition() {
        assert!(checked(COMPLETE).is_ok());
        assert!(checked("type = \"git\"\nurl = \"git@e.example:me/core.git\"\n").is_ok());
        assert!(
            checked("type = \"git\"\nurl = \"/srv/core.git\"\nunless = \"work\"\n").is_ok(),
            "a plain directory is a repository git can clone"
        );
    }

    #[test]
    fn a_url_is_whatever_git_accepts_but_never_nothing() {
        // The one rule decidable from the value alone, exactly as it is for a
        // `git-clone` source. What the rest means is git's question.
        let message = refused("type = \"git\"\nurl = \"\"\n");
        assert!(message.contains("remote `core`"), "{message}");
        assert!(message.contains("url is empty"), "{message}");
    }

    #[test]
    fn a_ref_written_with_nothing_in_it_is_refused() {
        // An absent `ref` follows whatever branch the materialization is on,
        // which is not what a record asking for one meant.
        let message = refused("type = \"git\"\nurl = \"https://e.example/a.git\"\nref = \" \"\n");
        assert!(message.contains("remote `core`"), "{message}");
        assert!(message.contains("ref is empty"), "{message}");
    }

    #[test]
    fn a_remote_writes_one_condition_or_none() {
        let message = refused(
            "type = \"git\"\nurl = \"https://e.example/a.git\"\nwhen = \"work\"\nunless = \"gui\"\n",
        );
        assert!(message.contains("remote `core`"), "{message}");
        assert!(
            message.contains("writes both `when` and `unless`"),
            "{message}"
        );
    }

    #[test]
    fn a_git_remote_runs_no_dynamic_commands_unless_it_says_so() {
        let allows = |document: &str| {
            parse(document)
                .unwrap_or_else(|error| panic!("{error}"))
                .allows_dynamic_vars()
        };
        assert!(!allows(COMPLETE));
        assert!(allows(
            "type = \"git\"\nurl = \"https://e.example/a.git\"\nallow-dynamic-vars = true\n"
        ));
        assert!(
            parse(
                "type = \"git\"\nurl = \"https://e.example/a.git\"\nallow-dynamic-vars = \"yes\"\n"
            )
            .is_err()
        );
    }

    /// A digest of the right shape, which nothing here fetches.
    const SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn a_file_remote_takes_a_url_a_digest_and_one_condition() {
        assert!(checked(&format!("type = \"file\"\nurl = \"https://e.example/a.vim\"\nsha256 = \"{SHA}\"\nwhen = \"work\"\n")).is_ok());
        assert!(checked("type = \"file\"\nurl = \"file:///srv/a.vim\"\n").is_ok());
        let message = refused("type = \"file\"\nurl = \"e.example/a.vim\"\n");
        assert!(
            message.contains("remote `core`: url `e.example/a.vim`"),
            "{message}"
        );
        let message = refused("type = \"file\"\nurl = \"https://e.example/a\"\nsha256 = \"00\"\n");
        assert!(message.contains("sha256 `00`"), "{message}");
        let message = refused(
            "type = \"file\"\nurl = \"https://e.example/a\"\nwhen = \"a\"\nunless = \"b\"\n",
        );
        assert!(
            message.contains("writes both `when` and `unless`"),
            "{message}"
        );
    }

    #[test]
    fn an_archive_remote_takes_what_a_fetch_archive_does() {
        assert!(
            checked(&format!(
                "type = \"archive\"\nurl = \"https://e.example/a.tar.gz\"\nsha256 = \"{SHA}\"\n\
                 archive-root = \"*\"\nunless = \"work\"\n"
            ))
            .is_ok()
        );
        let message = refused(
            "type = \"archive\"\nurl = \"https://e.example/a.tar.gz\"\narchive-root = \"../x\"\n",
        );
        assert!(
            message.contains("remote `core`: archive-root `../x`"),
            "{message}"
        );
    }

    #[test]
    fn only_a_git_remote_may_run_an_included_manifests_commands() {
        for document in [
            "type = \"file\"\nurl = \"https://e.example/a.vim\"\n",
            "type = \"archive\"\nurl = \"https://e.example/a.tar.gz\"\n",
        ] {
            let remote = parse(document).unwrap_or_else(|error| panic!("{error}"));
            assert!(!remote.allows_dynamic_vars());
        }
        assert!(
            parse("type = \"file\"\nurl = \"https://e.example/a\"\nallow-dynamic-vars = true\n")
                .is_err(),
            "a file has no manifest to include"
        );
    }

    #[test]
    fn an_unknown_type_is_not_a_remote_at_all() {
        // A remote is selected by its `type`, and there is no record to check.
        let error = parse("type = \"rsync\"\nurl = \"e.example:/a\"\n")
            .expect_err("an unknown type should not deserialize");
        assert!(error.to_string().contains("rsync"), "{error}");
    }

    #[test]
    fn every_remote_is_closed_over_the_fields_it_accepts() {
        for (document, unknown) in [
            // The spelling `docs/future/repoformat.md` gives the ref field,
            // which batfiles reads under `git-clone`'s name instead.
            (
                "type = \"git\"\nurl = \"https://e.example/a.git\"\nbranch = \"main\"\n",
                "branch",
            ),
            // A field belonging to another variant.
            (
                "type = \"git\"\nurl = \"https://e.example/a.git\"\nsha256 = \"00\"\n",
                "sha256",
            ),
            (
                "type = \"file\"\nurl = \"https://e.example/a\"\narchive-root = \"*\"\n",
                "archive-root",
            ),
            // The entry filters, which are not built for an archive remote any
            // more than for a `fetch-archive`.
            (
                "type = \"archive\"\nurl = \"https://e.example/a.tar.gz\"\ninclude = \"bin/*\"\n",
                "include",
            ),
        ] {
            let error = parse(document).expect_err("a closed record should reject the field");
            assert!(
                error
                    .to_string()
                    .contains(&format!("unknown field `{unknown}`")),
                "{error}"
            );
        }
    }
}
