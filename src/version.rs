//! Release versions: the grammar every release is tagged in, ordered by SemVer precedence.

use std::cmp::Ordering;
use std::fmt;

use thiserror::Error;

/// A [release version](../docs/distribution.md#versions): `X.Y.Z` or `X.Y.Z-<pre-release>`,
/// without build metadata. Numbers of any length compare exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version(String);

impl Version {
    /// The numeric core and the pre-release identifiers, if any.
    fn parts(
        &self,
    ) -> (
        impl Iterator<Item = &str>,
        Option<impl Iterator<Item = &str>>,
    ) {
        let (core, pre) = match self.0.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (self.0.as_str(), None),
        };
        (core.split('.'), pre.map(|pre| pre.split('.')))
    }
}

impl TryFrom<&str> for Version {
    type Error = VersionError;

    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        let (core, pre) = match raw.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (raw, None),
        };
        let core_valid = core.split('.').count() == 3 && core.split('.').all(is_number);
        let pre_valid = pre.is_none_or(|pre| pre.split('.').all(is_pre_release_identifier));
        if core_valid && pre_valid {
            Ok(Self(raw.to_owned()))
        } else {
            Err(VersionError {
                version: raw.to_owned(),
            })
        }
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let (core, pre) = self.parts();
        let (other_core, other_pre) = other.parts();
        core.zip(other_core)
            .map(|(a, b)| compare_numbers(a, b))
            .find(|order| order.is_ne())
            .unwrap_or(Ordering::Equal)
            .then_with(|| match (pre, other_pre) {
                (None, None) => Ordering::Equal,
                // A pre-release precedes the release it leads to.
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(pre), Some(other_pre)) => compare_identifiers(pre, other_pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Digits without a leading zero, or `0`.
fn is_number(part: &str) -> bool {
    !part.is_empty()
        && part.bytes().all(|byte| byte.is_ascii_digit())
        && (part == "0" || !part.starts_with('0'))
}

/// A number, or letters, digits, and hyphens with at least one letter or hyphen.
fn is_pre_release_identifier(part: &str) -> bool {
    is_number(part)
        || (part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            && part.bytes().any(|byte| !byte.is_ascii_digit()))
}

/// Two numbers without leading zeros: the longer is larger, and digits decide the rest.
fn compare_numbers(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}

/// Pre-release identifiers in turn: numbers numerically and below anything else, others as
/// ASCII, and a list below a longer one it begins.
fn compare_identifiers<'a>(
    mut a: impl Iterator<Item = &'a str>,
    mut b: impl Iterator<Item = &'a str>,
) -> Ordering {
    loop {
        let order = match (a.next(), b.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) => match (is_number(a), is_number(b)) {
                (true, true) => compare_numbers(a, b),
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                (false, false) => a.cmp(b),
            },
        };
        if order.is_ne() {
            return order;
        }
    }
}

/// Text that is not a release version.
#[derive(Debug, Error)]
#[error(
    "`{version}` is not a release version: X.Y.Z or X.Y.Z-<pre-release>, as \
     docs/distribution.md#versions describes"
)]
pub(crate) struct VersionError {
    version: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(raw: &str) -> Version {
        Version::try_from(raw).expect("a release version")
    }

    #[test]
    fn versions_order_by_semantic_version_precedence() {
        // Each version precedes the next.
        let ordered = [
            "0.9.9",
            "1.2.3-1",
            "1.2.3-alpha",
            "1.2.3-rc.1",
            "1.2.3-rc.1.1",
            "1.2.3-rc.2",
            "1.2.3-rc.10",
            "1.2.3",
            "1.9.9",
            "1.10.0",
            "18446744073709551616.0.0",
            "18446744073709551617.0.0",
        ];
        for pair in ordered.windows(2) {
            assert!(version(pair[0]) < version(pair[1]), "{pair:?}");
            assert!(version(pair[1]) > version(pair[0]), "{pair:?}");
        }
        assert_eq!(
            version("1.2.3-rc.1").cmp(&version("1.2.3-rc.1")),
            Ordering::Equal
        );
    }

    #[test]
    fn identifiers_compare_exactly_and_as_ascii() {
        for (lower, higher) in [
            // Not numbers, however a shell's arithmetic might read them.
            ("1.0.0-100", "1.0.0-1e2"),
            ("1.0.0-1e2", "1.0.0-2e1"),
            // Upper case sorts first, whatever the locale.
            ("1.0.0-B", "1.0.0-a"),
            // Beyond what a floating-point number holds.
            ("1.0.0-rc.9007199254740992", "1.0.0-rc.9007199254740993"),
        ] {
            assert!(version(lower) < version(higher), "{lower} < {higher}");
        }
    }

    #[test]
    fn only_the_release_grammar_is_a_version() {
        for raw in [
            "1.2.3",
            "0.0.0",
            "1.2.3-rc.2",
            "1.2.3-beta",
            "1.2.3-alpha.1.x-2",
            "1.2.3-0",
            "1.2.3--",
            "1.2.3-0a",
        ] {
            assert!(Version::try_from(raw).is_ok(), "{raw}");
        }
        for raw in [
            "",
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.02.3",
            "1.2.03",
            "v1.2.3",
            "1.2.3-",
            "1.2.3-rc..2",
            "1.2.3-rc.02",
            "1.2.3-rc.",
            "1.2.3-.rc",
            "1.2.3-01",
            "1.2.3-rc_2",
            "1.2.3+build",
            "1.2.3-rc.1+build",
            "1.2.x",
            " 1.2.3",
            "1.2.3\n",
        ] {
            assert!(Version::try_from(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn this_build_reports_a_release_version() {
        assert!(Version::try_from(crate::cli::VERSION).is_ok());
    }
}
