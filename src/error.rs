//! The crate's error type.
//!
//! One enum for the whole program, grouped by what was being done when it
//! failed. Callers report an error rather than matching on it, so a variant
//! carries only what its message needs; it earns more when something needs to
//! read it.
//!
//! A variant carries the facts and its `#[error]` attribute carries the prose.
//! No variant holds a message formatted at the call site (`guidance.md`,
//! rule 5).

use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::manifest;
use crate::paths::ExistingNode;

#[derive(Debug, Error)]
pub(crate) enum Error {
    // Where batfiles works.
    /// No home directory could be determined for a command that needs one.
    #[error("could not determine a home directory")]
    HomeUnavailable,

    /// The working directory a relative path had to be anchored against could
    /// not be determined.
    #[error("could not determine the current directory: {source}")]
    WorkingDirectory { source: io::Error },

    // Touching the filesystem, for documents, actions, and state files alike.
    /// A path that could not be read or inspected: a document off the disk, or
    /// a destination whose existing node had to be identified.
    #[error("could not read {}: {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },

    /// A path that could not be created or replaced.
    #[error("could not write {}: {source}", .path.display())]
    Write { path: PathBuf, source: io::Error },

    // The documents batfiles parses.
    /// A document that was read but is not valid TOML. The `toml` message is a
    /// multi-line excerpt pointing at the value, so it goes last and on its own
    /// line.
    #[error("invalid TOML in {}:\n{source}", .path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },

    /// A manifest that is valid TOML but breaks a rule serde cannot express.
    /// The rules themselves are in [`manifest::Invalid`].
    #[error("invalid configuration in {}: {source}", .path.display())]
    InvalidManifest {
        path: PathBuf,
        source: manifest::Invalid,
    },

    // Carrying an action out.
    /// An action naming a source the repository does not contain.
    #[error("no such file in the repository: {}", .path.display())]
    SourceMissing { path: PathBuf },

    /// A `source-dir` naming something the repository holds, but not a
    /// directory. There are no children to link, and linking the thing itself
    /// is what a `symlink` action is for.
    #[error("not a directory: {}", .path.display())]
    SourceNotADirectory { path: PathBuf },

    /// A dot-prefixed action over a child whose name already starts with `.`.
    /// The link would be `..name`, which is a legal file name and never the
    /// one that was meant.
    #[error(
        "`{child}` already starts with a dot, so `dot-prefix` would install it as `.{child}`; \
         a dot-prefixed directory holds undotted names"
    )]
    DotPrefixOnDotfile { child: String },

    /// A destination holding something batfiles did not create and cannot
    /// safely replace (`guidance.md`, rule 13). What was found there is
    /// [`ExistingNode`], which [`crate::paths`] decides and renders.
    #[error(
        "{} already exists and is {found}; move it aside and run sync again",
        .path.display()
    )]
    DestinationExists { path: PathBuf, found: ExistingNode },

    /// An action type this build of batfiles cannot carry out on this platform.
    #[error("`{action_type}` actions are not supported on this platform")]
    Unsupported { action_type: &'static str },
}
