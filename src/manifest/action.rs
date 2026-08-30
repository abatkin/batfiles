//! `[[actions]]`: the ordered list of things a repository does.
//!
//! Each action is one closed record selected by its `type` tag. Variants repeat
//! the common fields rather than sharing a flattened record, because
//! `#[serde(flatten)]` silently disables `deny_unknown_fields`, and a closed
//! record is what the format promises.

use serde::Deserialize;

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
}

impl Action {
    /// The action's `id`, if it was written with one.
    ///
    /// IDs share one namespace across a repository, so uniqueness is checked
    /// over the list as a whole — by a caller that does not know, and should
    /// not have to ask, which variant it is holding.
    pub fn id(&self) -> Option<&ItemId> {
        match self {
            Self::Symlink(action) => action.id.as_ref(),
            Self::SymlinkDir(action) => action.id.as_ref(),
            Self::CreateDir(action) => action.id.as_ref(),
            Self::Copy(action) => action.id.as_ref(),
            Self::CopyDir(action) => action.id.as_ref(),
        }
    }

    /// The group the action belongs to, if it was written with one.
    ///
    /// A group is nothing but this field: no section declares one, so a group
    /// exists because some action names it and holds exactly the actions that
    /// do.
    pub fn group(&self) -> Option<&ItemId> {
        match self {
            Self::Symlink(action) => action.group.as_ref(),
            Self::SymlinkDir(action) => action.group.as_ref(),
            Self::CreateDir(action) => action.group.as_ref(),
            Self::Copy(action) => action.group.as_ref(),
            Self::CopyDir(action) => action.group.as_ref(),
        }
    }

    /// How the action introduces itself in a report: what kind it is, what it is
    /// called, and the group it is in.
    ///
    /// `number` is its one-based position in the list, which is what names a
    /// record carrying no `id` — the same way a load error names one.
    pub fn describe(&self, number: usize) -> String {
        let kind = self.kind();
        let name = match self.id() {
            Some(id) => id.to_string(),
            None => format!("action {number}"),
        };
        match self.group() {
            Some(group) => format!("{kind} {name} (group {group})"),
            None => format!("{kind} {name}"),
        }
    }

    /// The action's `type`, spelled as the manifest spells it.
    ///
    /// No wildcard, so a variant added later fails to compile until someone says
    /// what it is called.
    fn kind(&self) -> &'static str {
        match self {
            Self::Symlink(_) => "symlink",
            Self::SymlinkDir(_) => "symlink-dir",
            Self::CreateDir(_) => "create-dir",
            Self::Copy(_) => "copy",
            Self::CopyDir(_) => "copy-dir",
        }
    }
}

/// `symlink`: one symlink, from a path in the repository to a destination.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct SymlinkAction {
    /// Makes the action addressable.
    pub id: Option<ItemId>,
    /// The one group the action belongs to.
    pub group: Option<ItemId>,
    /// The source, relative to the repository root. A plain string until 6.3
    /// makes it a path that may also name a remote.
    pub source: String,
    /// The destination path as written, resolved against the selected home when
    /// the action runs.
    pub dest: String,
}

/// `symlink-dir`: one symlink per direct child of a directory in the
/// repository, all of them into one destination directory.
///
/// A separate type rather than a second mode of `symlink`, so that serde
/// decides which fields a record must carry and there is no invariant spanning
/// two optional halves. Filtering the children — `include` and `exclude` — is
/// specified in `docs/future/repoformat.md` and not built; adding it later is
/// additive, and neither repository this exists for needs it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct SymlinkDirAction {
    /// Makes the action addressable. The children never are, individually: the
    /// action installs all of them or none.
    pub id: Option<ItemId>,
    /// The one group the action belongs to.
    pub group: Option<ItemId>,
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
///
/// The only action with no source. It exists for a directory whose contents
/// come from somewhere else — a plugin root another tool clones into, a cache a
/// program expects to find — where the manifest has nothing of its own to put
/// there.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CreateDirAction {
    /// Makes the action addressable.
    pub id: Option<ItemId>,
    /// The one group the action belongs to.
    pub group: Option<ItemId>,
    /// The directory to create, resolved against the selected home when the
    /// action runs. Missing parents are created with it.
    pub dest: String,
}

/// `copy`: one file or one directory, seeded at a destination where nothing is.
///
/// The copy is the user's from then on. That is what separates this from
/// `symlink` — the same pair of fields, installing the same thing at the same
/// place, but a detached one that editing does not write back into the
/// repository, and that a later `sync` will not undo.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CopyAction {
    /// Makes the action addressable.
    pub id: Option<ItemId>,
    /// The one group the action belongs to.
    pub group: Option<ItemId>,
    /// The file or directory to copy, relative to the repository root.
    pub source: String,
    /// Where the copy goes, exactly. Resolved against the selected home when
    /// the action runs.
    pub dest: String,
}

/// `copy-dir`: one copy per direct child of a directory, all of them into one
/// destination directory.
///
/// Stands to [`CopyAction`] as [`SymlinkDirAction`] does to [`SymlinkAction`]:
/// the same installation, done once per child rather than once. There is no
/// `dot-prefix` on `CopyAction` for the same reason there is none on
/// `SymlinkAction` — a destination written out in full already says what it is
/// called.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CopyDirAction {
    /// Makes the action addressable. The children never are, individually.
    pub id: Option<ItemId>,
    /// The one group the action belongs to.
    pub group: Option<ItemId>,
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
