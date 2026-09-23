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
use crate::manifest::check::{RecordName, check_included_source};
use crate::manifest::default_disabled::DefaultDisabled;
use crate::manifest::remote::Remote;
use crate::tomlfile;
use crate::var::VarName;

/// Re-exported as `manifest::Invalid`, matching
/// [`clone_list::Invalid`](crate::clone_list::Invalid) and
/// [`archive::Invalid`](crate::archive::Invalid).
pub(crate) use crate::manifest::check::Invalid;

/// Which repository's manifest is being read.
///
/// Because [inclusion is one level
/// deep](../../docs/repoformat.md#what-an-included-action-may-not-write), an
/// included manifest's actions may not source from a remote, its
/// `include-remote`s need not name a declared remote, and its `[remotes]` are
/// ignored rather than checked. All other rules are the same.
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
    /// Declared repositories, keyed by ID. Sync materializes non-excluded
    /// declarations in ID order, whether or not an action references them.
    #[serde(default)]
    pub remotes: BTreeMap<ItemId, Remote>,

    /// The ordered action list. Order is significant, so this is the one
    /// section that is a sequence rather than a map.
    #[serde(default)]
    pub actions: Vec<Action>,

    /// Static variable values, keyed by name. Serde rejects invalid names and
    /// non-string values as the document is read. The lowest layer of the run's
    /// own [variable set](crate::var_set).
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

    /// Read, parse, and check the leaf repository's own manifest.
    pub fn load(path: &Path) -> Result<Self, CrateError> {
        Self::load_as(path, ReadAs::Leaf)
    }

    /// The same, for the manifest of a remote an `include-remote` selects,
    /// checked [as an included manifest](ReadAs::Included).
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

    /// The rules serde cannot express. Cross-record rules are checked here;
    /// each record applies its own, using the shared [checks](check).
    fn validate(&self, read_as: ReadAs) -> Result<(), Invalid> {
        // Skipped for an included manifest, whose `[remotes]` are ignored.
        //
        // A remote's ID is also its materialization directory, so IDs must
        // differ case-insensitively. IDs are ASCII, so ASCII folding matches a
        // case-insensitive filesystem.
        if let ReadAs::Leaf = read_as {
            let mut directories: BTreeMap<String, &ItemId> = BTreeMap::new();
            for (id, remote) in &self.remotes {
                remote.validate(id)?;
                if let Some(one) = directories.insert(id.as_str().to_ascii_lowercase(), id) {
                    return Err(Invalid::RemotesShareOneDirectory {
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
                return Err(Invalid::DuplicateActionId {
                    id: id.clone(),
                    first,
                    second: action_number,
                });
            }

            if action.metadata().writes_both_conditions() {
                return Err(Invalid::BothConditions { record });
            }

            // Checked first: an included action naming any remote is refused
            // for that, not for naming an undeclared one.
            if let ReadAs::Included = read_as
                && let Some(source) = action.source()
            {
                check_included_source(source, &record)?;
            }

            // Each record checks its own fields; `remotes` lets it check a
            // source's remote against the declarations.
            action.validate(&record, &self.remotes, read_as)?;
        }
        self.default_disabled.validate()
    }
}
