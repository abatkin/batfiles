//! Item IDs and the dotted addresses built from them.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// An ASCII alphanumeric ID, with hyphens and underscores allowed after the first character.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(try_from = "String")]
pub(crate) struct ItemId(String);

/// Why a candidate ID was rejected.
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(try_from = "String")]
pub(crate) struct ItemAddress(String);

/// Why a candidate address was rejected.
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
    pub fn names(&self, id: &ItemId) -> bool {
        self.0 == id.as_str()
    }
}

impl TryFrom<String> for ItemAddress {
    type Error = ItemAddressError;

    fn try_from(address: String) -> Result<Self, Self::Error> {
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
        let mut sorted = ["b", "a.b", "a", "a-c", "a.a"].map(address);
        sorted.sort();
        assert_eq!(
            sorted.map(|address| address.to_string()),
            ["a", "a-c", "a.a", "a.b", "b"]
        );
    }
}
