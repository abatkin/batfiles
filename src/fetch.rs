//! Download HTTP or `file://` content into a caller-provided staging or scratch
//! file. HTTP transfers require status 200; both verify an optional SHA-256
//! digest.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest as _, Sha256};
use thiserror::Error;
use ureq::tls::{RootCerts, TlsConfig};

use crate::error::Error;

/// How long to wait for a connection before giving up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for a server that took the connection to start answering.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the body may take, in total.
const BODY_TIMEOUT: Duration = Duration::from_secs(600);

/// Maximum number of HTTP redirects to follow.
const MAX_REDIRECTS: u32 = 5;

/// The mode a fetched file lands with.
#[cfg(unix)]
const FETCHED_MODE: u32 = 0o644;

/// Download one URL into the file opened for it, and give it the permissions a
/// fetched file should have.
pub(crate) fn download_file(
    url: &str,
    sha256: Option<&str>,
    mut into: fs::File,
    built_at: &Path,
) -> Result<(), Error> {
    download(url, sha256, &mut into, built_at)?;
    set_download_permissions(&mut into, built_at)
}

/// Download a complete HTTP 200 response, or a whole local file for a
/// `file://` URL, into `into`, verifying an optional digest. `built_at` names
/// the staging or scratch file for diagnostics. Failure may leave partial
/// content in the sink; the caller must discard it rather than publish it.
pub(crate) fn download(
    url: &str,
    sha256: Option<&str>,
    into: &mut impl Write,
    built_at: &Path,
) -> Result<(), Error> {
    let digest = match file_url_path(url) {
        Some(path) => {
            let path = path.map_err(|source| Error::FileUrl {
                url: url.to_owned(),
                source,
            })?;
            let unreadable = |error| Error::Read {
                path: path.clone(),
                source: error,
            };
            let file = fs::File::open(&path).map_err(unreadable)?;
            copy_hashed(file, into, built_at, unreadable)?
        }
        None => {
            let mut response = agent()
                .get(url)
                .call()
                .map_err(|error| failed(url, error))?;
            let status = response.status();
            if status != ureq::http::StatusCode::OK {
                return Err(Error::FetchStatus {
                    url: url.to_owned(),
                    status: status.as_u16(),
                });
            }
            copy_hashed(
                response.body_mut().as_reader(),
                into,
                built_at,
                |error: io::Error| failed(url, error.into()),
            )?
        }
    };

    if let Some(expected) = sha256 {
        let actual = hex(&digest);
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(Error::DigestMismatch {
                url: url.to_owned(),
                expected: expected.to_owned(),
                actual,
            });
        }
    }
    Ok(())
}

/// Download a small document, such as a release's `VERSION`, whole into memory.
pub(crate) fn download_to_memory(url: &str) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    // Writing to a vector cannot fail, so no diagnostic ever names `built_at`.
    download(url, None, &mut body, Path::new(url))?;
    Ok(body)
}

/// Copy everything `reader` holds into `into`, returning its SHA-256.
/// `unreadable` turns a failure reading the source into the caller's error.
fn copy_hashed(
    mut reader: impl Read,
    into: &mut impl Write,
    built_at: &Path,
    unreadable: impl Fn(io::Error) -> Error,
) -> Result<Vec<u8>, Error> {
    let written = |error| Error::Write {
        path: built_at.to_path_buf(),
        source: error,
    };
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let filled = reader.read(&mut buffer).map_err(&unreadable)?;
        if filled == 0 {
            break;
        }
        let arrived = &buffer[..filled];
        hasher.update(arrived);
        into.write_all(arrived).map_err(written)?;
    }
    into.flush().map_err(written)?;
    Ok(hasher.finalize().to_vec())
}

/// Why a `file://` URL names no file on this machine.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub(crate) enum FileUrlError {
    #[error(
        "names a host other than `localhost`; a file URL reads this machine, \
         as `file:///path` or `file://localhost/path`"
    )]
    Host,

    #[error(
        "has a query or fragment, which a file does not; \
         write a `?` or `#` in the path as `%3F` or `%23`"
    )]
    QueryOrFragment,

    #[error("does not name an absolute path")]
    NotAbsolute,

    #[error("does not decode to a path this platform can open")]
    Undecodable,
}

/// The local path a `file://` URL names, or `None` for any other URL. The
/// scheme is matched without regard to case and the path is percent-decoded.
/// The host must be empty or `localhost`.
pub(crate) fn file_url_path(url: &str) -> Option<Result<PathBuf, FileUrlError>> {
    const SCHEME: &str = "file://";
    let rest = url
        .get(..SCHEME.len())
        .filter(|scheme| scheme.eq_ignore_ascii_case(SCHEME))
        .map(|_| &url[SCHEME.len()..])?;
    Some(local_path(rest))
}

