//! `[[actions]]`: the ordered list of things a repository does.

use serde::Deserialize;

use super::{
    Invalid, check_archive_root, check_dest, check_digest, check_git_ref, check_git_source,
    check_source, check_url,
};
use crate::clone_list::Entry;
use crate::condition::{Condition, Gate};
use crate::item::ItemId;

/// One entry of `[[actions]]`.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum Action {
    Symlink(SymlinkAction),
    SymlinkDir(SymlinkDirAction),
    CreateDir(CreateDirAction),
    Copy(CopyAction),
    CopyDir(CopyDirAction),
    FetchFile(FetchFileAction),
    FetchArchive(FetchArchiveAction),
    GitClone(GitCloneAction),
    GitCloneList(GitCloneListAction),
}

impl Action {
    /// What every record carries, whichever variant it is.
    pub fn metadata(&self) -> ActionMetadata<'_> {
        let (kind, id, group, when, unless) = match self {
            Self::Symlink(it) => ("symlink", &it.id, &it.group, &it.when, &it.unless),
            Self::SymlinkDir(it) => ("symlink-dir", &it.id, &it.group, &it.when, &it.unless),
            Self::CreateDir(it) => ("create-dir", &it.id, &it.group, &it.when, &it.unless),
            Self::Copy(it) => ("copy", &it.id, &it.group, &it.when, &it.unless),
            Self::CopyDir(it) => ("copy-dir", &it.id, &it.group, &it.when, &it.unless),
            Self::FetchFile(it) => ("fetch-file", &it.id, &it.group, &it.when, &it.unless),
            Self::FetchArchive(it) => ("fetch-archive", &it.id, &it.group, &it.when, &it.unless),
            Self::GitClone(it) => ("git-clone", &it.id, &it.group, &it.when, &it.unless),
            Self::GitCloneList(it) => ("git-clone-list", &it.id, &it.group, &it.when, &it.unless),
        };
        ActionMetadata {
            kind,
            id: id.as_ref(),
            group: group.as_ref(),
            when: when.as_ref(),
            unless: unless.as_ref(),
        }
    }

    /// Validate declared paths, URLs, digests, archive roots, and Git refs.
    /// `number` is the one-based action position used in diagnostics.
    pub fn validate(&self, number: usize) -> Result<(), Invalid> {
        match self {
            Self::Symlink(action) => {
                check_source(&action.source, number)?;
                check_dest(&action.dest, number)
            }
            Self::SymlinkDir(action) => {
                check_source(&action.source_dir, number)?;
                check_dest(&action.dest_dir, number)
            }
            Self::CreateDir(action) => check_dest(&action.dest, number),
            Self::Copy(action) => {
                check_source(&action.source, number)?;
                check_dest(&action.dest, number)
            }
            Self::CopyDir(action) => {
                check_source(&action.source_dir, number)?;
                check_dest(&action.dest_dir, number)
            }
            // The two action types whose `source` names something off this
            // machine, so it answers to neither path rule.
            Self::FetchFile(action) => {
                check_url(&action.source, number)?;
                check_digest(action.sha256.as_deref(), number)?;
                check_dest(&action.dest, number)
            }
            Self::FetchArchive(action) => {
                check_url(&action.source, number)?;
                check_digest(action.sha256.as_deref(), number)?;
                check_archive_root(action.archive_root.as_deref(), number)?;
                check_dest(&action.dest, number)
            }
            // A third kind of source: neither a repository path nor a URL, but
            // whatever `git` accepts as a repository to clone.
            Self::GitClone(action) => {
                check_git_source(&action.source, number)?;
                check_git_ref(action.git_ref.as_deref(), number)?;
                check_dest(&action.dest, number)
            }
            Self::GitCloneList(action) => {
                check_source(&action.source, number)?;
                check_dest(&action.dest_dir, number)
            }
        }
    }

    /// The action's `id`, if it was written with one.
    pub fn id(&self) -> Option<&ItemId> {
        self.metadata().id
    }

    /// The group the action belongs to, if it was written with one.
    pub fn group(&self) -> Option<&ItemId> {
        self.metadata().group
    }

    /// The gate the action's condition makes, if it was written with one.
    pub fn gate(&self) -> Option<Gate<'_>> {
        self.metadata().gate()
    }

    /// How the action introduces itself in a report: what kind it is, what it is
    /// called, and the group it is in.
    pub fn describe(&self, number: usize) -> String {
        let ActionMetadata {
            kind, id, group, ..
        } = self.metadata();
        let name = match id {
            Some(id) => id.to_string(),
            None => format!("action {number}"),
        };
        match group {
            Some(group) => format!("{kind} {name} (group {group})"),
            None => format!("{kind} {name}"),
        }
    }
}

