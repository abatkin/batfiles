//! The crate's error type.
//!
//! One enum for the whole program. Callers report an error rather than matching
//! on it, so it carries a message and nothing else; a variant earns fields when
//! something needs to read them.

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Error {
    /// No home directory could be determined for a command that needs one.
    HomeUnavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HomeUnavailable => write!(f, "could not determine a home directory"),
        }
    }
}
