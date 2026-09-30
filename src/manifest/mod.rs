//! Parse and validate `batfiles.toml` manifests.

pub(crate) mod action;
pub(crate) mod check;
pub(crate) mod default_disabled;
pub(crate) mod duration;
pub(crate) mod remote;
pub(crate) mod vars;

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::Error as CrateError;
use crate::item::ItemId;
use crate::manifest::action::Action;
use crate::manifest::check::{RecordName, check_included_source};
use crate::manifest::default_disabled::DefaultDisabled;
use crate::manifest::remote::Remote;
use crate::manifest::vars::VarSpec;
use crate::tomlfile;
use crate::var::VarName;

pub(crate) use crate::manifest::check::ManifestError;

/// Manifest validation context. Included manifests cannot source from remotes; their nested
/// inclusions need not reference declared remotes, and their `[remotes]` declarations are
/// ignored.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ReadAs {
    /// The repository batfiles was pointed at.
    Leaf,
    /// The manifest inside a remote's materialization, read because a leaf
    /// `include-remote` selected it.
    Included,
}

/// A parsed `batfiles.toml`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct Manifest {
    /// Remote declarations keyed by ID. Sync materializes admitted remotes in ID order,
    /// including unreferenced ones.
    #[serde(default)]
    pub remotes: BTreeMap<ItemId, Remote>,

    /// Actions in execution order.
    #[serde(default)]
    pub actions: Vec<Action>,

    /// Validated static values and dynamic declarations, keyed by variable name.
    #[serde(default)]
    pub vars: BTreeMap<VarName, VarSpec>,

    /// Validated action and group candidates for bootstrap disabled state.
    #[serde(default)]
    pub default_disabled: DefaultDisabled,
}

impl Manifest {
    /// The manifest's name within a repository root.
    pub const FILE_NAME: &'static str = "batfiles.toml";

    /// Read, parse, and check the leaf repository's own manifest.
    pub fn load(path: &Path) -> Result<Self, CrateError> {
        Self::load_as(path, ReadAs::Leaf)
    }

    /// Read and validate a remote manifest using [`ReadAs::Included`] rules.
    pub fn load_included(path: &Path) -> Result<Self, CrateError> {
        Self::load_as(path, ReadAs::Included)
    }

    fn load_as(path: &Path, read_as: ReadAs) -> Result<Self, CrateError> {
        let manifest: Self = tomlfile::read(path)?;
        manifest
            .validate(read_as)
            .map_err(|source| CrateError::InvalidManifest {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(manifest)
    }

    /// Apply per-record and cross-record validation for the manifest context.
    fn validate(&self, read_as: ReadAs) -> Result<(), ManifestError> {
        // Ignore included remotes. Leaf remote IDs become directory names and must be unique
        // ignoring ASCII case.
        if let ReadAs::Leaf = read_as {
            let mut directories: BTreeMap<String, &ItemId> = BTreeMap::new();
            for (id, remote) in &self.remotes {
                remote.validate(id)?;
                if let Some(one) = directories.insert(id.as_str().to_ascii_lowercase(), id) {
                    return Err(ManifestError::RemotesShareOneDirectory {
                        one: one.clone(),
                        other: id.clone(),
                    });
                }
            }
        }

        let mut seen: BTreeMap<&ItemId, usize> = BTreeMap::new();
        for (index, action) in self.actions.iter().enumerate() {
            let action_number = index + 1;
            let record = RecordName::Action(action_number);

            if let Some(id) = action.id()
                && let Some(first) = seen.insert(id, action_number)
            {
                return Err(ManifestError::DuplicateActionId {
                    id: id.clone(),
                    first,
                    second: action_number,
                });
            }

            if action.metadata().writes_both_conditions() {
                return Err(ManifestError::BothConditions { record });
            }

            // Report the forbidden remote reference before checking whether that remote exists.
            if let ReadAs::Included = read_as
                && let Some(source) = action.source()
            {
                check_included_source(source, &record)?;
            }

            action.validate(&record, &self.remotes, read_as)?;
        }
        self.default_disabled.validate()
    }
}
