//! Read TOML documents, atomically replace them through owned temporary files,
//! and remove them.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::Error;

/// Read and parse one document. A missing file is an error.
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

/// Read and parse a document, returning `T::default()` if the file is missing.
pub(crate) fn read_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T, Error> {
    match read(path) {
        Err(error) if error.is_not_found() => Ok(T::default()),
        other => other,
    }
}

/// Serialize and atomically replace a document, creating missing parents.
/// Existing permissions are applied before writing content. Cleanup is
/// best-effort; no fsync or lock is used.
pub(crate) fn write<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<(), Error> {
    let text = toml::to_string(value).map_err(|source| Error::Serialize {
        path: path.to_path_buf(),
        source,
    })?;

    let write_error = |source| Error::Write {
        path: path.to_path_buf(),
        source,
    };

    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(write_error)?;
    }

    // The temporary file is a sibling so the rename stays within one filesystem.
    let temp = temp_path(path);
    publish(&temp, path, text.as_bytes()).map_err(write_error)
}

/// Remove a document. A missing one is already removed.
pub(crate) fn remove(path: &Path) -> Result<(), Error> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(Error::Write {
            path: path.to_path_buf(),
            source: error,
        }),
        _ => Ok(()),
    }
}

/// Write through an exclusively created temporary file and rename it over `dest`.
/// On failure, remove only the temporary file created by this call.
fn publish(temp: &Path, dest: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = create_guarded(temp, dest)?;
    let written = file.write_all(bytes);
    drop(file);
    let result = written.and_then(|()| fs::rename(temp, dest));
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

/// Exclusively create `temp` and apply existing destination permissions before
/// returning its empty handle. New documents retain the process umask.
/// Permission failures remove the newly created file; occupied paths are untouched.
fn create_guarded(temp: &Path, dest: &Path) -> io::Result<fs::File> {
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp)?;
    if let Ok(metadata) = fs::metadata(dest)
        && let Err(error) = file.set_permissions(metadata.permissions())
    {
        drop(file);
        let _ = fs::remove_file(temp);
        return Err(error);
    }
    Ok(file)
}

