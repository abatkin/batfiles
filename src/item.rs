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
    /// The address an item answers to: its own ID, under the `id` of the
    /// inclusion that contributed it where one did.
    ///
    /// A leaf repository's action or group is addressed by its ID alone. One an
    /// [`include-remote`](crate::manifest::action::IncludeRemoteAction) spliced
    /// in is addressed under that inclusion, which is what makes `corp.zshrc` a
    /// different address from the leaf's own `zshrc`.
    ///
    /// Both halves are already IDs, so the result satisfies the rule
    /// [`try_from`](Self::try_from) enforces and is built rather than parsed.
    /// An item no address reaches — one written without an `id`, or one an
    /// inclusion written without one contributed — has `None` beside it instead
    /// of a value of this type.
    pub fn qualified(qualifier: Option<&ItemId>, id: &ItemId) -> Self {
        match qualifier {
            None => Self(id.as_str().to_owned()),
            Some(qualifier) => Self(format!("{qualifier}{SEGMENT_SEPARATOR}{id}")),
        }
    }

    /// Whether this address reaches inside the inclusion written with `id`.
    ///
    /// Asked of an inclusion rather than of an item: it is what decides whether
    /// a run that named one thing reads the manifest that might hold it. A
    /// deeper address than anything resolves — `corp.vim-bundles.p10k` — reaches
    /// in and then names nothing, which is the outcome an unresolvable address
    /// already has.
    pub fn qualified_by(&self, id: &ItemId) -> bool {
        self.0
            .split_once(SEGMENT_SEPARATOR)
            .is_some_and(|(first, _)| first == id.as_str())
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

    fn id(id: &str) -> ItemId {
        ItemId::try_from(id.to_owned()).expect("valid ID")
    }

    #[test]
    fn an_item_is_addressed_under_the_inclusion_that_contributed_it() {
        // The rule an inclusion rests on: what a remote contributed is a
        // different address from the leaf's own record of the same ID, and
        // matching is then equality between addresses.
        let (core, zshrc) = (id("core"), id("zshrc"));
        assert_eq!(ItemAddress::qualified(None, &zshrc), address("zshrc"));
        assert_eq!(
            ItemAddress::qualified(Some(&core), &zshrc),
            address("core.zshrc")
        );
        assert_ne!(
            ItemAddress::qualified(Some(&core), &zshrc),
            ItemAddress::qualified(None, &zshrc)
        );
    }

    #[test]
    fn an_address_reaches_into_the_inclusion_its_first_segment_names() {
        // What decides whether an inclusion's manifest is read at all, which is
        // a coarser question than which record the address goes on to name.
        let core = id("core");
        assert!(address("core.zshrc").qualified_by(&core));
        assert!(address("core.vim-bundles.p10k").qualified_by(&core));
        assert!(!address("work.zshrc").qualified_by(&core));
        // The inclusion's own name reaches the record, not inside it.
        assert!(!address("core").qualified_by(&core));
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
