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
}

/// `symlink`: one symlink, from a path in the repository to a destination.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct SymlinkAction {
    /// Makes the action addressable.
    pub id: Option<ItemId>,
    /// The one group the action belongs to.
    #[expect(dead_code, reason = "3.2 selects by group")]
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
    #[expect(dead_code, reason = "3.2 selects by group")]
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
    #[expect(dead_code, reason = "3.2 selects by group")]
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
    #[expect(dead_code, reason = "3.2 selects by group")]
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
    #[expect(dead_code, reason = "3.2 selects by group")]
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
