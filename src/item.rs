//! Item IDs and the dotted addresses built from them.
//!
//! "Item" is the collective noun for the things the ID rule names — actions,
//! groups, remotes, and manifest entries. An address is defined entirely in
//! terms of IDs, so both types live here.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A validated item ID.
///
/// IDs match `[A-Za-z0-9][A-Za-z0-9_-]*` and name actions, groups, remotes, and
/// manifest entries. Holding one is proof the value was checked, so an ID is
/// validated where it enters and nothing re-checks it afterwards.
///
/// This is deliberately not the user-variable rule: `_hidden` is a valid
/// variable name and not a valid ID, while `9front`, `oh-my-zsh`, and `env` are
/// valid IDs and not valid variable names. In particular an ID cannot contain
/// whitespace, `.`, or `,` — dots compose qualified addresses and commas
/// delimit environment lists.
/// Serialized as the bare string it wraps, so a document batfiles writes reads
/// back the way a hand-written one is spelled.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(try_from = "String")]
pub(crate) struct ItemId(String);

/// Why a candidate ID was rejected.
///
/// The rejected value travels with the error: serde renders this where the user
/// cannot see what was written, so the message has to carry it.
#[derive(Debug, Error)]
#[error(
    "`{candidate}` is not a valid ID: an ID starts with a letter or digit, \
     followed by letters, digits, hyphens, or underscores"
)]
pub(crate) struct ItemIdError {
    candidate: String,
}

impl TryFrom<String> for ItemId {
    type Error = ItemIdError;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        let mut rest = id.chars();
        let valid = matches!(rest.next(), Some(first) if first.is_ascii_alphanumeric())
            && rest.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if valid {
            Ok(Self(id))
        } else {
            Err(ItemIdError { candidate: id })
        }
    }
}

impl ItemId {
    /// The ID as written, for comparing against the segment of an address.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The character separating an address's segments.
const SEGMENT_SEPARATOR: char = '.';

/// A validated address: one or more [`ItemId`]s joined by `.`.
///
/// This is only the shared syntax. Which shapes a repository can *resolve* is a
/// separate question, and one this type deliberately does not ask: `a.b.c.d.e`
/// is a well-formed address that nothing happens to contain, and the commands
/// that record an address without resolving it may write it down.
///
/// Held as the dotted text rather than as a segment list, so ordering is over
/// that text. `disabled.toml` holds a sorted set of addresses and the order is
/// visible in the written document, which is the order a plain string sort
/// gives: `a-c` before `a.b`, because `-` sorts before `.`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(try_from = "String")]
pub(crate) struct ItemAddress(String);

/// Why a candidate address was rejected.
///
/// Carries the whole address rather than the offending segment: the segment
/// alone does not say which line to fix.
#[derive(Debug, Error)]
#[error(
    "`{candidate}` is not a valid address: every dot-separated segment must be an ID \
     starting with a letter or digit, followed by letters, digits, hyphens, or underscores"
)]
pub(crate) struct ItemAddressError {
    candidate: String,
}

impl ItemAddress {
    /// Whether this address names `id`, which is a leaf action or a leaf group.
    ///
    /// Only an unqualified address can, and an ID cannot contain the separator,
    /// so equality is the whole test. A qualified address names an action or a
    /// group an included remote contributed, and no remote exists yet, so it
    /// names nothing — which is an outcome every list holding one already has a
    /// rule for.
    pub fn names(&self, id: &ItemId) -> bool {
        self.0 == id.as_str()
    }
}

impl TryFrom<String> for ItemAddress {
    type Error = ItemAddressError;

    fn try_from(address: String) -> Result<Self, Self::Error> {
        // Every segment is an ID, and `split` yields an empty one for a leading,
        // trailing, or doubled dot, so the empty forms are refused by the same
        // rule rather than by a check of their own.
        if address
            .split(SEGMENT_SEPARATOR)
            .all(|segment| ItemId::try_from(segment.to_owned()).is_ok())
        {
            Ok(Self(address))
        } else {
            Err(ItemAddressError { candidate: address })
        }
    }
}

impl fmt::Display for ItemAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepted(id: &str) -> bool {
        ItemId::try_from(id.to_owned()).is_ok()
    }

    #[test]
    fn an_id_starts_alphanumeric_and_continues_with_hyphens_or_underscores() {
        assert!(accepted("zshrc"));
        assert!(accepted("oh-my-zsh"));
        assert!(accepted("9front"));
        assert!(accepted("p10k_theme"));
        assert!(!accepted("_hidden"));
        assert!(!accepted("-leading"));
        assert!(!accepted(""));
    }

    #[test]
    fn an_id_cannot_contain_a_separator() {
        // Dots compose qualified addresses and commas delimit environment
        // lists, so neither can appear in a segment.
        assert!(!accepted("core.zshrc"));
        assert!(!accepted("zshrc,vimrc"));
        assert!(!accepted("two words"));
    }

    fn address(address: &str) -> ItemAddress {
        ItemAddress::try_from(address.to_owned()).expect("valid address")
    }

    #[test]
    fn an_address_is_any_positive_number_of_id_segments() {
        // Three segments is the deepest form the repository model will ever
        // resolve, but arity is a lookup concern rather than a syntax one.
        for candidate in ["p10k", "core.zshrc", "core.zsh-plugins.p10k", "a.b.c.d.e"] {
            assert_eq!(address(candidate).to_string(), candidate);
        }
    }

    #[test]
    fn an_empty_segment_is_not_an_address() {
        for malformed in ["a..b", ".a", "a.", "", ".", "core._hidden", "my group"] {
            assert!(
                ItemAddress::try_from(malformed.to_owned()).is_err(),
                "`{malformed}` should not be an address"
            );
        }
    }

    #[test]
    fn a_rejected_address_names_the_whole_address() {
        // The offending segment alone would not say which line to fix.
        let error = ItemAddress::try_from("core._hidden".to_owned()).expect_err("invalid segment");
        assert!(error.to_string().contains("`core._hidden`"), "{error}");
    }

    #[test]
    fn only_an_unqualified_address_names_a_leaf_item() {
        let zshrc = ItemId::try_from("zshrc".to_owned()).expect("valid ID");
        assert!(address("zshrc").names(&zshrc));
        assert!(!address("core.zshrc").names(&zshrc));
        assert!(!address("vimrc").names(&zshrc));
    }

    #[test]
    fn an_address_serializes_as_one_dotted_string() {
        #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
        struct Wrapper {
            value: ItemAddress,
        }

        let document = toml::to_string(&Wrapper {
            value: address("core.zshrc"),
        })
        .expect("serialize");
        assert_eq!(document, "value = \"core.zshrc\"\n");
        assert_eq!(
            toml::from_str::<Wrapper>(&document).expect("deserialize"),
            Wrapper {
                value: address("core.zshrc")
            }
        );

        let error = toml::from_str::<Wrapper>("value = 'a..b'\n")
            .expect_err("an invalid address should not deserialize");
        assert!(error.to_string().contains("`a..b`"), "{error}");
    }

    #[test]
    fn addresses_sort_by_their_dotted_text() {
        // The written `disabled.toml` array is this order, and it is the one a
        // plain string sort gives: `-` sorts before `.`, so `a-c` comes first.
        // Comparing segment lists instead would swap these two.
        let mut sorted = ["b", "a.b", "a", "a-c", "a.a"].map(address);
        sorted.sort();
        assert_eq!(
            sorted.map(|address| address.to_string()),
            ["a", "a-c", "a.a", "a.b", "b"]
        );
    }
}
