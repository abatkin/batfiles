//! Git, file, and archive sources declared in `[remotes]`.

use serde::Deserialize;

use super::DeclarationName;
use super::check::{
    ManifestError, ValueRule, check_archive_root, check_conditions, check_optional_template,
    check_template,
};
use crate::manifest::condition::{Condition, Gate};
use crate::manifest::entry_filter::GlobFilter;
use crate::manifest::item::ItemId;
use crate::manifest::template::TemplateString;

/// The tree a declaration or record is read from: the leaf repository, or the materialization
/// of the remote with this ID. `Id` is `&ItemId` where the ID is borrowed and `ItemId` where it
/// is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Tree<Id> {
    Leaf,
    Remote(Id),
}

impl<Id> Tree<Id> {
    /// The remote's ID, or `None` for the leaf.
    pub fn remote(&self) -> Option<&Id> {
        match self {
            Self::Leaf => None,
            Self::Remote(id) => Some(id),
        }
    }

    /// The same tree, borrowing its ID.
    pub fn as_ref(&self) -> Tree<&Id> {
        match self {
            Self::Leaf => Tree::Leaf,
            Self::Remote(id) => Tree::Remote(id),
        }
    }
}

impl Tree<&ItemId> {
    /// The same tree, owning its ID.
    pub fn cloned(self) -> Tree<ItemId> {
        match self {
            Self::Leaf => Tree::Leaf,
            Self::Remote(id) => Tree::Remote(id.clone()),
        }
    }
}

impl<Id> From<Option<Id>> for Tree<Id> {
    /// `None` is the leaf.
    fn from(remote: Option<Id>) -> Self {
        remote.map_or(Self::Leaf, Self::Remote)
    }
}

/// A remote declaration, selected by its required `type` field and identified by its map key.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum Remote {
    Git(GitRemote),
    File(FileRemote),
    Archive(ArchiveRemote),
}

