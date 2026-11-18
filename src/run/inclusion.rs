//! Identify inclusions, evaluate admission, and read manifests from existing remote
//! materializations. [`crate::run::compose`] composes records;
//! [`crate::commands::vars::refresh`] selects remotes for variable refresh.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::Error;
use crate::filesystem::paths;
use crate::manifest::Manifest;
use crate::manifest::actions::{Contributor, IncludeRemoteAction};
use crate::manifest::check::ValueRule;
use crate::manifest::condition::{Bindings, Exclusion};
use crate::manifest::item::ItemId;
use crate::manifest::remote::Remote;
use crate::manifest::template::{EnvAccess, Resolver};
use crate::output::Reporter;
use crate::run::selection::{Selection, Subject};
use crate::source::git::{self, Origin};
use crate::source::remotes;

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
    /// If the leaf's declaration in `remotes` grants the remote a permission, first
    /// [verify](Self::verify_checkout) the checkout's origin against the `url` it resolves with
    /// the leaf's `bindings`.
    ///
    /// Return `None` if the remote materialization is absent. Inspection failures, an
    /// unverified checkout, and missing, unreadable, or invalid manifests are errors. Report
    /// nothing; callers may use [`warn_not_materialized`] for an absent materialization.
    pub fn manifest(
        &self,
        repository: &Path,
        remotes: &BTreeMap<ItemId, Remote>,
        bindings: &Bindings<'_>,
    ) -> Result<Option<Manifest>, Error> {
        let tree = remotes::materialization(repository, &self.remote);
        if !paths::occupied(&tree)? {
            return Ok(None);
        }
        if let Some(declared) = remotes.get(&self.remote) {
            self.verify_checkout(declared, &tree, bindings)?;
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

impl Inclusion {
    /// Refuse `checkout`, a Git remote's materialization, if `declared` grants the remote a
    /// permission and the checkout's recorded origin is not the repository `declared`'s `url`
    /// names. A missing, ambiguous, or unreadable origin is refused too. Either permission
    /// triggers the check; a remote granted neither, or not a Git remote, is not checked.
    fn verify_checkout(
        &self,
        declared: &Remote,
        checkout: &Path,
        bindings: &Bindings<'_>,
    ) -> Result<(), Error> {
        let Remote::Git(git) = declared else {
            return Ok(());
        };
        let permissions = match (git.allow_dynamic_vars, git.allow_env_templates) {
            (false, false) => return Ok(()),
            (true, false) => "allow-dynamic-vars",
            (false, true) => "allow-env-templates",
            (true, true) => "allow-dynamic-vars and allow-env-templates",
        };
        let heading = format!("remote {}", self.remote);
        let url = Resolver::new(&heading, bindings, EnvAccess::Allowed).field(
            "url",
            &git.url,
            ValueRule::GitSource,
        )?;
        let reason = match git::recorded_origin(checkout)? {
            Origin::Url(recorded) => {
                let here =
                    std::env::current_dir().map_err(|source| Error::WorkingDirectory { source })?;
                if git::same_origin(&url, &recorded, &here) {
                    return Ok(());
                }
                "its origin is a different repository"
            }
            Origin::Missing => "it has no origin",
            Origin::Ambiguous => "its origin has more than one URL",
            Origin::NotItsOwn => "it has no .git directory of its own",
            Origin::Unreadable => "git cannot read its origin",
        };
        Err(Error::CheckoutUnverified {
            inclusion: self.label.clone(),
            remote: self.remote.clone(),
            path: checkout.to_path_buf(),
            reason,
            permissions,
        })
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
