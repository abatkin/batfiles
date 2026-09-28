//! `include-remote`, as far as every command that meets one needs it: which
//! inclusion it is, whether this machine opens it, and the manifest it reads
//! from this machine's materialization of its remote, however stale.
//!
//! What an opened inclusion contributes to a run is
//! [`execute::inclusion`](crate::execute::inclusion)'s; which remotes are in
//! play for `vars refresh` is [`dynamic::refresh`](crate::dynamic::refresh)'s.

use std::path::Path;

use crate::condition::{Bindings, Exclusion};
use crate::error::Error;
use crate::item::ItemId;
use crate::manifest::Manifest;
use crate::manifest::action::{Contributor, IncludeRemoteAction};
use crate::output::Reporter;
use crate::paths;
use crate::remotes;
use crate::selection::{Selection, Subject};

/// One `include-remote` of the leaf manifest: its identity, whether or not its
/// manifest is read.
pub(crate) struct Inclusion {
    /// The written `id`, which qualifies its records' addresses. `None`: its
    /// records have no address, and [`label`](Self::label) names it.
    id: Option<ItemId>,
    /// The included remote, whose materialization holds its manifest and its
    /// records' repository paths.
    remote: ItemId,
    label: String,
}

impl Inclusion {
    /// Identify the inclusion the record at one-based position `number` writes.
    ///
    /// Reports name it by `id`, or by its position and remote if unnamed.
    /// `number` must be its position in the validated leaf manifest.
    pub fn at(action: &IncludeRemoteAction, number: usize) -> Self {
        let label = match &action.id {
            Some(id) => format!("include-remote `{id}`"),
            None => format!(
                "include-remote action {number} of remote `{}`",
                action.remote
            ),
        };
        Self {
            id: action.id.clone(),
            remote: action.remote.clone(),
            label,
        }
    }

    /// The `id` this inclusion answers to, where it was written with one.
    pub fn id(&self) -> Option<&ItemId> {
        self.id.as_ref()
    }

    /// The remote it includes.
    pub fn remote(&self) -> &ItemId {
        &self.remote
    }

    /// How a report names it.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// How lines name its records: by `id`, or by label when it has none.
    pub fn contributor(&self) -> Contributor<'_> {
        match &self.id {
            Some(id) => Contributor::Inclusion(id),
            None => Contributor::UnnamedInclusion(&self.label),
        }
    }

    /// Why this machine would not open this inclusion, or `None` if it would.
    /// Reads nothing and reports nothing.
    ///
    /// `record` is the inclusion's leaf record as selection reads it. The first
    /// exclusion `selection` finds for it, decided in the leaf scope
    /// `bindings`: an inclusion's `vars` apply only to the records it
    /// contributes, not to its own condition. What the target waives is waived
    /// only where it names the record; a target reaching into the inclusion
    /// without naming it waives nothing. Failing that, `remote_exclusion`, the
    /// remote's condition as the command decided it, restated for this
    /// inclusion.
    pub fn exclusion(
        &self,
        record: Subject<'_>,
        selection: &Selection<'_>,
        bindings: &Bindings<'_>,
        remote_exclusion: Option<&Exclusion>,
    ) -> Option<Exclusion> {
        let exclusion = if selection.wants(record) {
            selection.exclusion(record, bindings)
        } else {
            selection.unwaived_exclusion(record, bindings)
        };
        exclusion.or_else(|| remote_exclusion.map(|it| self.closed_by_remote(it)))
    }

    /// A remote's exclusion, restated for this inclusion with the same severity.
    fn closed_by_remote(&self, exclusion: &Exclusion) -> Exclusion {
        let reason = format!(
            "remote `{}` is excluded here: {}",
            self.remote,
            exclusion.reason()
        );
        match exclusion {
            Exclusion::Expected(_) => Exclusion::Expected(reason),
            Exclusion::EvaluationFailed(_) => Exclusion::EvaluationFailed(reason),
        }
    }

    /// The included remote's manifest, validated whole, read from its
    /// materialization in the leaf repository at the anchored path
    /// `repository`; `None` where there is none. Reports nothing: a caller that
    /// passes over an absent one warns with [`warn_not_materialized`].
    ///
    /// Fails when the materialization cannot be inspected or its manifest is
    /// missing, unreadable, or invalid. A materialized tree without a manifest
    /// is an error: a remote's manifest is optional, so it is absent rather than
    /// not yet fetched.
    ///
    /// Only an inclusion [`exclusion`](Self::exclusion) admitted may be read:
    /// an excluded remote's tree is not read, whatever an earlier run left.
    pub fn manifest(&self, repository: &Path) -> Result<Option<Manifest>, Error> {
        let tree = remotes::materialization(repository, &self.remote);
        if !paths::occupied(&tree)? {
            return Ok(None);
        }
        let manifest = tree.join(Manifest::FILE_NAME);
        if !paths::occupied(&manifest)? {
            return Err(Error::IncludedManifestMissing {
                remote: self.remote.clone(),
                path: manifest,
            });
        }
        Manifest::load_included(&manifest).map(Some)
    }
}

/// Warn that `remote` has no materialization at `path`, so nothing it includes
/// can be read.
pub(crate) fn warn_not_materialized(remote: &ItemId, path: &Path, reporter: &Reporter) {
    reporter.warn(&format!(
        "remote `{remote}` is not materialized at {}, so what it includes cannot be \
         listed; run `batfiles sync` to bring it down",
        path.display()
    ));
}

/// Why an inclusion's manifest was not read.
pub(crate) enum Unread<'a> {
    /// The command did not request it.
    NotRequested,
    /// Requested, and excluded for this reason.
    Excluded(&'a Exclusion),
    /// Admitted, with no materialization of this remote to read.
    NotMaterialized(&'a ItemId),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inclusion_is_labelled_by_its_id_or_by_where_it_was_written() {
        let labelled = |record: &str, number| {
            let action: IncludeRemoteAction =
                toml::from_str(record).expect("the record should parse");
            Inclusion::at(&action, number).label().to_owned()
        };
        assert_eq!(
            labelled("id = \"corp\"\nremote = \"corporate\"\n", 3),
            "include-remote `corp`"
        );
        // Two inclusions of one remote, neither written with an `id`: the
        // position is the whole of what tells the labels apart.
        let unnamed = "remote = \"corporate\"\n";
        assert_eq!(
            labelled(unnamed, 2),
            "include-remote action 2 of remote `corporate`"
        );
        assert_eq!(
            labelled(unnamed, 5),
            "include-remote action 5 of remote `corporate`"
        );
    }
}