/// The `type` tag and the four fields every `[[actions]]` record carries,
/// borrowed from one.
///
/// This is where those four are explained, for all nine record types: each
/// declares them itself, because each is a flat TOML table that rejects the
/// fields it does not accept, and a struct below comments a shared field only
/// where that record means something particular by it.
///
/// - `id` makes the action addressable: `apply-action` names one, and
///   `disabled.toml` remembers one. What an action installs is not separately
///   addressable, whether it installs one thing or many.
/// - `group` is the one group the action belongs to, which `apply-group` names
///   and `disabled.toml` also remembers.
/// - `when` admits the action where the condition is true, `unless` where it is
///   false. A record writes at most one of the two.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ActionMetadata<'a> {
    /// The action's `type`, spelled as the manifest spells it.
    pub kind: &'static str,
    pub id: Option<&'a ItemId>,
    pub group: Option<&'a ItemId>,
    /// Kept apart from `unless` rather than resolved into a gate, because the
    /// check that a record does not write both is the manifest's and needs to
    /// see both.
    pub when: Option<&'a Condition>,
    pub unless: Option<&'a Condition>,
}

impl<'a> ActionMetadata<'a> {
    /// The gate this record's condition makes, or `None` where it has none.
    pub fn gate(self) -> Option<Gate<'a>> {
        Gate::declared(self.when, self.unless)
    }

    /// Whether the record writes both spellings, which no record may.
    pub fn writes_both_conditions(self) -> bool {
        self.when.is_some() && self.unless.is_some()
    }
}

/// `symlink`: one symlink, from a path in the repository to a destination.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct SymlinkAction {
    pub id: Option<ItemId>,
    pub group: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    /// The source, relative to the repository root. A plain string until 6.3
    /// makes it a path that may also name a remote.
    pub source: String,
    /// The destination path as written, resolved against the selected home when
    /// the action runs.
    pub dest: String,
}

/// `symlink-dir`: one symlink per direct child of a directory in the
/// repository, all of them into one destination directory.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct SymlinkDirAction {
    /// The children are never addressable: the action installs all of them or
    /// none.
    pub id: Option<ItemId>,
    pub group: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    /// The directory whose direct children are linked, relative to the
    /// repository root.
    pub source_dir: String,
    /// The directory the links are made in, resolved against the selected home
    /// when the action runs and created if it is missing.
    pub dest_dir: String,
    /// Whether each link's name gains a leading `.`, for a repository that
    /// keeps its dotfiles undotted.
    #[serde(default)]
    pub dot_prefix: bool,
}

/// `create-dir`: one directory, created where nothing is.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CreateDirAction {
    pub id: Option<ItemId>,
    pub group: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    /// The directory to create, resolved against the selected home when the
    /// action runs. Missing parents are created with it.
    pub dest: String,
}

/// `copy`: one file or one directory, seeded at a destination where nothing is.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CopyAction {
    pub id: Option<ItemId>,
    pub group: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    /// The file or directory to copy, relative to the repository root.
    pub source: String,
    /// Where the copy goes, exactly. Resolved against the selected home when
    /// the action runs.
    pub dest: String,
}

/// `copy-dir`: one copy per direct child of a directory, all of them into one
/// destination directory.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CopyDirAction {
    /// The children are never addressable, individually.
    pub id: Option<ItemId>,
    pub group: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    /// The directory whose direct children are copied, relative to the
    /// repository root.
    pub source_dir: String,
    /// The directory the copies are made in, resolved against the selected home
    /// when the action runs and created if it is missing.
    pub dest_dir: String,
    /// Whether each installed name gains a leading `.`, for a repository that
    /// keeps its dotfiles undotted.
    #[serde(default)]
    pub dot_prefix: bool,
}

/// `fetch-file`: one file downloaded to a destination where nothing is.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct FetchFileAction {
    pub id: Option<ItemId>,
    pub group: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    /// The URL to fetch, as written. Not a `RepoPath` and never resolved
    /// against a root: what it names is not on this machine.
    pub source: String,
    /// Where the file goes, exactly. Resolved against the selected home when
    /// the action runs, with missing parents created on the way.
    pub dest: String,
    /// The digest the fetched bytes must have, if the repository pins one.
    pub sha256: Option<String>,
}

/// `fetch-archive`: one archive downloaded and unpacked at a destination where
/// nothing is.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct FetchArchiveAction {
    /// The archive's entries are never addressable: the action installs the
    /// whole tree or none of it.
    pub id: Option<ItemId>,
    pub group: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    /// The URL to fetch, as written. Not a `RepoPath` and never resolved
    /// against a root: what it names is not on this machine.
    pub source: String,
    /// Where the unpacked directory goes, exactly. Resolved against the
    /// selected home when the action runs, with missing parents created on the
    /// way.
    pub dest: String,
    /// The digest the fetched archive must have, if the repository pins one.
    pub sha256: Option<String>,
    /// A prefix every entry is written without, or `*` for the single
    /// top-level directory a release tarball usually has.
    pub archive_root: Option<String>,
}

