//! Item IDs, the lists a record names several of them in, and the dotted
//! addresses built from them.

use std::fmt;

use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
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

/// One ID or a list of them, read as a list either way.
///
/// The spelling a field accepts where naming exactly one thing is the common
/// case: `exclude-actions = "p10k"` and `exclude-actions = ["p10k"]` are the
/// same list, and which one was written carries no meaning worth keeping. An
/// absent field and an empty list are not the same, so a record holds an
/// `Option<ItemIdList>` and this type never stands for the absent one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ItemIdList(Vec<ItemId>);

impl ItemIdList {
    /// The IDs as written, in the order they were written.
    pub fn as_slice(&self) -> &[ItemId] {
        &self.0
    }

    /// Whether the list names `id`.
    pub fn contains(&self, id: &ItemId) -> bool {
        self.0.contains(id)
    }
}

impl<'de> Deserialize<'de> for ItemIdList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ItemIdListVisitor;

        impl<'de> Visitor<'de> for ItemIdListVisitor {
            type Value = ItemIdList;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an ID or a list of IDs")
            }

            /// The short form, which is the one-item list. `ItemId`'s own rule
            /// decides it, so a dotted address is refused here as it is
            /// anywhere else an ID is written.
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<ItemIdList, E> {
                let id = ItemId::try_from(value.to_owned()).map_err(E::custom)?;
                Ok(ItemIdList(vec![id]))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<ItemIdList, A::Error> {
                let mut ids = Vec::with_capacity(seq.size_hint().unwrap_or_default());
                while let Some(id) = seq.next_element()? {
                    ids.push(id);
                }
                Ok(ItemIdList(ids))
            }
        }

        deserializer.deserialize_any(ItemIdListVisitor)
    }
}

impl FromIterator<ItemId> for ItemIdList {
    fn from_iter<I: IntoIterator<Item = ItemId>>(ids: I) -> Self {
        Self(ids.into_iter().collect())
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

    /// An `ItemIdList` read the way a manifest hands one over, so that the
    /// short and long spellings go through the same deserializer a record does.
    fn id_list(value: &str) -> Result<ItemIdList, toml::de::Error> {
        #[derive(Deserialize)]
        struct Wrapper {
            value: ItemIdList,
        }

        toml::from_str::<Wrapper>(&format!("value = {value}\n")).map(|wrapper| wrapper.value)
    }

    #[test]
    fn one_id_and_a_one_item_list_are_the_same_list() {
        let expected = ItemIdList::from_iter([id("p10k")]);
        assert_eq!(id_list("'p10k'").expect("the short form"), expected);
        assert_eq!(id_list("['p10k']").expect("the long form"), expected);
    }

    #[test]
    fn an_id_list_keeps_what_was_written_in_the_order_it_was_written() {
        let list = id_list("['zshrc', 'p10k', 'oh-my-zsh']").expect("a list of IDs");
        assert_eq!(
            list.as_slice(),
            [id("zshrc"), id("p10k"), id("oh-my-zsh")].as_slice()
        );
        assert!(list.contains(&id("p10k")));
        assert!(!list.contains(&id("seeds")));
    }

    #[test]
    fn an_empty_list_is_a_list_that_names_nothing() {
        // Distinct from an absent field, which is why a record holds an
        // `Option` and this type has no spelling for "not written".
        let empty = id_list("[]").expect("an empty list");
        assert_eq!(empty, ItemIdList::default());
        assert!(!empty.contains(&id("zshrc")));
    }

    #[test]
    fn every_element_of_an_id_list_is_an_id() {
        // An address is not an ID: a filter names what the included manifest
        // calls a record, and qualifying it would name it twice over.
        for malformed in ["'corp.p10k'", "['zshrc', 'corp.p10k']", "['ok', 2]", "''"] {
            let error = id_list(malformed).expect_err("the value should be refused");
            assert!(
                !error.to_string().is_empty(),
                "`{malformed}` was accepted as a list of IDs"
            );
        }
        let error = id_list("'corp.p10k'").expect_err("a dotted value is not an ID");
        assert!(error.to_string().contains("`corp.p10k`"), "{error}");
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
