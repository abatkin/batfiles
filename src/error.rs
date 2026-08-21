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

use std::fmt;
use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::item::ItemId;

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

    // Documents batfiles reads.
    /// A document that could not be read off the disk.
    #[error("could not read {}: {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },

    /// A document that was read but is not valid TOML. The `toml` message is a
    /// multi-line excerpt pointing at the value, so it goes last and on its own
    /// line.
    #[error("invalid TOML in {}:\n{source}", .path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },

    /// Two actions claiming one ID, which would make every address naming it
    /// ambiguous. The first manifest rule serde cannot check on its own; when
    /// the third lands these collapse into one variant over a rule enum.
    #[error(
        "invalid configuration in {}: action {second} repeats the id `{id}`, \
         which action {first} already uses",
        .path.display()
    )]
    DuplicateActionId {
        path: PathBuf,
        id: ItemId,
        first: usize,
        second: usize,
    },

    // Paths a record wrote that batfiles will not use as written.
    #[error("source `{value}` is absolute; a source names a path within the repository")]
    SourceAbsolute { value: String },

    #[error("source `{value}` resolves outside the repository")]
    SourceOutsideRepository { value: String },

    #[error(
        "destination `{value}` names another user's home; `~` expands only to the selected home"
    )]
    DestinationOtherHome { value: String },

    // Carrying an action out.
    /// An action naming a source the repository does not contain.
    #[error("no such file in the repository: {}", .path.display())]
    SourceMissing { path: PathBuf },

    /// A destination holding something batfiles did not create and cannot
    /// safely replace (`guidance.md`, rule 13).
    #[error(
        "{} already exists and is {found}; move it aside and run sync again",
        .path.display()
    )]
    DestinationExists { path: PathBuf, found: ExistingNode },

    /// A path that could not be created, replaced, or inspected.
    #[error("could not write {}: {source}", .path.display())]
    Write { path: PathBuf, source: io::Error },

    /// An action this build of batfiles cannot carry out on this platform.
    #[error("`{action}` actions are not supported on this platform")]
    Unsupported { action: &'static str },
}

/// What a refused destination turned out to hold, as the data half of
/// [`Error::DestinationExists`].
///
/// Which one it is decides nothing — all four are refused — but it is the
/// difference between a diagnostic someone can act on and one that only says
/// no. This is the crate's one hand-written `Display`: the symlink case renders
/// conditionally, which no `#[error]` attribute can express.
#[derive(Debug)]
pub(crate) enum ExistingNode {
    File,
    Directory,
    /// A symlink pointing somewhere other than into the repository, named both
    /// as it is written and as it resolves: a relative target is read from the
    /// link's own directory, so the spelling alone does not say where it goes.
    Link {
        written: PathBuf,
        points_at: PathBuf,
    },
    /// A socket, a fifo, a device — something batfiles has no idea how to give
    /// back, which is exactly why it will not take it.
    Other,
}

impl fmt::Display for ExistingNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => write!(f, "a regular file"),
            Self::Directory => write!(f, "a directory"),
            Self::Link { written, points_at } => {
                write!(f, "a symlink to {}", written.display())?;
                // An absolute target resolves to itself, and printing it twice
                // reads as though two paths were involved.
                if written != points_at {
                    write!(f, " ({})", points_at.display())?;
                }
                write!(f, ", which is outside the repository")
            }
            Self::Other => write!(f, "neither a regular file, a directory, nor a symlink"),
        }
    }
}
