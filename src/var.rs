//! User-variable names.

use std::borrow::Borrow;
use std::fmt;

use serde::{Deserialize, Serialize, Serializer};

/// A validated user-variable name.
///
/// Names match `[A-Za-z_][A-Za-z0-9_]*` and cannot be one of the reserved
/// identifiers `facts`, `env`, `vars`, `true`, or `false`, which the expression
/// language and its batfiles bindings own. Holding a `VarName` is proof the name
/// has already been checked, so names are validated once where they enter and
/// the rest of the code never re-checks.
///
/// This is **not** the ID rule that [`ItemId`](crate::item::ItemId) enforces,
/// and the two are deliberately different: `_hidden` is a valid variable name
/// but not a valid ID, while `9front`, `oh-my-zsh`, and `env` are valid IDs but
/// not valid variable names. Neither validator can stand in for the other.
///
/// Deserializing goes through the same check, which is what makes it the key
/// type of the `[vars]` and `vars.toml` maps: a name that reaches the rest of
/// the program from a file has already been rejected if it was invalid, and the
/// TOML error points at the offending key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct VarName(String);

/// Why a candidate user-variable name was rejected. Kept deliberately small;
/// the caller maps either variant to its own diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VarNameError {
    /// Empty, or a character outside `[A-Za-z_][A-Za-z0-9_]*`.
    Invalid,
    /// A reserved identifier (`facts`, `env`, `vars`, `true`, `false`).
    Reserved,
}

/// Identifiers the expression language and its batfiles bindings own, so they
/// cannot name a user variable.
///
/// Reserving all five is what makes condition evaluation's namespace dispatch
/// unambiguous *by construction*: no user variable can shadow `facts`, `env`, or
/// `vars`, so the resolver needs no precedence rule. Trimming this list back
/// would make it shadowable — see [`crate::condition`].
const RESERVED: [&str; 5] = ["facts", "env", "vars", "true", "false"];

impl VarName {
    /// Validate `name` and wrap it, or report why it was rejected.
    pub fn new(name: &str) -> Result<Self, VarNameError> {
        validate(name)?;
        Ok(Self(name.to_owned()))
    }
}

/// The rule itself, shared by both constructors so an owned name is checked
/// without being copied first.
fn validate(name: &str) -> Result<(), VarNameError> {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return Err(VarNameError::Invalid),
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(VarNameError::Invalid);
    }
    if RESERVED.contains(&name) {
        return Err(VarNameError::Reserved);
    }
    Ok(())
}

impl fmt::Display for VarNameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => f.write_str(
                "a variable name must start with a letter or underscore, \
                 followed by letters, digits, or underscores",
            ),
            Self::Reserved => f.write_str(
                "`facts`, `env`, `vars`, `true`, and `false` are reserved by the expression \
                 language",
            ),
        }
    }
}

impl std::error::Error for VarNameError {}

impl fmt::Display for VarName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for VarName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Lets a map keyed by `VarName` be looked up with the `&str` an expression
/// identifier is, without allocating a `VarName` per lookup — which condition
/// evaluation would otherwise do once per identifier per condition.
///
/// The derived `Ord` compares the inner `String`, so it agrees with `str`'s,
/// which is the consistency `Borrow` requires and which a test asserts rather
/// than leaving to a reader.
impl Borrow<str> for VarName {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for VarName {
    type Error = VarNameError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        validate(&name)?;
        Ok(Self(name))
    }
}

impl Serialize for VarName {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn a_leading_letter_or_underscore_is_required() {
        assert!(VarName::new("editor").is_ok());
        assert!(VarName::new("_hidden").is_ok());
        assert_eq!(VarName::new("1up"), Err(VarNameError::Invalid));
        assert_eq!(VarName::new("-x"), Err(VarNameError::Invalid));
    }

    #[test]
    fn later_characters_allow_letters_digits_and_underscores_only() {
        assert!(VarName::new("EDITOR_2").is_ok());
        assert_eq!(VarName::new("has-dash"), Err(VarNameError::Invalid));
        assert_eq!(VarName::new("has.dot"), Err(VarNameError::Invalid));
        assert_eq!(VarName::new("has space"), Err(VarNameError::Invalid));
    }

    #[test]
    fn an_empty_name_is_invalid() {
        assert_eq!(VarName::new(""), Err(VarNameError::Invalid));
    }

    #[test]
    fn the_expression_keywords_are_reserved() {
        // Listed literally rather than read from `RESERVED`, so trimming the
        // constant fails here — which is what keeps `crate::condition`'s
        // namespace dispatch unshadowable.
        for reserved in ["facts", "env", "vars", "true", "false"] {
            assert_eq!(
                VarName::new(reserved),
                Err(VarNameError::Reserved),
                "{reserved} should be reserved"
            );
        }
    }

    #[test]
    fn display_and_as_ref_return_the_name() {
        let name = VarName::new("profile").expect("valid");
        assert_eq!(name.to_string(), "profile");
        assert_eq!(name.as_ref(), "profile");
    }

    #[test]
    fn an_owned_name_follows_the_same_rule() {
        assert_eq!(
            VarName::try_from("profile".to_owned()),
            Ok(VarName::new("profile").expect("valid"))
        );
        assert_eq!(
            VarName::try_from("has.dot".to_owned()),
            Err(VarNameError::Invalid)
        );
    }

    #[test]
    fn a_rejection_explains_the_rule_it_broke() {
        // These reach the user through serde, which renders the error as-is, so
        // each one has to read on its own.
        assert!(
            VarNameError::Invalid
                .to_string()
                .starts_with("a variable name must start with")
        );
        assert!(VarNameError::Reserved.to_string().contains("`facts`"));
    }

    #[test]
    fn a_borrowed_name_orders_the_way_the_owned_one_does() {
        // The consistency `Borrow` requires: a map keyed by `VarName` and
        // searched by `&str` finds what the owned key would.
        let names = ["Editor", "_hidden", "editor", "editor2", "profile"];
        let map: BTreeMap<VarName, usize> = names
            .iter()
            .enumerate()
            .map(|(index, text)| (VarName::new(text).expect("valid"), index))
            .collect();

        for (index, text) in names.iter().enumerate() {
            assert_eq!(
                map.get(*text),
                Some(&index),
                "{text} was not found by `str`"
            );
        }
        assert_eq!(map.get("missing"), None);
        assert_eq!(
            map.keys().map(VarName::as_ref).collect::<Vec<_>>(),
            names,
            "the derived `Ord` does not agree with `str`'s"
        );
    }

    #[test]
    fn a_name_round_trips_through_serde() {
        let name = VarName::new("editor").expect("valid");
        let document = toml::to_string(&BTreeMap::from([(&name, "nvim")])).expect("serialize");
        assert_eq!(document, "editor = \"nvim\"\n");
        assert_eq!(
            toml::from_str::<BTreeMap<VarName, String>>(&document).expect("deserialize"),
            BTreeMap::from([(name, "nvim".to_owned())])
        );
    }

    #[test]
    fn an_invalid_key_fails_the_document_that_contains_it() {
        let error = toml::from_str::<BTreeMap<VarName, String>>("1up = \"x\"\n")
            .expect_err("an invalid name should not deserialize");
        assert!(
            error
                .to_string()
                .contains("a variable name must start with")
        );
    }
}
