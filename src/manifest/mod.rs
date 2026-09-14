//! `batfiles.toml`: the one file in a repository with intrinsic meaning.

pub(crate) mod action;
pub(crate) mod check;
pub(crate) mod default_disabled;
pub(crate) mod remote;

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::Error as CrateError;
use crate::item::ItemId;
use crate::manifest::action::Action;
use crate::manifest::check::RecordName;
use crate::manifest::default_disabled::DefaultDisabled;
use crate::manifest::remote::Remote;
use crate::tomlfile;
use crate::var::VarName;

/// Re-exported so that a manifest's load error is `manifest::Invalid` from
/// outside, as [`clone_list`](crate::clone_list::Invalid)'s and
/// [`archive`](crate::archive::Invalid)'s are: which module within `manifest`
/// holds it is not something a caller needs to track.
pub(crate) use crate::manifest::check::Invalid;

/// A parsed `batfiles.toml`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct Manifest {
    /// The repositories this one names, keyed by the ID an action reaches them
    /// by. Unordered, because a remote is looked up rather than run: what a
    /// machine does with one is decided by the action that names it.
    #[serde(default)]
    pub remotes: BTreeMap<ItemId, Remote>,

    /// The ordered action list. Order is significant, so this is the one
    /// section that is a sequence rather than a map.
    #[serde(default)]
    pub actions: Vec<Action>,

    /// Static variable values, keyed by name. Every value is a string: serde
    /// settles both rules as the document is read, so a name that breaks the
    /// rule and a value that is not a string each fail the document at the line
    /// they are written on, and nothing here re-checks either.
    ///
    /// This is the lowest layer of the [variable set](crate::var_set) a run
    /// resolves.
    #[serde(default)]
    pub vars: BTreeMap<VarName, String>,

    /// What a fresh machine starts with switched off. Accepted and checked as
    /// the document is read, including the conditions its entries carry;
    /// adopting the candidates belongs to the bootstrap.
    #[serde(default)]
    pub default_disabled: DefaultDisabled,
}

impl Manifest {
    /// The manifest's name within a repository root.
    pub const FILE_NAME: &'static str = "batfiles.toml";

    /// Read, parse, and check one manifest.
    pub fn load(path: &Path) -> Result<Self, CrateError> {
        let manifest: Self = tomlfile::read(path)?;
        manifest
            .validate()
            .map_err(|source| CrateError::InvalidManifest {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(manifest)
    }

    /// The rules serde cannot express, checked as the document is read.
    ///
    /// The ones spanning more than one record are here, since nothing smaller
    /// than the document can see them; each record's own rules are its to
    /// apply, over the [checks](check) they share.
    fn validate(&self) -> Result<(), Invalid> {
        // Ahead of the actions, since an action reaching a remote's content is
        // reaching one of these.
        for (id, remote) in &self.remotes {
            remote.validate(id)?;
        }

        let mut seen: BTreeMap<&ItemId, usize> = BTreeMap::new();
        for (index, action) in self.actions.iter().enumerate() {
            // One-based, because the diagnostic is read against a file whose
            // first action is action 1.
            let action_number = index + 1;
            let record = RecordName::Action(action_number);

            if let Some(id) = action.id()
                && let Some(first) = seen.insert(id, action_number)
            {
                return Err(Invalid::DuplicateActionId {
                    id: id.clone(),
                    first,
                    second: action_number,
                });
            }

            if action.metadata().writes_both_conditions() {
                return Err(Invalid::BothConditions { record });
            }

            // Which of a record's fields are paths, and which rule each one
            // follows, is the record's own answer.
            action.validate(&record)?;
        }
        self.default_disabled.validate()
    }
}
