//! Download HTTP content into a caller-provided staging or scratch file.
//! Transfers require status 200 and verify an optional SHA-256 digest.

use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

use sha2::{Digest as _, Sha256};
use ureq::tls::{RootCerts, TlsConfig};

use crate::error::Error;

/// How long to wait for a connection before giving up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for a server that took the connection to start answering.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the body may take, in total.
const BODY_TIMEOUT: Duration = Duration::from_secs(600);

/// How many redirects to follow. A release URL that redirects to a storage host
/// is ordinary; a chain this long is not.
const MAX_REDIRECTS: u32 = 5;

/// How batfiles introduces itself to a server.
const USER_AGENT: &str = concat!("batfiles/", env!("CARGO_PKG_VERSION"));

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
    widen(&mut into, built_at)
}

/// Download a complete HTTP 200 response into `into`, verifying an optional digest.
/// `built_at` names the staging or scratch file for diagnostics. Failure may leave
/// partial content in the sink; the caller must discard it rather than publish it.
pub(crate) fn download(
    url: &str,
    sha256: Option<&str>,
    into: &mut impl Write,
    built_at: &Path,
) -> Result<(), Error> {
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

    let mut reader = response.body_mut().as_reader();
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];

    loop {
        let filled = reader
            .read(&mut buffer)
            .map_err(|error| failed(url, error.into()))?;
        if filled == 0 {
            break;
        }
        let arrived = &buffer[..filled];
        hasher.update(arrived);
        into.write_all(arrived).map_err(|error| Error::Write {
            path: built_at.to_path_buf(),
            source: error,
        })?;
    }
    into.flush().map_err(|error| Error::Write {
        path: built_at.to_path_buf(),
        source: error,
    })?;

    if let Some(expected) = sha256 {
        let actual = hex(&hasher.finalize());
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

/// The client every fetch goes through.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_recv_response(Some(RESPONSE_TIMEOUT))
        .timeout_recv_body(Some(BODY_TIMEOUT))
        .max_redirects(MAX_REDIRECTS)
        .user_agent(USER_AGENT)
        // The OS trust store, so a corporate CA is honored where the bundled
        // Mozilla set would refuse the connection.
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .proxy(ureq::Proxy::try_from_env())
        .build()
        .new_agent()
}

/// Tell a server that said no from a network that could not ask.
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

/// The digest as a manifest writes one.
fn hex(digest: &[u8]) -> String {
    digest.iter().fold(String::new(), |mut written, byte| {
        // Writing to a String cannot fail.
        let _ = write!(written, "{byte:02x}");
        written
    })
}

/// Give the finished download the permissions a fetched file should have.
#[cfg(unix)]
fn widen(into: &mut fs::File, built_at: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;

    into.set_permissions(fs::Permissions::from_mode(FETCHED_MODE))
        .map_err(|error| Error::Write {
            path: built_at.to_path_buf(),
            source: error,
        })
}

/// Where a mode means something other than it does on unix, the staging node was
/// created with the platform default and there is nothing to widen.
#[cfg(not(unix))]
fn widen(_into: &mut fs::File, _built_at: &Path) -> Result<(), Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_digest_is_rendered_the_way_a_manifest_writes_one() {
        // The empty string's SHA-256, which is the one digest that can be
        // checked against a published constant without computing it here.
        assert_eq!(
            hex(&Sha256::digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn every_byte_is_two_digits_wide() {
        // A byte below 16 rendered as one digit would shift every digest that
        // contains one, and would still look like a digest.
        assert_eq!(hex(&[0x00, 0x0f, 0xff]), "000fff");
    }
}
