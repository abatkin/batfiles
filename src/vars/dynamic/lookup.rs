//! Look up the tag of a GitHub repository's latest release from the redirect its
//! `releases/latest` page answers with. See [how a lookup is
//! made](../../../docs/environment.md#how-github-release-lookups-are-made).

use std::fmt;
use std::io;
use std::time::Duration;

use crate::manifest::vars::GithubRepository;
use crate::source::fetch::{self, Unfollowed};

/// `timeout`'s default.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// What the tag follows in a release page's path.
const TAG_PATH: &str = "/releases/tag/";

/// Ask `host`, an http(s) URL without a trailing `/`, for `repository`'s latest release, and
/// return its tag exactly as GitHub names it. `timeout` bounds the whole request.
pub(crate) fn lookup(
    repository: &GithubRepository,
    host: &str,
    timeout: Duration,
) -> Result<String, LookupError> {
    let url = format!("{host}/{repository}/releases/latest");
    match fetch::request_unfollowed(&url, timeout) {
        Ok(answer) => tag_of(repository, &answer),
        Err(error) if timed_out(&error) => Err(LookupError::TimedOut(timeout)),
        Err(error) => Err(LookupError::Request(error.to_string())),
    }
}

/// Whether a request failed for want of time, however the transport reports it.
fn timed_out(error: &ureq::Error) -> bool {
    match error {
        ureq::Error::Timeout(_) => true,
        ureq::Error::Io(error) => error.kind() == io::ErrorKind::TimedOut,
        _ => false,
    }
}

/// The tag a response names, or why it names none.
fn tag_of(repository: &GithubRepository, answer: &Unfollowed) -> Result<String, LookupError> {
    let repository = repository.clone();
    match answer.status {
        404 => Err(LookupError::NoRepository(repository)),
        301 | 302 | 303 | 307 | 308 => {
            let Some(location) = &answer.location else {
                return Err(LookupError::UnexpectedRedirect {
                    status: answer.status,
                    location: None,
                });
            };
            let path = location
                .find(['?', '#'])
                .map_or(location.as_str(), |end| &location[..end]);
            if let Some(at) = path.find(TAG_PATH) {
                let encoded = &path[at + TAG_PATH.len()..];
                return match percent_encoding::percent_decode_str(encoded).decode_utf8() {
                    Ok(tag) if tag.is_empty() => Err(LookupError::EmptyTag),
                    Ok(tag) => Ok(tag.into_owned()),
                    Err(_) => Err(LookupError::TagNotUtf8),
                };
            }
            if path.ends_with("/releases") {
                Err(LookupError::NoRelease(repository))
            } else if path.ends_with("/releases/latest") {
                Err(LookupError::Moved {
                    repository,
                    location: printable(location),
                })
            } else {
                Err(LookupError::UnexpectedRedirect {
                    status: answer.status,
                    location: printable(location),
                })
            }
        }
        status => Err(LookupError::UnexpectedStatus(status)),
    }
}

/// Text a server supplied, as a message may quote it: itself if it is printable ASCII,
/// otherwise `None`.
fn printable(text: &str) -> Option<String> {
    text.bytes()
        .all(|byte| (b' '..=b'~').contains(&byte))
        .then(|| text.to_owned())
}

/// Why a lookup produced no tag. Displays as a clause following a variable's name, in ASCII.
#[derive(Debug)]
pub(crate) enum LookupError {
    /// The request did not complete: no connection, a DNS or TLS failure, and so on.
    Request(String),
    /// No complete answer within `timeout`.
    TimedOut(Duration),
    /// Answered 404.
    NoRepository(GithubRepository),
    /// Redirected to the repository's releases list.
    NoRelease(GithubRepository),
    /// Redirected to another repository's latest release; the `Location`, if printable.
    Moved {
        repository: GithubRepository,
        location: Option<String>,
    },
    /// A redirect to neither a release nor a known page; the `Location`, if present and
    /// printable.
    UnexpectedRedirect {
        status: u16,
        location: Option<String>,
    },
    /// Neither a redirect nor 404.
    UnexpectedStatus(u16),
    /// The tag after `/releases/tag/` is empty.
    EmptyTag,
    /// The tag does not decode to UTF-8.
    TagNotUtf8,
}

impl fmt::Display for LookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(error) => match printable(error) {
                Some(error) => write!(f, "the release lookup failed: {error}"),
                None => f.write_str("the release lookup failed"),
            },
            Self::TimedOut(limit) => write!(f, "the release lookup timed out after {limit:?}"),
            Self::NoRepository(repository) => {
                write!(f, "GitHub has no public repository `{repository}`")
            }
            Self::NoRelease(repository) => write!(f, "`{repository}` has no published release"),
            Self::Moved {
                repository,
                location: Some(location),
            } => write!(
                f,
                "`{repository}` has moved to `{location}`; update `github-release`"
            ),
            Self::Moved { repository, .. } => write!(
                f,
                "`{repository}` has moved to another repository; update `github-release`"
            ),
            Self::UnexpectedRedirect {
                status,
                location: Some(location),
            } => write!(
                f,
                "the release lookup was redirected ({status}) to `{location}`, which is not a \
                 release"
            ),
            Self::UnexpectedRedirect {
                status,
                location: None,
            } => write!(
                f,
                "the release lookup was redirected ({status}) somewhere that is not a release"
            ),
            Self::UnexpectedStatus(status) => write!(
                f,
                "the release lookup was answered with status {status}, not a redirect to a \
                 release"
            ),
            Self::EmptyTag => f.write_str("the latest release's tag is empty"),
            Self::TagNotUtf8 => f.write_str("the latest release's tag is not valid UTF-8"),
        }
    }
}

#[cfg(test)]
mod tests;
