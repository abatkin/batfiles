//! `[remotes]`: the repositories a manifest names so its actions can reach
//! content it does not hold itself.

use serde::Deserialize;

use super::check::{Invalid, RecordName, check_git_ref, check_git_source};
use crate::condition::Condition;
use crate::item::ItemId;

/// One entry of `[remotes]`, selected by its required `type` field.
///
/// The map key is the remote's ID, so unlike an action a remote is always
/// named. That is what a diagnostic about one uses, and what an action writes
/// to reach its content.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum Remote {
    Git(GitRemote),
    /// Reserved by the schema and refused by [`Remote::validate`] until 9.3.
    File(Unbuilt),
    /// The same.
    Archive(Unbuilt),
}

impl Remote {
    /// The rules serde cannot express, checked as the document is read.
    ///
    /// `id` is the map key the remote was declared under, which is how every
    /// diagnostic here names it.
    pub fn validate(&self, id: &ItemId) -> Result<(), Invalid> {
        let record = RecordName::Remote(id.clone());
        match self {
            Self::Git(remote) => remote.validate(&record),
            // CARRY(9.3): file and archive remotes are what makes these two
            // records mean something; delete this arm, `Unbuilt`, and
            // `Invalid::RemoteTypeUnbuilt` then.
            Self::File(_) => Err(unbuilt(record, "file")),
            Self::Archive(_) => Err(unbuilt(record, "archive")),
        }
    }
}

/// Why a reserved remote type is refused, named apart from an unknown one
/// because a reader of `docs/future/repoformat.md` has reason to expect it to
/// work.
fn unbuilt(record: RecordName, kind: &'static str) -> Invalid {
    Invalid::RemoteTypeUnbuilt { record, kind }
}

/// A remote type the schema reserves and nothing materializes yet.
///
/// Deliberately not closed and holding nothing: the record is refused by its
/// `type` alone, so checking the fields around it would report the second fault
/// in a record that cannot be declared for the first reason anyway.
#[derive(Debug, Deserialize)]
pub(crate) struct Unbuilt {}

/// `git`: a repository batfiles clones and keeps up to date on its own, so that
/// actions can install from a tree the leaf repository does not hold.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GitRemote {
    /// The repository to clone, exactly as git is given it. The same value
    /// [`GitCloneAction`](super::action::GitCloneAction) spells `source`, under
    /// the name a remote declaration reads better with.
    pub url: String,

    /// The branch, tag, or commit the materialization should be on, following
    /// [`git-clone`](../../docs/repoformat.md#ref-following-one-branch-tag-or-commit)'s
    /// rules. Absent, the materialization follows whatever branch it is on.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,

    /// The condition under which this machine materializes the remote at all.
    pub when: Option<Condition>,
    /// The condition under which it does not.
    pub unless: Option<Condition>,
}

impl GitRemote {
    /// What a declared Git remote has to say to be one, which is what a
    /// `git-clone` action's `source` and `ref` have to say, under one other
    /// name.
    fn validate(&self, record: &RecordName) -> Result<(), Invalid> {
        if self.when.is_some() && self.unless.is_some() {
            return Err(Invalid::BothConditions {
                record: record.clone(),
            });
        }
        check_git_source(&self.url, "url", record)?;
        check_git_ref(self.git_ref.as_deref(), record)
    }
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
    fn a_reserved_type_names_the_step_that_builds_it() {
        // Refused by its `type` rather than by the fields around it, so the
        // record is read as the future schema writes it and the diagnostic is
        // about the one thing that is wrong with it.
        for (kind, document) in [
            (
                "file",
                "type = \"file\"\nurl = \"https://e.example/a.vim\"\nsha256 = \"00\"\n",
            ),
            (
                "archive",
                "type = \"archive\"\nurl = \"https://e.example/a.tar.gz\"\narchive-root = \"*\"\n",
            ),
        ] {
            let message = refused(document);
            assert!(message.contains("remote `core`"), "{message}");
            assert!(message.contains(&format!("type `{kind}`")), "{message}");
            assert!(message.contains("step 9.3"), "{message}");
        }
    }

    #[test]
    fn a_type_the_schema_does_not_reserve_is_not_a_remote_at_all() {
        // Unlike the two above, this one fails while the document is read: a
        // remote is selected by its `type`, and there is no record to check.
        let error = parse("type = \"rsync\"\nurl = \"e.example:/a\"\n")
            .expect_err("an unknown type should not deserialize");
        assert!(error.to_string().contains("rsync"), "{error}");
    }

    #[test]
    fn a_git_remote_is_closed_over_the_fields_it_accepts() {
        for (document, unknown) in [
            // The spelling `docs/future/repoformat.md` gives the ref field,
            // which batfiles reads under `git-clone`'s name instead.
            (
                "type = \"git\"\nurl = \"https://e.example/a.git\"\nbranch = \"main\"\n",
                "branch",
            ),
            // Arrives with the dynamic variables it would permit, at 9.1.
            (
                "type = \"git\"\nurl = \"https://e.example/a.git\"\nallow-dynamic-vars = true\n",
                "allow-dynamic-vars",
            ),
            // A field belonging to another variant.
            (
                "type = \"git\"\nurl = \"https://e.example/a.git\"\nsha256 = \"00\"\n",
                "sha256",
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
