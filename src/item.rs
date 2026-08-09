//! Item IDs and the dotted addresses built from them.
//!
//! "Item" is the collective noun for the things the ID rule names — actions,
//! groups, remotes, and manifest entries. An address is defined entirely in
//! terms of IDs, so both types live here.
//!
//! The rules are implemented here; the commands that resolve an address against
//! a repository are not written yet.

use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Serialize, Serializer};

/// A validated item ID.
///
/// IDs match `[A-Za-z0-9][A-Za-z0-9_-]*` and name actions, `include-remote`
/// inclusions, manifest entries, remotes, and groups. Holding an `ItemId` is
/// proof the value has already been checked, so IDs are validated once where
/// they enter and the rest of the code never re-checks.
///
/// This is **not** the user-variable rule that [`VarName`](crate::var::VarName)
/// enforces, and the two are deliberately different: `_hidden` is a valid
/// variable name but not a valid ID, while `9front`, `oh-my-zsh`, and `env` are
/// valid IDs but not valid variable names. Neither validator can stand in for
/// the other.
///
/// In particular an ID cannot contain whitespace, `.`, or `,`: dots separate the
/// segments of an [`ItemAddress`] and commas delimit environment lists.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct ItemId(String);

/// Why a candidate ID was rejected.
///
/// There is only one way to break the rule, but the rejected value travels with
/// the error: serde renders it verbatim, and a command diagnostic has to name
/// the value the user actually wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemIdError {
    candidate: String,
}

impl ItemId {
    /// Validate `id` and wrap it, or report why it was rejected.
    pub fn new(id: &str) -> Result<Self, ItemIdError> {
        validate_id(id)?;
        Ok(Self(id.to_owned()))
    }
}

/// The rule itself, shared by both constructors so an owned ID is checked
/// without being copied first.
fn validate_id(id: &str) -> Result<(), ItemIdError> {
    let rejected = || ItemIdError {
        candidate: id.to_owned(),
    };

    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return Err(rejected()),
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(rejected());
    }
    Ok(())
}

impl fmt::Display for ItemIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a valid ID: an ID must start with a letter or digit, \
             followed by letters, digits, hyphens, or underscores",
            self.candidate
        )
    }
}

impl std::error::Error for ItemIdError {}

impl fmt::Display for ItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ItemId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<&str> for ItemId {
    type Error = ItemIdError;

    fn try_from(id: &str) -> Result<Self, Self::Error> {
        Self::new(id)
    }
}

impl TryFrom<String> for ItemId {
    type Error = ItemIdError;

    fn try_from(id: String) -> Result<Self, Self::Error> {
        validate_id(&id)?;
        Ok(Self(id))
    }
}

impl Serialize for ItemId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

/// The character separating an address's segments.
const SEGMENT_SEPARATOR: char = '.';

/// A validated address: one or more [`ItemId`]s joined by `.`.
///
/// The forms the current repository model can *resolve* are one to three
/// segments, but this type is only the shared syntax: any positive number of
/// valid ID segments parses. Arity and existence are lookup concerns, so
/// `a.b.c.d.e` is a well-formed address that no repository happens to contain,
/// and the persistent enable/disable commands — which resolve nothing — may
/// record it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct ItemAddress {
    segments: Vec<ItemId>,
}

/// Why a candidate address was rejected. Carries the whole address rather than
/// the offending segment: the segment alone does not tell the user which line
/// of their file to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ItemAddressError {
    candidate: String,
}

impl ItemAddress {
    /// Validate `address` and split it into segments, or report the rejection.
    pub fn new(address: &str) -> Result<Self, ItemAddressError> {
        let segments = address
            .split(SEGMENT_SEPARATOR)
            .map(ItemId::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ItemAddressError {
                candidate: address.to_owned(),
            })?;
        Ok(Self { segments })
    }

    /// The address's segments, in written order.
    #[allow(dead_code, reason = "no command resolves an address yet")]
    pub fn segments(&self) -> &[ItemId] {
        &self.segments
    }

    /// The dotted form, one byte at a time.
    ///
    /// Ordering compares this rather than the segment vector, which is why it
    /// exists: see the [`Ord`] implementation.
    fn text_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.segments
            .iter()
            .enumerate()
            .flat_map(|(index, segment)| {
                (index > 0)
                    .then_some(SEGMENT_SEPARATOR as u8)
                    .into_iter()
                    .chain(segment.as_ref().bytes())
            })
    }
}

