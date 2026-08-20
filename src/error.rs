//! The crate's error type.
//!
//! One enum for the whole program. Callers report an error rather than matching
//! on it, so a variant carries only what its message needs; it earns more when
//! something needs to read it.

use std::fmt;
use std::io;
use std::path::PathBuf;

#[derive(Debug)]
pub(crate) enum Error {
    /// No home directory could be determined for a command that needs one.
    HomeUnavailable,
    /// A document that could not be read off the disk.
    Read { path: PathBuf, source: io::Error },
    /// A document that was read but is not valid TOML.
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeUnavailable => write!(f, "could not determine a home directory"),
            Self::Read { path, source } => {
                write!(f, "could not read {}: {source}", path.display())
            }
            // The `toml` message is a multi-line excerpt pointing at the value,
            // so it goes last and on its own line.
            Self::Parse { path, source } => {
                write!(f, "invalid TOML in {}:\n{source}", path.display())
            }
        }
    }
}
