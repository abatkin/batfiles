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
    /// A document that parsed, but breaks a rule spanning more than one record
    /// — which is the kind serde cannot check on its own.
    Invalid { path: PathBuf, message: String },
    /// A path an action wrote that batfiles will not use as written.
    Path {
        field: &'static str,
        value: String,
        message: &'static str,
    },
    /// An action naming a source the repository does not contain.
    SourceMissing { path: PathBuf },
    /// A destination holding something batfiles did not create and cannot
    /// safely replace (`guidance.md`, rule 13).
    Occupied { path: PathBuf },
    /// A path that could not be created, replaced, or inspected.
    Write { path: PathBuf, source: io::Error },
    /// An action this build of batfiles cannot carry out on this platform.
    Unsupported { action: &'static str },
    /// The working directory a relative path had to be anchored against could
    /// not be determined.
    WorkingDirectory { source: io::Error },
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
            Self::Invalid { path, message } => {
                write!(f, "invalid configuration in {}: {message}", path.display())
            }
            Self::Path {
                field,
                value,
                message,
            } => write!(f, "{field} `{value}` {message}"),
            Self::SourceMissing { path } => {
                write!(f, "no such file in the repository: {}", path.display())
            }
            Self::Occupied { path } => write!(
                f,
                "{} already exists and is not a link batfiles created",
                path.display()
            ),
            Self::Write { path, source } => {
                write!(f, "could not write {}: {source}", path.display())
            }
            Self::Unsupported { action } => {
                write!(f, "`{action}` actions are not supported on this platform")
            }
            Self::WorkingDirectory { source } => {
                write!(f, "could not determine the current directory: {source}")
            }
        }
    }
}