/// Choose a hidden sibling name using the process ID and a local counter.
/// Exclusive creation detects collisions with files from other invocations.
fn temp_path(dest: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);

    let name = dest.file_name().unwrap_or_else(|| OsStr::new("document"));
    let unique = NEXT.fetch_add(1, Ordering::Relaxed);
    dest.with_file_name(format!(
        ".{}.{}.{unique}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    type Doc = BTreeMap<String, String>;

    fn doc(pairs: [(&str, &str); 1]) -> Doc {
        pairs
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
            .collect()
    }

    #[test]
    fn a_document_survives_a_write_and_read_round_trip() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("disabled.toml");

        write(&path, &doc([("editor", "nvim")])).expect("write should succeed");
        assert_eq!(read::<Doc>(&path).expect("read"), doc([("editor", "nvim")]));
    }

    #[test]
    fn writing_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested/deeper/disabled.toml");

        write(&path, &doc([("editor", "nvim")])).expect("write should create the directories");
        assert!(path.is_file());
    }

    #[test]
    fn writing_replaces_the_previous_document_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("disabled.toml");

        write(&path, &doc([("editor", "nvim")])).expect("first write");
        write(&path, &doc([("editor", "emacs")])).expect("second write");

        assert_eq!(
            read::<Doc>(&path).expect("read"),
            doc([("editor", "emacs")])
        );
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .map(|entry| entry.expect("entry").file_name())
            .collect();
        assert_eq!(leftovers, [OsStr::new("disabled.toml")]);
    }

    #[test]
    fn a_missing_file_is_reported_as_not_found() {
        let dir = tempfile::tempdir().expect("temp dir");
        let error = read::<Doc>(&dir.path().join("absent.toml")).expect_err("should fail");
        assert!(error.is_not_found());
        assert!(error.to_string().contains("absent.toml"));
    }

    #[test]
    fn read_or_default_only_falls_back_for_a_missing_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            read_or_default::<Doc>(&dir.path().join("absent.toml")).expect("absent is empty"),
            Doc::new()
        );

        let malformed = dir.path().join("broken.toml");
        fs::write(&malformed, "editor = \n").expect("fixture");
        assert!(read_or_default::<Doc>(&malformed).is_err());
    }

    #[test]
    fn a_malformed_document_names_the_file_and_the_location() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("broken.toml");
        fs::write(&path, "editor = 3\n").expect("fixture");

        let error = read::<Doc>(&path).expect_err("should fail");
        assert!(!error.is_not_found());
        let message = error.to_string();
        assert!(message.contains("broken.toml"), "{message}");
        assert!(message.contains("line 1"), "{message}");
    }

    #[test]
    fn a_failed_write_leaves_the_destination_alone() {
        let dir = tempfile::tempdir().expect("temp dir");
        // A directory in place of the file: the rename fails, and the original
        // must survive untouched.
        let path = dir.path().join("occupied.toml");
        fs::create_dir(&path).expect("fixture");

        assert!(write(&path, &doc([("editor", "nvim")])).is_err());
        assert!(path.is_dir());
        let leftovers = fs::read_dir(dir.path()).expect("read dir").count();
        assert_eq!(leftovers, 1, "the temporary file should have been removed");
    }

    #[test]
    fn an_existing_temporary_file_is_neither_truncated_nor_removed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dest = dir.path().join("disabled.toml");
        let temp = temp_path(&dest);
        fs::write(&dest, "original").expect("destination");
        fs::write(&temp, "someone else's file").expect("occupied temporary path");

        let error = publish(&temp, &dest, b"replacement").expect_err("occupied path");
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(&temp).expect("temporary file"),
            "someone else's file"
        );
        assert_eq!(fs::read_to_string(&dest).expect("destination"), "original");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_at_the_temporary_path_and_its_target_are_preserved() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dest = dir.path().join("disabled.toml");
        let temp = temp_path(&dest);
        let target = dir.path().join("other");
        fs::write(&dest, "original").expect("destination");
        fs::write(&target, "other content").expect("target");
        std::os::unix::fs::symlink(&target, &temp).expect("symlink");

        assert_eq!(
            publish(&temp, &dest, b"replacement")
                .expect_err("symlink")
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_link(&temp).expect("symlink remains"), target);
        assert_eq!(
            fs::read_to_string(&target).expect("target"),
            "other content"
        );
        assert_eq!(fs::read_to_string(&dest).expect("destination"), "original");
    }

    /// Tests for Unix permission preservation.
    #[cfg(unix)]
    mod permissions {
        use super::*;
        use std::os::unix::fs::PermissionsExt as _;

        fn mode_of(path: &Path) -> u32 {
            fs::metadata(path).expect("metadata").permissions().mode() & 0o777
        }

        #[test]
        fn a_replacement_keeps_the_permissions_of_what_it_replaced() {
            let dir = tempfile::tempdir().expect("temp dir");
            let path = dir.path().join("disabled.toml");
            fs::write(&path, "editor = \"nvim\"\n").expect("fixture");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("restrict");

            write(&path, &doc([("editor", "emacs")])).expect("write");

            assert_eq!(mode_of(&path), 0o600);
        }

        #[test]
        fn the_temporary_file_is_narrowed_before_it_can_hold_content() {
            let dir = tempfile::tempdir().expect("temp dir");
            let dest = dir.path().join("disabled.toml");
            fs::write(&dest, "editor = \"nvim\"\n").expect("fixture");
            fs::set_permissions(&dest, fs::Permissions::from_mode(0o600)).expect("restrict");

            let temp = temp_path(&dest);
            let file = create_guarded(&temp, &dest).expect("create");

            assert_eq!(mode_of(&temp), 0o600);
            assert_eq!(file.metadata().expect("metadata").len(), 0);
        }

        #[test]
        fn a_temporary_file_with_no_destination_to_match_stays_at_the_umask() {
            let dir = tempfile::tempdir().expect("temp dir");
            let reference = dir.path().join("reference.toml");
            fs::write(&reference, "").expect("fixture");

            let dest = dir.path().join("disabled.toml");
            let temp = temp_path(&dest);
            create_guarded(&temp, &dest).expect("create");

            assert_eq!(mode_of(&temp), mode_of(&reference));
        }

        #[test]
        fn a_new_file_honors_the_umask_like_any_other_create() {
            let dir = tempfile::tempdir().expect("temp dir");
            let reference = dir.path().join("reference.toml");
            fs::write(&reference, "").expect("fixture");

            let path = dir.path().join("disabled.toml");
            write(&path, &doc([("editor", "nvim")])).expect("write");

            assert_eq!(mode_of(&path), mode_of(&reference));
        }
    }

    #[test]
    fn temporary_paths_are_hidden_siblings_that_never_repeat() {
        let dest = Path::new("/config/batfiles/disabled.toml");
        let first = temp_path(dest);
        let second = temp_path(dest);

        assert_ne!(first, second);
        assert_eq!(first.parent(), dest.parent());
        let name = first
            .file_name()
            .expect("name")
            .to_string_lossy()
            .to_string();
        assert!(name.starts_with(".disabled.toml."), "{name}");
        assert!(name.ends_with(".tmp"), "{name}");
    }
}
