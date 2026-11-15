//! The run lock: one exclusive lock under the cache directory, held for a whole
//! state-writing command, so that two batfiles runs sharing that directory
//! cannot interleave.

use std::fs::{self, File, OpenOptions, TryLockError};
use std::path::Path;

use crate::error::Error;

/// The lock file's name inside the cache directory.
const FILE_NAME: &str = "run.lock";

/// An exclusive run lock, released when dropped or when the process exits.
#[derive(Debug)]
pub(crate) struct RunLock {
    /// Held only so the lock lives as long as the guard.
    _file: File,
}

impl RunLock {
    /// Take the lock under `cache_dir` without waiting, creating the directory
    /// and an empty lock file when missing. Never truncates, writes, or removes
    /// the file.
    ///
    /// # Errors
    ///
    /// [`Error::RunLockHeld`] when another process holds the lock, and
    /// [`Error::RunLock`] when the file cannot be created, opened, or locked.
    pub fn acquire(cache_dir: &Path) -> Result<Self, Error> {
        let path = cache_dir.join(FILE_NAME);
        let failed = |source| Error::RunLock {
            path: path.clone(),
            source,
        };
        fs::create_dir_all(cache_dir).map_err(failed)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(failed)?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err(Error::RunLockHeld { path }),
            Err(TryLockError::Error(source)) => Err(failed(source)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_acquisition_is_refused_until_the_first_is_dropped() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let cache = dir.path().join("absent/cache");

        let held = RunLock::acquire(&cache).expect("the first acquisition");
        let path = cache.join(FILE_NAME);
        assert_eq!(fs::metadata(&path).expect("the lock file").len(), 0);

        let refused = RunLock::acquire(&cache).expect_err("a second acquisition");
        assert!(
            matches!(&refused, Error::RunLockHeld { path: named } if *named == path),
            "{refused:?}"
        );

        drop(held);
        RunLock::acquire(&cache).expect("an acquisition after release");
        assert!(path.exists(), "the lock file is never removed");
    }
}