impl Remote {
    /// Validate the remote's fields and condition combination, naming it by `id` in errors.
    pub fn validate(&self, id: &ItemId) -> Result<(), ManifestError> {
        let record = DeclarationName::Remote(id.clone());
        let (when, unless) = self.gate_fields();
        check_conditions(when, unless, &record)?;
        match self {
            Self::Git(remote) => {
                check_template(&remote.url, "url", ValueRule::GitSource, &record)?;
                check_optional_template(remote.git_ref.as_ref(), "ref", ValueRule::GitRef, &record)
            }
            Self::File(remote) => {
                check_template(&remote.url, "url", ValueRule::Url, &record)?;
                check_optional_template(
                    remote.sha256.as_ref(),
                    "sha256",
                    ValueRule::Digest,
                    &record,
                )
            }
            Self::Archive(remote) => {
                check_template(&remote.url, "url", ValueRule::Url, &record)?;
                check_optional_template(
                    remote.sha256.as_ref(),
                    "sha256",
                    ValueRule::Digest,
                    &record,
                )?;
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

    /// Whether templates in an included manifest's records may read `env`. Only a `git`
    /// remote has a manifest to include.
    pub fn allows_env_templates(&self) -> bool {
        match self {
            Self::Git(remote) => remote.allow_env_templates,
            Self::File(_) | Self::Archive(_) => false,
        }
    }

    /// Return the remote's admission condition gate, if declared.
    pub fn gate(&self) -> Option<Gate<'_>> {
        let (when, unless) = self.gate_fields();
        Gate::from_fields(when, unless)
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

/// `git`: a repository cloned and updated under `remotes/<id>`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GitRemote {
    /// Repository source passed to Git unchanged.
    pub url: TemplateString,

    /// The branch, tag, or commit the materialization should be on, following
    /// [`git-clone`](../../docs/repoformat.md#ref-following-one-branch-tag-or-commit)'s
    /// rules. Absent, the materialization follows whatever branch it is on.
    #[serde(rename = "ref")]
    pub git_ref: Option<TemplateString>,

    /// Whether the manifest an `include-remote` reads from this remote may run
    /// its dynamic variables' commands.
    #[serde(default)]
    pub allow_dynamic_vars: bool,

    /// Whether templates in the manifest an `include-remote` reads from this remote may read
    /// the `env` namespace.
    #[serde(default)]
    pub allow_env_templates: bool,

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
    pub url: TemplateString,
    /// The digest the fetched bytes must have, if the manifest pins one.
    pub sha256: Option<TemplateString>,
    /// Whether the file lands executable.
    #[serde(default)]
    pub executable: bool,
    /// Whether the download is a gzip or bzip2 stream whose contents are the file.
    #[serde(default)]
    pub decompress: bool,

    /// The condition under which this machine materializes the remote at all.
    pub when: Option<Condition>,
    /// The condition under which it does not.
    pub unless: Option<Condition>,
}

/// `archive`: a tarball fetched from a URL and unpacked into `remotes/<id>`,
/// by the rules a [`fetch-archive`](super::actions::FetchArchiveAction) follows.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct ArchiveRemote {
    /// An `http://`, `https://`, or `file://` URL.
    pub url: TemplateString,
    /// The digest the fetched archive must have, if the manifest pins one.
    pub sha256: Option<TemplateString>,
    /// Prefix to strip from entry paths, or `*` for the archive's single top-level directory.
    pub archive_root: Option<String>,
    /// Entries to unpack, by their path with the root stripped; absent, every entry.
    pub include: Option<GlobFilter>,
    /// Entries not to unpack, by their path with the root stripped.
    pub exclude: Option<GlobFilter>,
    /// Files to make executable, by their path with the root stripped.
    pub executable: Option<GlobFilter>,

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

    fn checked(document: &str) -> Result<(), ManifestError> {
        let remote = parse(document).unwrap_or_else(|error| panic!("{error}"));
        remote.validate(&ItemId::try_from("core".to_owned()).expect("valid ID"))
    }

    fn refused(document: &str) -> String {
        checked(document)
            .expect_err("expected the remote to be refused")
            .to_string()
    }

    /// A Git remote declaration with optional ref and condition fields.
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
        let message = refused("type = \"git\"\nurl = \"\"\n");
        assert!(message.contains("remote `core`"), "{message}");
        assert!(message.contains("url is empty"), "{message}");
    }

    #[test]
    fn a_ref_written_with_nothing_in_it_is_refused() {
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

    /// A valid SHA-256 digest for parsing tests.
    const SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn a_file_remote_takes_a_url_a_digest_and_one_condition() {
        assert!(checked(&format!("type = \"file\"\nurl = \"https://e.example/a.vim\"\nsha256 = \"{SHA}\"\nwhen = \"work\"\n")).is_ok());
        // An absolute path on Windows starts with its drive.
        let local = if cfg!(windows) {
            "file:///C:/srv/a.vim"
        } else {
            "file:///srv/a.vim"
        };
        assert!(checked(&format!("type = \"file\"\nurl = \"{local}\"\n")).is_ok());
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
        let error = parse("type = \"rsync\"\nurl = \"e.example:/a\"\n")
            .expect_err("an unknown type should not deserialize");
        assert!(error.to_string().contains("rsync"), "{error}");
    }

    #[test]
    fn every_remote_is_closed_over_the_fields_it_accepts() {
        for (document, unknown) in [
            (
                "type = \"git\"\nurl = \"https://e.example/a.git\"\nbranch = \"main\"\n",
                "branch",
            ),
            (
                "type = \"git\"\nurl = \"https://e.example/a.git\"\nsha256 = \"00\"\n",
                "sha256",
            ),
            (
                "type = \"file\"\nurl = \"https://e.example/a\"\narchive-root = \"*\"\n",
                "archive-root",
            ),
            (
                "type = \"file\"\nurl = \"https://e.example/a\"\ninclude = \"bin/*\"\n",
                "include",
            ),
            (
                "type = \"git\"\nurl = \"https://e.example/a.git\"\nexclude = \"*.md\"\n",
                "exclude",
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
