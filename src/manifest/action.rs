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
}

impl Action {
    /// The action's `id`, if it was written with one.
    ///
    /// IDs share one namespace across a repository, so uniqueness is checked
    /// over the list as a whole — by a caller that does not know, and should
    /// not have to ask, which variant it is holding. More such accessors are
    /// coming: `group` at 3.2 and the conditions at 5.6. At the third, collapse
    /// them into one `fn common(&self) -> Common<'_>` returning a borrowed view
    /// of the shared fields, so there is one exhaustive match rather than one
    /// per field.
    pub fn id(&self) -> Option<&ItemId> {
        match self {
            Self::Symlink(action) => action.id.as_ref(),
        }
    }
}

/// `symlink`: one symlink, from a path in the repository to a destination.
///
/// Directory mode — `source-dir`, `dest-dir`, and the filters — is not here.
/// It needs a glob filter to mean anything, so a manifest that writes it is
/// rejected rather than silently linking nothing.
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