/// `git-clone`: one repository cloned where nothing is, and brought up to date
/// where a clone of it already is.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GitCloneAction {
    /// The clone's contents are never addressable, individually.
    pub id: Option<ItemId>,
    pub group: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    /// The repository to clone, exactly as git is given it. Not a `RepoPath`
    /// and not checked as a URL: git accepts an `scp`-style `git@host:path`, a
    /// plain directory, and several schemes, and which of them a source is is
    /// git's question rather than batfiles'.
    pub source: String,
    /// Where the clone goes, exactly. Resolved against the selected home when
    /// the action runs, with missing parents created on the way.
    pub dest: String,
    /// The branch, tag, or commit to follow. Absent, an update follows whatever
    /// branch the clone is on.
    #[serde(rename = "ref")]
    pub git_ref: Option<String>,
}

/// `git-clone-list`: every repository a list in the repository names, cloned
/// under one directory.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GitCloneListAction {
    /// Once entries are individually selectable, this is also the first segment
    /// of `<action>.<entry>`.
    pub id: Option<ItemId>,
    pub group: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    /// The list, relative to the repository root. An ordinary repository path,
    /// unlike the sources of the two actions that reach the network: what is off
    /// this machine is named by the list's lines, not by this field.
    pub source: String,
    /// The directory the clones are made in, resolved against the selected home
    /// when the action runs. Spelled `dest-dir` like the other actions that
    /// install into a directory rather than at a name, because that is what it
    /// is: each entry contributes one child of it.
    pub dest_dir: String,

    /// The list's entries, read during execution preparation and settled
    /// against this run's variables: run state kept on the declaration record,
    /// which is why serde neither reads nor writes it.
    ///
    /// `None` is a list nothing has opened — a record no run has prepared, or
    /// one this run excluded before reading it. `Some([])` is a list that was
    /// read and declares nothing. The two are different answers and must not
    /// collapse into an empty default: one is unknown, the other is known to be
    /// empty, and only the second is a list this run can execute.
    #[serde(skip)]
    pub entries: Option<Vec<Entry>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn described(record: &str, number: usize) -> String {
        let action: Action = toml::from_str(record).expect("the record should parse");
        action.describe(number)
    }

    /// One complete record per variant, so a swapped or misspelled label fails
    /// here. The names are written out rather than read back from the tag serde
    /// matched on, which is the only way this test can disagree with the code it
    /// covers.
    #[test]
    fn every_action_type_is_named_as_the_manifest_spells_it() {
        for (record, kind) in [
            (
                "type = \"symlink\"\nsource = \"a\"\ndest = \"~/b\"\n",
                "symlink",
            ),
            (
                "type = \"symlink-dir\"\nsource-dir = \"a\"\ndest-dir = \"~/b\"\n",
                "symlink-dir",
            ),
            ("type = \"create-dir\"\ndest = \"~/b\"\n", "create-dir"),
            ("type = \"copy\"\nsource = \"a\"\ndest = \"~/b\"\n", "copy"),
            (
                "type = \"copy-dir\"\nsource-dir = \"a\"\ndest-dir = \"~/b\"\n",
                "copy-dir",
            ),
            (
                "type = \"fetch-file\"\nsource = \"https://e.example/a\"\ndest = \"~/b\"\n",
                "fetch-file",
            ),
            (
                "type = \"fetch-archive\"\nsource = \"https://e.example/a.tar.gz\"\ndest = \"~/b\"\n",
                "fetch-archive",
            ),
            (
                "type = \"git-clone\"\nsource = \"https://e.example/a.git\"\ndest = \"~/b\"\n",
                "git-clone",
            ),
            (
                "type = \"git-clone-list\"\nsource = \"list.txt\"\ndest-dir = \"~/b\"\n",
                "git-clone-list",
            ),
        ] {
            assert_eq!(described(record, 1), format!("{kind} action 1"));
        }
    }

    #[test]
    fn a_record_with_an_id_is_named_by_it() {
        assert_eq!(
            described(
                "type = \"copy\"\nid = \"gitconfig\"\nsource = \"a\"\ndest = \"~/b\"\n",
                3
            ),
            "copy gitconfig"
        );
    }

    #[test]
    fn a_record_without_an_id_is_named_by_its_position() {
        // The same words a load error uses for a record with no `id`, so a line
        // of output and a diagnostic point at the same one.
        assert_eq!(
            described("type = \"copy\"\nsource = \"a\"\ndest = \"~/b\"\n", 3),
            "copy action 3"
        );
    }

    #[test]
    fn a_grouped_record_says_which_group() {
        assert_eq!(
            described(
                "type = \"create-dir\"\nid = \"cache\"\ngroup = \"shell\"\ndest = \"~/b\"\n",
                1
            ),
            "create-dir cache (group shell)"
        );
        assert_eq!(
            described(
                "type = \"create-dir\"\ngroup = \"shell\"\ndest = \"~/b\"\n",
                2
            ),
            "create-dir action 2 (group shell)"
        );
    }
}