impl fmt::Display for ItemAddressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a valid address: every dot-separated segment must be an ID starting \
             with a letter or digit, followed by letters, digits, hyphens, or underscores",
            self.candidate
        )
    }
}

impl std::error::Error for ItemAddressError {}

impl fmt::Display for ItemAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (index, segment) in self.segments.iter().enumerate() {
            if index > 0 {
                f.write_str(".")?;
            }
            f.write_str(segment.as_ref())?;
        }
        Ok(())
    }
}

/// Ordering is over the dotted text, not the segment list.
///
/// `disabled.toml` holds a `BTreeSet` of addresses and the set's order is
/// visible in the written document, so the order has to be the one the previous
/// `BTreeSet<String>` produced. The two disagree: textually `a-c < a.b`, because
/// `-` sorts before `.`, while comparing segment lists puts `a.b` first.
impl Ord for ItemAddress {
    fn cmp(&self, other: &Self) -> Ordering {
        self.text_bytes().cmp(other.text_bytes())
    }
}

impl PartialOrd for ItemAddress {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl TryFrom<&str> for ItemAddress {
    type Error = ItemAddressError;

    fn try_from(address: &str) -> Result<Self, Self::Error> {
        Self::new(address)
    }
}

impl TryFrom<String> for ItemAddress {
    type Error = ItemAddressError;

    fn try_from(address: String) -> Result<Self, Self::Error> {
        Self::new(&address)
    }
}

impl Serialize for ItemAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // One dotted string, not a segment array: this is the `disabled.toml`
        // schema, and it is what a hand-editing user wrote.
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::var::VarName;
    use std::collections::{BTreeMap, BTreeSet};

    fn id(id: &str) -> ItemId {
        ItemId::new(id).expect("valid id")
    }

    fn address(address: &str) -> ItemAddress {
        ItemAddress::new(address).expect("valid address")
    }

    /// The types only ever appear as a field or a key, and a bare value is not a
    /// TOML document, so the serde tests wrap them in one.
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Wrapper<T> {
        value: T,
    }

    #[test]
    fn an_id_starts_with_a_letter_or_digit() {
        assert!(ItemId::new("p10k").is_ok());
        assert!(ItemId::new("9front").is_ok());
        assert!(ItemId::new("oh-my-zsh").is_ok());
        assert!(ItemId::new("a").is_ok());
        assert!(ItemId::new("-leading").is_err());
        assert!(ItemId::new("_leading").is_err());
    }

    #[test]
    fn an_id_excludes_the_address_and_list_separators() {
        assert!(ItemId::new("has.dot").is_err());
        assert!(ItemId::new("has,comma").is_err());
        assert!(ItemId::new("has space").is_err());
        assert!(ItemId::new("").is_err());
    }

    #[test]
    fn the_id_and_variable_name_rules_do_not_collapse() {
        // Each rule accepts something the other rejects, so neither validator can
        // stand in for the other. This is the regression test for that.
        assert!(VarName::new("_hidden").is_ok());
        assert!(ItemId::new("_hidden").is_err());

        for id in ["oh-my-zsh", "env", "9front"] {
            assert!(ItemId::new(id).is_ok(), "{id} should be a valid ID");
            assert!(
                VarName::new(id).is_err(),
                "{id} should not be a valid variable name"
            );
        }
    }

    #[test]
    fn display_and_as_ref_return_the_id() {
        let id = id("p10k");
        assert_eq!(id.to_string(), "p10k");
        assert_eq!(id.as_ref(), "p10k");
    }

    #[test]
    fn an_owned_id_follows_the_same_rule() {
        assert_eq!(ItemId::try_from("p10k".to_owned()), Ok(id("p10k")));
        assert!(ItemId::try_from("has.dot".to_owned()).is_err());
    }

    #[test]
    fn a_rejected_id_names_the_value_and_the_rule() {
        // This reaches the user through serde, which renders the error as-is.
        let error = ItemId::new("my remote").expect_err("spaces are not allowed");
        assert!(error.to_string().contains("`my remote`"), "{error}");
        assert!(
            error
                .to_string()
                .contains("must start with a letter or digit"),
            "{error}"
        );
    }

    #[test]
    fn an_id_round_trips_as_a_value_and_as_a_key() {
        let document = toml::to_string(&Wrapper { value: id("p10k") }).expect("serialize");
        assert_eq!(document, "value = \"p10k\"\n");
        assert_eq!(
            toml::from_str::<Wrapper<ItemId>>(&document).expect("deserialize"),
            Wrapper { value: id("p10k") }
        );

        let keyed = toml::to_string(&BTreeMap::from([(id("oh-my-zsh"), "x")])).expect("serialize");
        assert_eq!(keyed, "oh-my-zsh = \"x\"\n");
        assert_eq!(
            toml::from_str::<BTreeMap<ItemId, String>>(&keyed).expect("deserialize"),
            BTreeMap::from([(id("oh-my-zsh"), "x".to_owned())])
        );
    }

