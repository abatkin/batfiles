//! `[[actions]]`: the ordered list of things a repository does.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::ReadAs;
use super::check::{
    Invalid, RecordName, check_archive_root, check_dest, check_digest, check_git_ref,
    check_git_source, check_inclusion_filters, check_inclusion_remote, check_source, check_url,
};
use super::remote::Remote;
use crate::clone_list::Entry;
use crate::condition::{Condition, Gate};
use crate::item::{ItemAddress, ItemId, ItemIdList};
use crate::repo_path::RepoPath;
use crate::var::VarName;

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
    IncludeRemote(IncludeRemoteAction),
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
            Self::IncludeRemote(it) => ("include-remote", &it.id, &it.group, &it.when, &it.unless),
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
    /// `record` is how a diagnostic names the record, which for an action is
    /// its one-based position.
    ///
    /// `remotes` is what the manifest declares, for the rules a record cannot
    /// settle alone: a source may name a remote, and an `include-remote` names
    /// one outright, and either way the remote named has to be one of these.
    /// `read_as` says whose manifest this is, for the second of those: an
    /// inclusion an included manifest declares is dropped rather than followed,
    /// so what it names is not required to resolve.
    pub fn validate(
        &self,
        record: &RecordName,
        remotes: &BTreeMap<ItemId, Remote>,
        read_as: ReadAs,
    ) -> Result<(), Invalid> {
        match self {
            Self::Symlink(action) => {
                check_source(&action.source, record, remotes)?;
                check_dest(&action.dest, record)
            }
            Self::SymlinkDir(action) => {
                check_source(&action.source_dir, record, remotes)?;
                check_dest(&action.dest_dir, record)
            }
            Self::CreateDir(action) => check_dest(&action.dest, record),
            Self::Copy(action) => {
                check_source(&action.source, record, remotes)?;
                check_dest(&action.dest, record)
            }
            Self::CopyDir(action) => {
                check_source(&action.source_dir, record, remotes)?;
                check_dest(&action.dest_dir, record)
            }
            // The two action types whose `source` names something off this
            // machine, so it answers to neither path rule.
            Self::FetchFile(action) => {
                check_url(&action.source, record)?;
                check_digest(action.sha256.as_deref(), record)?;
                check_dest(&action.dest, record)
            }
            Self::FetchArchive(action) => {
                check_url(&action.source, record)?;
                check_digest(action.sha256.as_deref(), record)?;
                check_archive_root(action.archive_root.as_deref(), record)?;
                check_dest(&action.dest, record)
            }
            // A third kind of source: neither a repository path nor a URL, but
            // whatever `git` accepts as a repository to clone. A Git remote
            // spells the same thing `url` and shares both checks.
            Self::GitClone(action) => {
                check_git_source(&action.source, "source", record)?;
                check_git_ref(action.git_ref.as_deref(), record)?;
                check_dest(&action.dest, record)
            }
            Self::GitCloneList(action) => {
                check_source(&action.source, record, remotes)?;
                check_dest(&action.dest_dir, record)
            }
            // The one record with neither a source nor a destination: what it
            // installs is whatever the included manifest says, wherever that
            // says to put it. Only a leaf's is required to name a remote that
            // resolves: one in an included manifest is left out of the run with
            // a warning, so refusing the document over the name it wrote would
            // fail the leaf's run over a record nothing was going to honor.
            Self::IncludeRemote(action) => {
                if let ReadAs::Leaf = read_as {
                    check_inclusion_remote(&action.remote, record, remotes)?;
                }
                check_inclusion_filters(action, record)
            }
        }
    }

    /// The repository path this record installs from, where it has one.
    ///
    /// The two fetching actions and `git-clone` name something off this machine
    /// instead, and an `include-remote` names no path at all. No record has
    /// more than one, so this is a value rather than a list.
    pub fn source(&self) -> Option<&RepoPath> {
        match self {
            Self::Symlink(action) => Some(&action.source),
            Self::SymlinkDir(action) => Some(&action.source_dir),
            Self::Copy(action) => Some(&action.source),
            Self::CopyDir(action) => Some(&action.source_dir),
            Self::GitCloneList(action) => Some(&action.source),
            Self::CreateDir(_)
            | Self::FetchFile(_)
            | Self::FetchArchive(_)
            | Self::GitClone(_)
            | Self::IncludeRemote(_) => None,
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
    ///
    /// `qualifier` is the `id` of the inclusion that contributed the record, so
    /// that what a line calls it is the address a reader would type. A record an
    /// inclusion written without an `id` contributed is named as the manifest
    /// that declared it names it, since no address reaches it.
    // CARRY(7.7): every inclusion gains a display label, which is what tells two
    // inclusions of one remote apart in a line like that.
    pub fn describe(&self, number: usize, qualifier: Option<&ItemId>) -> String {
        let ActionMetadata {
            kind, id, group, ..
        } = self.metadata();
        // A record with no `id` is named by its position, which is a position in
        // the manifest that declared it rather than an address, so it is not
        // qualified.
        let name = match id {
            Some(id) => ItemAddress::qualified(qualifier, id).to_string(),
            None => format!("action {number}"),
        };
        match group {
            Some(group) => format!(
                "{kind} {name} (group {})",
                ItemAddress::qualified(qualifier, group)
            ),
            None => format!("{kind} {name}"),
        }
    }
}

/// The `type` tag and the four fields every `[[actions]]` record carries,
/// borrowed from one.
///
/// This is where those four are explained, for all ten record types: each
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
    /// The source, within the repository that declared the action or within a
    /// remote it names.
    pub source: RepoPath,
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
    /// The directory whose direct children are linked, within the repository
    /// that declared the action or within a remote it names.
    pub source_dir: RepoPath,
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
    /// The file or directory to copy, within the repository that declared the
    /// action or within a remote it names.
    pub source: RepoPath,
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
    /// The directory whose direct children are copied, within the repository
    /// that declared the action or within a remote it names.
    pub source_dir: RepoPath,
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
    /// The list, within the repository that declared the action or within a
    /// remote it names. An ordinary repository path, unlike the sources of the
    /// two actions that reach the network: what is off this machine is named by
    /// the list's lines, not by this field.
    pub source: RepoPath,
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

/// `include-remote`: the actions another repository declares, taken into this
/// one at this position in the list.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct IncludeRemoteAction {
    /// What makes the included actions, groups, and entries addressable, by
    /// standing as the first segment of a qualified `<inclusion>.<action>`. It
    /// is this inclusion's name and need not match `remote`, since one remote
    /// may be included more than once.
    pub id: Option<ItemId>,
    pub group: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    /// The declared remote whose manifest is read, named by the key it was
    /// declared under. Required: an inclusion that names no remote includes
    /// nothing.
    pub remote: ItemId,

    /// Which of the remote's actions this inclusion takes, named as the
    /// manifest that declared them names them: unqualified, since the
    /// qualifier is this record's own `id`.
    ///
    /// Each is absent, one ID, or a list of IDs, and absent is not the same as
    /// empty: with none of the four written every action in the remote is
    /// selected, while an empty allow-list selects none. Which combinations
    /// mean something is [`check_inclusion_filters`]'s rule; what each one
    /// selects is [`Filter`](crate::action::include_remote::Filter)'s.
    pub install_actions: Option<ItemIdList>,
    pub install_groups: Option<ItemIdList>,
    pub exclude_actions: Option<ItemIdList>,
    pub exclude_groups: Option<ItemIdList>,

    /// Variable values for what this inclusion contributes, and for nothing
    /// else: the leaf saying what the remote's conditions should decide against
    /// on this machine.
    ///
    /// Read exactly as a manifest's own [`[vars]`](crate::manifest::Manifest::vars)
    /// is, so a name that breaks [`VarName`]'s rule and a value that is not a
    /// string each fail the document at the line they are written on. Where they
    /// sit among the layers is
    /// [`docs/environment.md`](../../docs/environment.md#variable-precedence)'s
    /// rule and [`VarSet::with_inclusion`](crate::var_set::VarSet::with_inclusion)'s
    /// job; an empty map is a record that overrides nothing.
    #[serde(default)]
    pub vars: BTreeMap<VarName, String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn described(record: &str, number: usize) -> String {
        let action: Action = toml::from_str(record).expect("the record should parse");
        action.describe(number, None)
    }

    /// The same record as an inclusion contributed it, named under that
    /// inclusion's `id`.
    fn described_under(record: &str, number: usize, inclusion: &str) -> String {
        let action: Action = toml::from_str(record).expect("the record should parse");
        let inclusion = ItemId::try_from(inclusion.to_owned()).expect("valid ID");
        action.describe(number, Some(&inclusion))
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
            (
                "type = \"include-remote\"\nremote = \"core\"\n",
                "include-remote",
            ),
        ] {
            assert_eq!(described(record, 1), format!("{kind} action 1"));
        }
    }

    #[test]
    fn an_inclusions_overrides_are_read_as_names_and_string_values() {
        let Action::IncludeRemote(inclusion) =
            parse_inclusion("vars = { profile = \"work\", editor = \"vi\" }\n")
        else {
            panic!("the record should be an inclusion");
        };
        assert_eq!(
            inclusion.vars,
            BTreeMap::from([
                (var("editor"), "vi".to_owned()),
                (var("profile"), "work".to_owned()),
            ])
        );
    }

    #[test]
    fn an_inclusion_writing_no_overrides_overrides_nothing() {
        // The default a record without the field gets, which is also what an
        // empty table says: the inclusion contributes actions and leaves the
        // run's variables as they are.
        for record in ["", "vars = {}\n"] {
            let Action::IncludeRemote(inclusion) = parse_inclusion(record) else {
                panic!("the record should be an inclusion");
            };
            assert!(inclusion.vars.is_empty(), "`{record}` declared a variable");
        }
    }

    #[test]
    fn an_override_follows_the_rules_a_manifests_own_vars_follow() {
        // Both settled by serde as the document is read, which is why nothing
        // re-checks either: a name the variable rule refuses, and a value that
        // is not a string.
        for (document, expected) in [
            (
                "vars = { \"has space\" = \"1\" }\n",
                "must start with a letter or underscore",
            ),
            ("vars = { facts = \"1\" }\n", "reserved"),
            ("vars = { rank = 9 }\n", "expected a string"),
        ] {
            let record = format!("type = \"include-remote\"\nremote = \"core\"\n{document}");
            let error =
                toml::from_str::<Action>(&record).expect_err("the record should be refused");
            assert!(error.to_string().contains(expected), "{error}");
        }
    }

    #[test]
    fn an_inclusions_filters_are_read_in_either_spelling() {
        let Action::IncludeRemote(inclusion) = parse_inclusion(
            "install-groups = \"shell\"\nexclude-actions = [\"p10k\", \"seeds\"]\n",
        ) else {
            panic!("the record should be an inclusion");
        };
        assert_eq!(
            inclusion.install_groups,
            Some(ItemIdList::from_iter([item("shell")]))
        );
        assert_eq!(
            inclusion.exclude_actions,
            Some(ItemIdList::from_iter([item("p10k"), item("seeds")]))
        );
        // The two that were not written, which is a different answer from an
        // empty list: these select everything rather than nothing.
        assert_eq!(inclusion.install_actions, None);
        assert_eq!(inclusion.exclude_groups, None);
    }

    fn parse_inclusion(filters: &str) -> Action {
        toml::from_str(&format!(
            "type = \"include-remote\"\nid = \"corp\"\nremote = \"core\"\n{filters}"
        ))
        .expect("the record should parse")
    }

    fn item(id: &str) -> ItemId {
        ItemId::try_from(id.to_owned()).expect("valid ID")
    }

    fn var(name: &str) -> VarName {
        VarName::try_from(name.to_owned()).expect("valid name")
    }

    #[test]
    fn an_inclusion_names_the_remote_it_includes() {
        // The one required field. Without it the record selects nothing, which
        // is not an inclusion that means anything.
        let error = toml::from_str::<Action>("type = \"include-remote\"\n")
            .expect_err("a record naming no remote should be refused");
        assert!(
            error.to_string().contains("missing field `remote`"),
            "{error}"
        );
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
    fn an_included_record_is_named_by_the_address_that_reaches_it() {
        // What a line calls a record is what a reader would type to name it, so
        // an inclusion's qualifier goes on both of a record's IDs.
        assert_eq!(
            described_under(
                "type = \"symlink\"\nid = \"zshrc\"\ngroup = \"shell\"\nsource = \"a\"\ndest = \"~/b\"\n",
                1,
                "corp"
            ),
            "symlink corp.zshrc (group corp.shell)"
        );
        // A record with no `id` is named by its position in the manifest that
        // declared it, which is not an address and so is not qualified.
        assert_eq!(
            described_under("type = \"create-dir\"\ndest = \"~/b\"\n", 2, "corp"),
            "create-dir action 2"
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
