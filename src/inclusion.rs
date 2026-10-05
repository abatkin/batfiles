//! Identify inclusions, evaluate admission, and read manifests from existing remote
//! materializations. `execute::inclusion` composes records;
//! [`crate::dynamic::refresh`] selects remotes for variable refresh.

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

/// A leaf `include-remote` declaration's identity, independent of manifest loading.
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
    /// Build an inclusion identity from its declaration and one-based position in the validated
    /// leaf manifest. Unnamed inclusions use their position and remote in diagnostics.
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

    /// Return the first exclusion from record selection or the remote condition, or `None` if
    /// admitted. Perform no I/O or reporting.
    ///
    /// Evaluate `record` using leaf-scope `bindings`; inclusion variables do not affect its own
    /// condition. Apply target exemptions only when the target directly names this record.
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
            selection.exclusion_without_waivers(record, bindings)
        };
        exclusion.or_else(|| remote_exclusion.map(|it| self.restate_remote_exclusion(it)))
    }

    /// A remote's exclusion, restated for this inclusion with the same severity.
    fn restate_remote_exclusion(&self, exclusion: &Exclusion) -> Exclusion {
        let reason = format!(
            "remote `{}` is excluded here: {}",
            self.remote,
            exclusion.reason()
        );
        match exclusion {
            Exclusion::Deliberate(_) => Exclusion::Deliberate(reason),
            Exclusion::EvaluationFailed(_) => Exclusion::EvaluationFailed(reason),
        }
    }

    /// Read and validate the included manifest from `repository`, the anchored leaf root. Call
    /// only after [`exclusion`](Self::exclusion) admits the inclusion.
    ///
    /// Return `None` if the remote materialization is absent. Inspection failures and missing,
    /// unreadable, or invalid manifests are errors. Report nothing; callers may use
    /// [`warn_not_materialized`] for an absent materialization.
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
