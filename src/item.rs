//! Item IDs: the names by which the things in a repository are addressed.

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

impl fmt::Display for ItemId {
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
}
