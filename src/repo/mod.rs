//! `batfiles.toml`: the one file in a repository with intrinsic meaning.
//!
//! The record mirrors the file. Constraints that span fields cannot be
//! expressed in serde, so they parse unchecked here and are rejected by
//! [`BatfilesConfig::validate`].

mod action;

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::Error;
use crate::item::ItemId;
use crate::repo::action::Action;
use crate::tomlfile;

/// A parsed `batfiles.toml`.
///
/// Every section is optional, and there is no format-version field. The
/// document is a closed record: a top-level key batfiles does not know is an
/// error rather than something to ignore, so a section belonging to a slice
/// that has not landed — `[vars]`, `[remotes]` — fails outright instead of
/// looking as though it took effect.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct BatfilesConfig {
    /// The ordered action list. Order is significant, so this is the one
    /// section that is a sequence rather than a map.
    #[serde(default)]
    pub actions: Vec<Action>,
}

impl BatfilesConfig {
    /// The manifest's name within a repository root.
    ///
    /// The name travels with the parser, but *where* a repository is does not:
    /// a leaf comes from the resolved roots and a remote will come from its
    /// materialization, so callers pass a whole path to [`load`](Self::load).
    pub const FILE_NAME: &'static str = "batfiles.toml";

    /// Read, parse, and check one manifest.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let config: Self = tomlfile::read(path)?;
        config.validate(path)?;
        Ok(config)
    }

    /// The cross-record rules serde cannot express.
    ///
    /// One so far: action IDs share a single namespace within a repository, so
    /// a repeated one makes every address naming it ambiguous. Checking it here
    /// rejects the manifest before anything interprets the actions, rather than
    /// at whichever use happens to come first. Rules arrive with the fields
    /// they span — `when` excluding `unless` at 5.6, `symlink`'s two modes when
    /// directory mode lands.
    ///
    /// **One problem at a time.** The first offense returns, so a manifest with
    /// two faults reports the earlier one and the next run reports the rest.
    fn validate(&self, path: &Path) -> Result<(), Error> {
        let mut seen: BTreeMap<&ItemId, usize> = BTreeMap::new();
        for (index, action) in self.actions.iter().enumerate() {
            let Some(id) = action.id() else { continue };
            let position = index + 1;
            if let Some(first) = seen.insert(id, position) {
                return Err(Error::Invalid {
                    path: path.to_path_buf(),
                    message: format!(
                        "action {position} repeats the id `{id}`, \
                         which action {first} already uses"
                    ),
                });
            }
        }
        Ok(())
    }
}