/// The path after `file://`: an optional host, then an absolute path.
fn local_path(rest: &str) -> Result<PathBuf, FileUrlError> {
    let (host, path) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
    if !host.is_empty() && !host.eq_ignore_ascii_case("localhost") {
        return Err(FileUrlError::Host);
    }
    if path.contains(['?', '#']) {
        return Err(FileUrlError::QueryOrFragment);
    }
    let decoded: Vec<u8> = percent_encoding::percent_decode_str(path).collect();
    if decoded.is_empty() {
        return Err(FileUrlError::NotAbsolute);
    }
    if decoded.contains(&0) {
        return Err(FileUrlError::Undecodable);
    }
    let path = platform_path(decoded)?;
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(FileUrlError::NotAbsolute)
    }
}

/// A decoded URL path as a Unix path: any bytes, as written.
#[cfg(unix)]
fn platform_path(decoded: Vec<u8>) -> Result<PathBuf, FileUrlError> {
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(std::ffi::OsString::from_vec(decoded)))
}

/// A decoded URL path as a Windows path: `/C:/dir` is `C:/dir`.
#[cfg(not(unix))]
fn platform_path(decoded: Vec<u8>) -> Result<PathBuf, FileUrlError> {
    let path = String::from_utf8(decoded).map_err(|_| FileUrlError::Undecodable)?;
    let drive = path
        .as_bytes()
        .get(1..3)
        .is_some_and(|it| it[0].is_ascii_alphabetic() && it[1] == b':');
    Ok(PathBuf::from(if drive { &path[1..] } else { &path }))
}

/// The client every fetch goes through.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(RESPONSE_TIMEOUT))
        .timeout_recv_body(Some(BODY_TIMEOUT))
        .max_redirects(MAX_REDIRECTS)
        .user_agent(format!("batfiles/{}", crate::cli::VERSION))
        // Use the OS trust store, including locally installed corporate CAs.
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .proxy(ureq::Proxy::try_from_env())
        .build()
        .new_agent()
}

/// Convert HTTP status failures and transport failures to their corresponding crate errors.
fn failed(url: &str, error: ureq::Error) -> Error {
    match error {
        ureq::Error::StatusCode(status) => Error::FetchStatus {
            url: url.to_owned(),
            status,
        },
        other => Error::Fetch {
            url: url.to_owned(),
            source: other,
        },
    }
}

/// Encode digest bytes as lowercase hexadecimal.
fn hex(digest: &[u8]) -> String {
    digest.iter().fold(String::new(), |mut written, byte| {
        // Writing to a String cannot fail.
        let _ = write!(written, "{byte:02x}");
        written
    })
}

/// Give the finished download the permissions a fetched file should have.
#[cfg(unix)]
fn set_download_permissions(into: &mut fs::File, built_at: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;

    into.set_permissions(fs::Permissions::from_mode(FETCHED_MODE))
        .map_err(|error| Error::Write {
            path: built_at.to_path_buf(),
            source: error,
        })
}

/// Keep platform-default permissions on non-Unix systems.
#[cfg(not(unix))]
fn set_download_permissions(_into: &mut fs::File, _built_at: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_is_rendered_the_way_a_manifest_writes_one() {
        assert_eq!(
            hex(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_file_url_names_a_path_on_this_machine() {
        let path = |url: &str| file_url_path(url).expect("a file URL");
        assert_eq!(path("file:///etc/hosts"), Ok(PathBuf::from("/etc/hosts")));
        assert_eq!(
            path("FILE://localhost/etc/hosts"),
            Ok(PathBuf::from("/etc/hosts")),
            "the scheme and the one host a file URL may name are matched without case"
        );
        assert_eq!(
            path("file:///srv/my%20files/a%3Fb"),
            Ok(PathBuf::from("/srv/my files/a?b"))
        );
        assert!(file_url_path("https://e.example/a").is_none());
        assert!(file_url_path("file:").is_none());
    }

    #[test]
    fn a_file_url_that_names_no_local_file_says_why() {
        let path = |url: &str| file_url_path(url).expect("a file URL");
        assert_eq!(path("file://server/share/a"), Err(FileUrlError::Host));
        assert_eq!(path("file:///a?b"), Err(FileUrlError::QueryOrFragment));
        assert_eq!(path("file:///a#b"), Err(FileUrlError::QueryOrFragment));
        assert_eq!(path("file://"), Err(FileUrlError::NotAbsolute));
        assert_eq!(path("file://localhost"), Err(FileUrlError::NotAbsolute));
        assert_eq!(path("file:///a%00b"), Err(FileUrlError::Undecodable));
    }

    #[test]
    fn every_byte_is_two_digits_wide() {
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
