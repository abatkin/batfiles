//! Reading the TOML documents batfiles owns.
//!
//! Everything here is about files and syntax. Which document lives where, what
//! its records mean, and when it is rewritten belong to the modules that own
//! those documents.

use std::fs;
use std::path::Path;

use serde::de::DeserializeOwned;

use crate::error::Error;

/// Read and parse one document. A missing file is an error.
///
/// Both failures name the path: a diagnostic that does not say which file it
/// read leaves the reader guessing between the manifest and the state
/// documents.
pub(crate) fn read<T: DeserializeOwned>(path: &Path) -> Result<T, Error> {
    let text = fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| Error::Parse {
        path: path.to_path_buf(),
        source,
    })
}
