//! Reading and writing the TOML documents batfiles owns.
//!
//! Four documents share this path: the leaf and remote `batfiles.toml`, and the
//! three local files in `docs/state.md`. Reading is plain parse-and-validate;
//! writing follows the state specification's [whole-document
//! rewrite](../docs/state.md#writing) — serialize in memory, write a temporary
//! file beside the destination, then rename over it, so a reader sees either the
//! complete old document or the complete new one.
//!
//! Everything here is about files and syntax. Which document lives where, what
//! its records mean, and when it is rewritten belong to the modules that own
//! those documents.
#![allow(dead_code, reason = "no command loads or stores a document yet")]

use std::ffi::OsStr;
use std::fmt;
use std::fs;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde::de::DeserializeOwned;

/// A document that could not be read, parsed, serialized, or written.
///
/// Every variant carries the path, because a diagnostic that does not name the
/// file leaves the reader guessing which of the four documents failed. The
/// wrapped `toml` errors already carry the line, column, and offending value.
#[derive(Debug)]
pub(crate) enum Error {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    Serialize {
        path: PathBuf,
        source: toml::ser::Error,
    },
    Write {
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "could not read {}: {source}", path.display())
            }
            // The `toml` message is a multi-line excerpt pointing at the value,
            // so it goes last and on its own line.
            Self::Parse { path, source } => {
                write!(f, "invalid TOML in {}:\n{source}", path.display())
            }
            Self::Serialize { path, source } => {
                write!(f, "could not serialize {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(f, "could not write {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::Serialize { source, .. } => Some(source),
        }
    }
}

impl Error {
    /// Whether the failure was simply that the file does not exist.
    ///
    /// The state files treat that as an empty document; a leaf `batfiles.toml`
    /// does not.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Read { source, .. } if source.kind() == io::ErrorKind::NotFound)
    }
}

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

/// Read and parse one document, treating a missing file as an empty one.
///
/// Only a missing file falls back. An unreadable or malformed document is still
/// fatal and leaves the file untouched, per `docs/state.md#reading-and-validation`.
pub(crate) fn read_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T, Error> {
    match read(path) {
        Err(error) if error.is_not_found() => Ok(T::default()),
        other => other,
    }
}

/// Serialize `value` and replace `path` with it atomically.
///
/// The rename is the commit point. If anything before it fails the temporary
/// file is removed and the destination keeps its previous contents. Missing
/// parent directories are created. There is no fsync: `docs/state.md#writing`
/// buys atomicity, not crash durability.
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
    publish(&temp, path, text.as_bytes()).map_err(|source| {
        let _ = fs::remove_file(&temp);
        write_error(source)
    })
}

/// Write the bytes to `temp` and rename it over `dest`.
///
/// The order matters. A replacement keeps the permissions the destination
/// already had, and it has to acquire them while it is still empty: the
/// temporary file is created under the process umask, so writing first and
/// narrowing afterwards would publish the finished document through a
/// world-readable sibling — exactly in the case where the destination was
/// deliberately restricted, and `vars.toml` is the file most likely to hold
/// something worth restricting.
fn publish(temp: &Path, dest: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = create_guarded(temp, dest)?;
    file.write_all(bytes)?;
    drop(file);

    fs::rename(temp, dest)
}

/// Create `temp` empty and, when `dest` already exists, narrow it to `dest`'s
/// permissions — all before the caller has anything to write into it.
///
/// Splitting this out is what keeps the ordering true: the only handle a caller
/// can write through is one this function has already protected. A brand new
/// file keeps what the umask gave it, which is what the spec asks for.
fn create_guarded(temp: &Path, dest: &Path) -> io::Result<fs::File> {
    let file = fs::File::create(temp)?;
    if let Ok(metadata) = fs::metadata(dest) {
        // Applied through the open handle rather than the path, so nothing can
        // swap the file out from under it. A failure here is fatal instead of
        // best-effort: publishing a replacement more permissive than what it
        // replaces would quietly widen access to the destination.
        file.set_permissions(metadata.permissions())?;
    }
    Ok(file)
}

/// A sibling path for the temporary file: dot-prefixed so it is inconspicuous,
/// and tagged with the process id and a counter so concurrent writers — within
/// this process or across processes — never share one.
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
        let path = dir.path().join("vars.toml");

        write(&path, &doc([("editor", "nvim")])).expect("write should succeed");
        assert_eq!(read::<Doc>(&path).expect("read"), doc([("editor", "nvim")]));
    }

    #[test]
    fn writing_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("nested/deeper/vars.toml");

        write(&path, &doc([("editor", "nvim")])).expect("write should create the directories");
        assert!(path.is_file());
    }

    #[test]
    fn writing_replaces_the_previous_document_and_leaves_no_temporary_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("vars.toml");

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
        assert_eq!(leftovers, [OsStr::new("vars.toml")]);
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

    /// Permissions are a Unix mode here; on Windows `Permissions` carries only
    /// the read-only flag, so there is nothing equivalent to assert.
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
            let path = dir.path().join("vars.toml");
            fs::write(&path, "editor = \"nvim\"\n").expect("fixture");
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("restrict");

            write(&path, &doc([("editor", "emacs")])).expect("write");

            assert_eq!(mode_of(&path), 0o600);
        }

        #[test]
        fn the_temporary_file_is_narrowed_before_it_can_hold_content() {
            // A restricted document must be unreadable to other users for the
            // whole time it exists, not only once it has been renamed into
            // place. Writing first and adopting the mode afterwards would leave
            // the finished document sitting in a world-readable sibling, so the
            // guarantee is that the handle a caller writes through is already
            // narrowed and still empty.
            let dir = tempfile::tempdir().expect("temp dir");
            let dest = dir.path().join("vars.toml");
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

            let dest = dir.path().join("vars.toml");
            let temp = temp_path(&dest);
            create_guarded(&temp, &dest).expect("create");

            assert_eq!(mode_of(&temp), mode_of(&reference));
        }

        #[test]
        fn a_new_file_honors_the_umask_like_any_other_create() {
            // Asserting a literal mode would depend on the umask CI happens to
            // run with, so compare against an ordinary create in the same
            // process instead.
            let dir = tempfile::tempdir().expect("temp dir");
            let reference = dir.path().join("reference.toml");
            fs::write(&reference, "").expect("fixture");

            let path = dir.path().join("vars.toml");
            write(&path, &doc([("editor", "nvim")])).expect("write");

            assert_eq!(mode_of(&path), mode_of(&reference));
        }
    }

    #[test]
    fn temporary_paths_are_hidden_siblings_that_never_repeat() {
        let dest = Path::new("/config/batfiles/vars.toml");
        let first = temp_path(dest);
        let second = temp_path(dest);

        assert_ne!(first, second);
        assert_eq!(first.parent(), dest.parent());
        let name = first
            .file_name()
            .expect("name")
            .to_string_lossy()
            .to_string();
        assert!(name.starts_with(".vars.toml."), "{name}");
        assert!(name.ends_with(".tmp"), "{name}");
    }
}
