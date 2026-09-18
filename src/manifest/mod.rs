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

/// Re-exported so that a manifest's load error is `manifest::Invalid` from
/// outside, as [`clone_list`](crate::clone_list::Invalid)'s and
/// [`archive`](crate::archive::Invalid)'s are: which module within `manifest`
/// holds it is not something a caller needs to track.
pub(crate) use crate::manifest::check::Invalid;

/// Which repository's manifest is being read, for the rules that depend on the
/// answer.
///
/// All three follow from inclusion being one level deep: remote references
/// belong to the leaf repository. An action an inclusion brings in may not
/// source from a remote; an `include-remote` it brings in is not required to
/// name a declared remote, since it is dropped rather than followed; and the
/// `[remotes]` such a manifest declares is
/// [ignored](crate::action::include_remote) rather than checked, because nothing
/// in the run can reach one. Everything else a manifest has to satisfy it
/// satisfies the same way in both, so this is a mode rather than a second
/// reader.
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

    /// Read, parse, and check the leaf repository's own manifest.
    pub fn load(path: &Path) -> Result<Self, CrateError> {
        Self::load_as(path, ReadAs::Leaf)
    }

    /// The same, for the manifest of a remote an `include-remote` selects,
    /// which is checked against [one extra rule](ReadAs).
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

    /// The rules serde cannot express, checked as the document is read.
    ///
    /// The ones spanning more than one record are here, since nothing smaller
    /// than the document can see them; each record's own rules are its to
    /// apply, over the [checks](check) they share.
    fn validate(&self, read_as: ReadAs) -> Result<(), Invalid> {
        // Ahead of the actions, since an action reaching a remote's content is
        // reaching one of these. Nothing in an included manifest reaches one, so
        // there is nothing there for these rules to hold together and they are
        // not applied: what such a map declares is the other repository's
        // business, answered where that repository is the leaf.
        //
        // A remote's ID is also the directory it materializes in, so the keys
        // have to be distinct as directory names and not only as map keys. An
        // ID is ASCII by its own rule, so folding it is exactly what a
        // case-insensitive filesystem does to it.
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

            // Ahead of the shared rules, and the more specific answer where a
            // record breaks both: an included action naming a remote is refused
            // for naming one at all, rather than for naming one that the
            // included manifest happens not to declare.
            if let ReadAs::Included = read_as
                && let Some(source) = action.source()
            {
                check_included_source(source, &record)?;
            }

            // Which of a record's fields are paths, and which rule each one
            // follows, is the record's own answer. The remotes go with it for
            // the one source rule that reaches past the record: the remote a
            // path names has to be one of the records above. How this manifest
            // is read goes with them for the rule that does not hold of an
            // inclusion's own records.
            action.validate(&record, &self.remotes, read_as)?;
        }
        self.default_disabled.validate()
    }
}
