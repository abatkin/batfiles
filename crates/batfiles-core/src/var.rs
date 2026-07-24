//! User-variable names.

use std::fmt;

/// A validated user-variable name.
///
/// Names match `[A-Za-z_][A-Za-z0-9_]*` and cannot be one of the reserved
/// identifiers `facts`, `env`, `true`, or `false`, per the repository format's
/// shared name rules (`docs/repoformat.md#names-and-ids`). Holding a `VarName`
/// is proof the name has already been checked, so the configuration layer
/// validates once at the boundary and the rest of the code never re-checks.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VarName(String);

/// Why a candidate user-variable name was rejected. Kept deliberately small;
/// the configuration layer maps either variant to its own diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarNameError {
    /// Empty, or a character outside `[A-Za-z_][A-Za-z0-9_]*`.
    Invalid,
    /// A reserved identifier (`facts`, `env`, `true`, `false`).
    Reserved,
}

/// Identifiers the expression language owns, so they cannot name a user
/// variable.
const RESERVED: [&str; 4] = ["facts", "env", "true", "false"];

impl VarName {
    /// Validate `name` and wrap it, or report why it was rejected.
    pub fn new(name: &str) -> Result<Self, VarNameError> {
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
        Ok(Self(name.to_owned()))
    }
}

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

impl TryFrom<&str> for VarName {
    type Error = VarNameError;

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        Self::new(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for reserved in ["facts", "env", "true", "false"] {
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
}
