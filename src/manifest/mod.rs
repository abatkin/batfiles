//! Parse and validate `batfiles.toml` manifests, and the declarations they are
//! made of: item names, conditions, templates, repository paths, entry filters, clone
//! lists, and the `disabled.toml` that switches items off on this machine.

pub(crate) mod actions;
pub(crate) mod check;
pub(crate) mod clone_list;
pub(crate) mod condition;
pub(crate) mod default_disabled;
pub(crate) mod disabled;
pub(crate) mod duration;
pub(crate) mod entry_filter;
pub(crate) mod item;
pub(crate) mod remote;
pub(crate) mod repo_path;
pub(crate) mod template;
pub(crate) mod trigger;
pub(crate) mod vars;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::error::Error as CrateError;
use crate::filesystem::tomlfile;
use crate::manifest::actions::Action;
use crate::manifest::check::{check_conditions, check_included_source, check_var};
use crate::manifest::default_disabled::DefaultDisabled;
use crate::manifest::item::{ItemId, ItemKind};
use crate::manifest::remote::Remote;
use crate::manifest::vars::VarSpec;
use crate::vars::name::VarName;

pub(crate) use crate::manifest::check::ManifestError;

/// What a manifest declares, as validation errors name it: a one-based position for actions and
/// bootstrap candidates, or a remote ID.
#[derive(Debug, Clone)]
pub(crate) enum DeclarationName {
    /// An action, by its one-based position in `[[actions]]`.
    Action(usize),
    /// A `[default-disabled]` candidate, by its array and one-based position.
    Candidate { kind: ItemKind, number: usize },
    /// A `[remotes]` entry, by the ID it was declared under.
    Remote(ItemId),
    /// A `[vars]` entry, by its name.
    Var(VarName),
}

impl fmt::Display for DeclarationName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Action(number) => write!(f, "action {number}"),
            Self::Candidate { kind, number } => write!(f, "default-disabled {kind} {number}"),
            Self::Remote(id) => write!(f, "remote `{id}`"),
            Self::Var(name) => write!(f, "variable `{name}`"),
        }
    }
}

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

        // Where each ID is first declared, for `after` targets.
        let mut positions: BTreeMap<&ItemId, usize> = BTreeMap::new();
        for (index, action) in self.actions.iter().enumerate() {
            if let Some(id) = action.id() {
                positions.entry(id).or_insert(index + 1);
            }
        }
        let mut seen: BTreeMap<&ItemId, usize> = BTreeMap::new();
        for (index, action) in self.actions.iter().enumerate() {
            let action_number = index + 1;
            let record = DeclarationName::Action(action_number);

            if let Some(id) = action.id()
                && let Some(first) = seen.insert(id, action_number)
            {
                return Err(ManifestError::DuplicateActionId {
                    id: id.clone(),
                    first,
                    second: action_number,
                });
            }

            let metadata = action.metadata();
            check_conditions(metadata.when, metadata.unless, &record)?;

            // Report the forbidden remote reference before checking whether that remote exists.
            if let ReadAs::Included = read_as
                && let Some(source) = action.source()
            {
                check_included_source(source, &record)?;
            }

            action.validate(&record, &positions, &self.remotes, read_as)?;
        }
        for (name, spec) in &self.vars {
            check_var(spec, &DeclarationName::Var(name.clone()))?;
        }
        self.default_disabled.validate()
    }
}
