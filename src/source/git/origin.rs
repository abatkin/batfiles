//! Read the origin a checkout records, and decide whether it names the repository a clone
//! source does. See [origin verification](../../../docs/safety.md#permissions-follow-the-origin).

use std::ffi::OsStr;
use std::path::Path;

use super::clone::{GitDirectory, own_git_directory};
use super::process::launch;
use crate::error::Error;
use crate::filesystem::paths;

/// What a checkout records as its `origin` fetch URL.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    /// Exactly one URL, as Git recorded it.
    Url(String),
    /// No `remote.origin.url`.
    Missing,
    /// More than one `remote.origin.url`.
    Ambiguous,
    /// No `.git` directory of the checkout's own, so there is nothing of its own to read.
    NotItsOwn,
    /// Git could not read the checkout's configuration, or the URL is not text.
    Unreadable,
}

/// Read the `origin` URL recorded in `checkout`'s own configuration. Git reads the checkout's
/// `.git` directory by name, so it never falls back to a repository enclosing `checkout`.
/// Configuration outside that directory is not consulted. Fails only if Git cannot run.
pub(crate) fn recorded_origin(checkout: &Path) -> Result<Origin, Error> {
    if !matches!(own_git_directory(checkout)?, GitDirectory::Own) {
        return Ok(Origin::NotItsOwn);
    }
    let git_dir = checkout.join(".git");
    let output = launch(
        Some(checkout),
        &[
            OsStr::new("--git-dir"),
            git_dir.as_os_str(),
            OsStr::new("config"),
            OsStr::new("--local"),
            OsStr::new("--null"),
            OsStr::new("--get-all"),
            OsStr::new("remote.origin.url"),
        ],
    )?;
    Ok(match output.status.code() {
        Some(0) => {
            let printed = output.stdout.strip_suffix(b"\0").unwrap_or(&output.stdout);
            let mut values = printed.split(|byte| *byte == 0);
            match (values.next(), values.next()) {
                (Some(value), None) => {
                    String::from_utf8(value.to_vec()).map_or(Origin::Unreadable, Origin::Url)
                }
                (Some(_), Some(_)) => Origin::Ambiguous,
                (None, _) => Origin::Missing,
            }
        }
        // `git config` exits 1 when the key is absent.
        Some(1) => Origin::Missing,
        _ => Origin::Unreadable,
    })
}

/// Whether `recorded`, the origin Git recorded when it cloned, names the repository `declared`
/// does. A relative local path in either is resolved against `working_directory`, where
/// batfiles runs `git clone`, and local paths compare after normalization. Anything else, a
/// URL with a scheme or an `[user@]host:path` address, compares exactly.
pub(crate) fn same_origin(declared: &str, recorded: &str, working_directory: &Path) -> bool {
    match (local_path(declared), local_path(recorded)) {
        (Some(declared), Some(recorded)) => {
            paths::canonicalize_or_normalize(&working_directory.join(declared))
                == paths::canonicalize_or_normalize(&working_directory.join(recorded))
        }
        (None, None) => declared == recorded,
        _ => false,
    }
}

/// `source` as a path, if Git reads it as one: no `://`, and no `:` ahead of the first
/// directory separator, except in a Windows drive prefix.
fn local_path(source: &str) -> Option<&Path> {
    if source.contains("://") {
        return None;
    }
    let separator = source.find(|c| c == '/' || (cfg!(windows) && c == '\\'));
    let local = match source.find(':') {
        None => true,
        Some(colon) => separator.is_some_and(|separator| separator < colon) || drive(source),
    };
    local.then(|| Path::new(source))
}

/// Whether `source` starts with a Windows drive prefix such as `C:`.
fn drive(source: &str) -> bool {
    let bytes = source.as_bytes();
    cfg!(windows) && bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_addresses_compare_exactly() {
        let here = Path::new("/work");
        for (declared, recorded, same) in [
            ("https://e.example/a.git", "https://e.example/a.git", true),
            ("https://e.example/a.git", "https://e.example/a.git/", false),
            ("https://e.example/a.git", "https://e.example/b.git", false),
            ("git@e.example:a.git", "git@e.example:a.git", true),
            ("git@e.example:a.git", "git@e.example:b.git", false),
            ("file:///srv/a.git", "file:///srv/a.git", true),
            ("file:///srv/a.git", "/srv/a.git", false),
            ("git@e.example:a.git", "/work/git@e.example:a.git", false),
        ] {
            assert_eq!(
                same_origin(declared, recorded, here),
                same,
                "{declared} against {recorded}"
            );
        }
    }

    #[test]
    #[cfg(unix)]
    fn local_paths_compare_as_git_recorded_them() {
        let here = Path::new("/work/leaf");
        for (declared, recorded, same) in [
            ("/srv/a.git", "/srv/a.git", true),
            ("../a.git", "/work/leaf/../a.git", true),
            ("./x/../../a.git", "/work/a.git", true),
            ("a.git", "/work/leaf/a.git", true),
            ("../a.git", "/work/leaf/../b.git", false),
            ("/srv/a.git", "/srv/b.git", false),
        ] {
            assert_eq!(
                same_origin(declared, recorded, here),
                same,
                "{declared} against {recorded}"
            );
        }
    }
}
