//! Downloading one file, for `fetch-file`.
//!
//! What arrives is written straight into the staging node [`crate::install`]
//! created, hashed on the way, so nothing partial and nothing unverified is ever
//! at a destination — the same property a copy has, bought the same way.
//!
//! Downstream of a mode reader, and structurally so: `install::seed` creates no
//! staging node under `DryRun`, so nothing here is reachable in that mode and no
//! `RunMode` is consulted (`guidance.md`, "Two lists, and why they are not the
//! same one").

use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Write as _};
use std::path::Path;
use std::time::Duration;

use sha2::{Digest as _, Sha256};
use ureq::tls::{RootCerts, TlsConfig};

use crate::error::Error;

/// How long to wait for a connection before giving up.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How long to wait for a server that took the connection to start answering.
///
/// Its own timeout because neither of the others covers this: the connect
/// timeout is spent once the socket is accepted, and the body timeout does not
/// begin until the headers have arrived. Without it, an endpoint that accepts
/// and then says nothing stops the whole run indefinitely.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the body may take, in total.
///
/// A budget for the whole transfer rather than for one read — ureq does not
/// restart it per read — so it is deliberately generous. It is here to bound a
/// stalled transfer, not to decide that a large download on a slow link has
/// taken too long.
const BODY_TIMEOUT: Duration = Duration::from_secs(600);

/// How many redirects to follow. A release URL that redirects to a storage host
/// is ordinary; a chain this long is not.
const MAX_REDIRECTS: u32 = 5;

/// How batfiles introduces itself to a server.
const USER_AGENT: &str = concat!("batfiles/", env!("CARGO_PKG_VERSION"));

/// The mode a fetched file lands with.
///
/// A copy takes its source's permissions; a download has no source on this
/// machine to take them from, so it gets what the `curl -o` this replaces would
/// have produced under an ordinary umask. The umask itself is not consulted —
/// there is no portable way to read one without a libc dependency — and the
/// content came from an unauthenticated public URL either way.
#[cfg(unix)]
const FETCHED_MODE: u32 = 0o644;

/// Download one URL into the file opened for it, verifying it if a digest was
/// declared.
///
/// `built_at` is where `into` lives, which is never the action's destination:
/// everything written here is a staging node, and it reaches the destination
/// only when `install` publishes it whole.
///
/// Three ways this refuses to hand back a file: the server did not answer with
/// one, the transfer did not finish, or the bytes are not the ones the manifest
/// named. Each returns an error, so the fill fails and nothing is published.
///
/// The middle one is the client's: a body that ends before its `Content-Length`
/// is an `UnexpectedEof` out of the reader rather than a short file to be
/// checked for afterwards, so there is no length comparison here.
pub(crate) fn download(
    url: &str,
    sha256: Option<&str>,
    mut into: fs::File,
    built_at: &Path,
) -> Result<(), Error> {
    let mut response = agent()
        .get(url)
        .call()
        .map_err(|error| failed(url, error))?;

    // `call` has already turned 4xx and 5xx into errors and followed what
    // redirects it will, which leaves the answers that are not refusals and not
    // files either: a 204 with nothing in it, a 206 holding one range of one,
    // a 304 pointing at a cache batfiles does not keep. **Only 200 means "the
    // whole thing follows."** Publishing any of the others would put something
    // that is not the file at the destination, where every later run finds it,
    // calls the work done, and reports success over it (rule 15) — and a 206
    // without a digest is exactly that, silently.
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

    widen(&mut into, built_at)
}

/// The client every fetch goes through.
///
/// Built per download rather than kept: a repository fetches a handful of files
/// at most, and one agent per action costs less than a connection pool that
/// outlives the run needs to be reasoned about.
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
///
/// Both are one failed fetch to a caller, but they are not the same sentence to
/// read: a 404 is a manifest naming something that is not there, and a refused
/// connection is a machine that cannot reach it.
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
///
/// Last, not at creation: the staging node is made closed so that an
/// interrupted run never leaves a readable half-file behind (`guidance.md`,
/// rule 15).
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
