//! User-variable names.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A validated user-variable name.
///
/// Names match `[A-Za-z_][A-Za-z0-9_]*` and cannot be one of the reserved
/// identifiers in [`RESERVED`]. Holding one is proof the name has been checked,
/// so a name is validated where it enters and nowhere after.
///
/// This is **not** the rule [`ItemId`](crate::item::ItemId) enforces, and the
/// difference is deliberate: `_hidden` is a name and not an ID, while `9front`,
/// `oh-my-zsh`, and `env` are IDs and not names. Neither validator stands in for
/// the other.
///
/// Deserializing goes through the same check, which is what makes it the key
/// type of the `[vars]` map: an invalid name fails the document that holds it,
/// and the TOML error underlines the offending key.
///
/// Serialization writes the name back as the bare string, so a name that was
/// read as a map key is written as one.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(try_from = "String")]
pub(crate) struct VarName(String);

/// Why a candidate user-variable name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum VarNameError {
    #[error(
        "a variable name must start with a letter or underscore, \
         followed by letters, digits, or underscores"
    )]
    Invalid,

    #[error("`facts`, `env`, `vars`, `true`, and `false` are reserved by the expression language")]
    Reserved,
}

/// Identifiers the expression language and its batfiles bindings own, so none of
/// them can name a user variable.
///
/// Reserving all five is what will make condition evaluation's namespace
/// dispatch unambiguous by construction: no user variable can shadow `facts`,
/// `env`, or `vars`, so the resolver arriving at 5.5 needs no precedence rule.
const RESERVED: [&str; 5] = ["facts", "env", "vars", "true", "false"];

/// The rule itself.
fn validate(name: &str) -> Result<(), VarNameError> {
    let mut rest = name.chars();
    match rest.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return Err(VarNameError::Invalid),
    }
    if !rest.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(VarNameError::Invalid);
    }
    if RESERVED.contains(&name) {
        return Err(VarNameError::Reserved);
    }
    Ok(())
}

impl TryFrom<String> for VarName {
    type Error = VarNameError;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        validate(&name)?;
        Ok(Self(name))
    }
}

impl fmt::Display for VarName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn checked(name: &str) -> Result<VarName, VarNameError> {
        VarName::try_from(name.to_owned())
    }

    #[test]
    fn a_name_starts_with_a_letter_or_an_underscore() {
        assert!(checked("editor").is_ok());
        assert!(checked("_hidden").is_ok());
        assert_eq!(checked("1up"), Err(VarNameError::Invalid));
        assert_eq!(checked("-x"), Err(VarNameError::Invalid));
    }

    #[test]
    fn later_characters_are_letters_digits_or_underscores() {
        assert!(checked("EDITOR_2").is_ok());
        // The hyphen is the one to know about: it is ordinary in an ID and in
        // the manifest's own field names, and invalid here.
        assert_eq!(checked("has-dash"), Err(VarNameError::Invalid));
        assert_eq!(checked("has.dot"), Err(VarNameError::Invalid));
        assert_eq!(checked("has space"), Err(VarNameError::Invalid));
    }

    #[test]
    fn an_empty_name_is_invalid() {
        assert_eq!(checked(""), Err(VarNameError::Invalid));
    }

    #[test]
    fn the_expression_keywords_are_reserved() {
        // Listed literally rather than read back from `RESERVED`, so trimming
        // the constant fails here — which is what keeps 5.5's namespace
        // dispatch unshadowable.
        for reserved in ["facts", "env", "vars", "true", "false"] {
            assert_eq!(
                checked(reserved),
                Err(VarNameError::Reserved),
                "`{reserved}` should be reserved"
            );
        }
    }

    #[test]
    fn a_name_is_case_sensitive() {
        // `Editor` and `editor` are two variables, here as in the map they key.
        assert_ne!(
            checked("Editor").expect("valid"),
            checked("editor").expect("valid")
        );
    }

    #[test]
    fn a_rejection_explains_the_rule_it_broke() {
        // These reach the user through serde, which renders the error as
        // written, so each one has to read on its own.
        assert!(
            VarNameError::Invalid
                .to_string()
                .starts_with("a variable name must start with")
        );
        assert!(VarNameError::Reserved.to_string().contains("`facts`"));
    }

    #[test]
    fn a_name_deserializes_as_the_key_of_a_map() {
        let map: BTreeMap<VarName, String> =
            toml::from_str("work = \"true\"\nprofile = \"personal\"\n").expect("deserialize");
        assert_eq!(
            map.keys().map(VarName::to_string).collect::<Vec<_>>(),
            ["profile", "work"]
        );
    }

    #[test]
    fn an_invalid_key_fails_the_document_that_holds_it() {
        let error = toml::from_str::<BTreeMap<VarName, String>>("1up = \"x\"\n")
            .expect_err("an invalid name should not deserialize");
        assert!(
            error
                .to_string()
                .contains("a variable name must start with"),
            "{error}"
        );
    }
}
