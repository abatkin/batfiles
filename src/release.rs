//! The release base: the URL batfiles releases are published under, compiled in and overridden
//! at run time by `BATFILES_BASE`.

use thiserror::Error as ThisError;

use crate::env::Environment;

/// The official release base.
const OFFICIAL: &str = "https://github.com/abatkin/batfiles/releases";

/// The base this build was released from, falling back to the official one.
const COMPILED: &str = match option_env!("BATFILES_DEFAULT_BASE") {
    Some(base) => base,
    None => OFFICIAL,
};

/// A release base URL without a trailing slash, made only of characters the installers and the
/// stub quote safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReleaseBase(String);

impl ReleaseBase {
    /// `BATFILES_BASE` when it is set and not empty, else the compiled-in base.
    pub fn from_env(env: &Environment) -> Result<Self, ReleaseBaseError> {
        let raw = env
            .get("BATFILES_BASE")
            .filter(|base| !base.is_empty())
            .unwrap_or(COMPILED);
        Self::try_from(raw)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ReleaseBase {
    type Error = ReleaseBaseError;

    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        let base = raw.strip_suffix('/').unwrap_or(raw);
        let quotable = |c: char| c.is_ascii_alphanumeric() || "._~:/@%+=,;!*()-".contains(c);
        let valid = base.split_once("://").is_some_and(|(scheme, rest)| {
            scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
                && !rest.is_empty()
                && rest.chars().all(quotable)
        });
        if valid {
            Ok(Self(base.to_owned()))
        } else {
            Err(ReleaseBaseError {
                base: raw.to_owned(),
            })
        }
    }
}

/// A release base that is not a URL, or holds a character the installers cannot quote.
#[derive(Debug, ThisError)]
#[error(
    "release base `{base}` is not a URL made of letters, digits, and `._~:/@%+=,;!*()-`, which \
     the installers can quote; check BATFILES_BASE"
)]
pub(crate) struct ReleaseBaseError {
    base: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(raw: &str) -> Result<String, ReleaseBaseError> {
        ReleaseBase::try_from(raw).map(|base| base.as_str().to_owned())
    }

    #[test]
    fn the_compiled_in_base_is_valid() {
        assert!(base(COMPILED).is_ok(), "{COMPILED}");
    }

    #[test]
    fn a_trailing_slash_is_dropped() {
        assert_eq!(
            base("https://example.com/batfiles/").expect("valid"),
            "https://example.com/batfiles"
        );
        assert_eq!(
            base("file:///srv/releases").expect("valid"),
            "file:///srv/releases"
        );
    }

    #[test]
    fn what_an_installer_cannot_quote_is_refused() {
        for raw in [
            "",
            "example.com/batfiles",
            "://example.com",
            "1http://example.com",
            "https://",
            "https://example.com/a'b",
            "https://example.com/a b",
            "https://example.com/a&b",
            "https://example.com/a$b",
            "https://example.com/a\\b",
        ] {
            assert!(base(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn the_environment_overrides_the_compiled_in_base() {
        let env = Environment::from_pairs([("BATFILES_BASE", "https://example.com/mine")]);
        assert_eq!(
            ReleaseBase::from_env(&env).expect("valid").as_str(),
            "https://example.com/mine"
        );
        let empty = Environment::from_pairs([("BATFILES_BASE", "")]);
        assert_eq!(
            ReleaseBase::from_env(&empty).expect("valid").as_str(),
            COMPILED
        );
    }
}