    #[test]
    fn an_invalid_id_fails_the_document_that_contains_it() {
        let error = toml::from_str::<Wrapper<ItemId>>("value = '_hidden'\n")
            .expect_err("an invalid ID should not deserialize");
        assert!(error.to_string().contains("`_hidden`"), "{error}");

        let error = toml::from_str::<BTreeMap<ItemId, String>>("'my remote' = 'x'\n")
            .expect_err("an invalid key should not deserialize");
        assert!(error.to_string().contains("`my remote`"), "{error}");
    }

    #[test]
    fn an_address_is_any_positive_number_of_id_segments() {
        // Three is the deepest form that currently resolves, but arity is a
        // lookup concern rather than a syntax one.
        for candidate in ["p10k", "core.zshrc", "core.zsh-plugins.p10k", "a.b.c.d.e"] {
            let parsed = ItemAddress::new(candidate).expect("valid address");
            assert_eq!(parsed.to_string(), candidate);
        }

        assert_eq!(address("core.zshrc").segments(), [id("core"), id("zshrc")]);
    }

    #[test]
    fn an_empty_segment_is_not_an_address() {
        for malformed in ["a..b", ".a", "a.", "", "."] {
            let error = ItemAddress::new(malformed).expect_err("empty segments are invalid");
            assert_eq!(
                error,
                ItemAddressError {
                    candidate: malformed.to_owned()
                }
            );
        }
    }

    #[test]
    fn a_rejected_address_names_the_whole_address() {
        // The offending segment alone would not say which line to fix.
        let error = ItemAddress::new("core._hidden").expect_err("invalid segment");
        assert!(error.to_string().contains("`core._hidden`"), "{error}");
    }

    #[test]
    fn an_owned_address_follows_the_same_rule() {
        assert_eq!(
            ItemAddress::try_from("core.zshrc".to_owned()),
            Ok(address("core.zshrc"))
        );
        assert!(ItemAddress::try_from("a..b".to_owned()).is_err());
    }

    #[test]
    fn an_address_serializes_as_one_dotted_string() {
        let document = toml::to_string(&Wrapper {
            value: address("core.zshrc"),
        })
        .expect("serialize");
        assert_eq!(document, "value = \"core.zshrc\"\n");
        assert_eq!(
            toml::from_str::<Wrapper<ItemAddress>>(&document).expect("deserialize"),
            Wrapper {
                value: address("core.zshrc")
            }
        );
    }

    #[test]
    fn a_set_of_addresses_round_trips() {
        let set = BTreeSet::from([address("p10k"), address("core.zshrc")]);
        let document = toml::to_string(&Wrapper { value: &set }).expect("serialize");
        assert_eq!(document, "value = [\"core.zshrc\", \"p10k\"]\n");
        assert_eq!(
            toml::from_str::<Wrapper<BTreeSet<ItemAddress>>>(&document).expect("deserialize"),
            Wrapper { value: set }
        );
    }

    #[test]
    fn an_invalid_address_fails_the_document_that_contains_it() {
        let error = toml::from_str::<Wrapper<ItemAddress>>("value = 'a..b'\n")
            .expect_err("an invalid address should not deserialize");
        assert!(error.to_string().contains("`a..b`"), "{error}");
    }

    #[test]
    fn addresses_sort_by_their_dotted_text() {
        // The old representation was a `BTreeSet<String>` and its order is
        // visible in `disabled.toml`, so `-` still sorts before `.`. Ordering the
        // segment lists instead would swap these two.
        let set = BTreeSet::from([address("a.b"), address("a-c")]);
        assert_eq!(
            toml::to_string(&Wrapper { value: &set }).expect("serialize"),
            "value = [\"a-c\", \"a.b\"]\n"
        );

        let mut sorted: Vec<_> = ["b", "a.b", "a", "a-c", "a.a"].map(address).to_vec();
        sorted.sort();
        let mut textual = ["b", "a.b", "a", "a-c", "a.a"].map(str::to_owned).to_vec();
        textual.sort();
        assert_eq!(
            sorted
                .iter()
                .map(ItemAddress::to_string)
                .collect::<Vec<_>>(),
            textual
        );
    }
}
