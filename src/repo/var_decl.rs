//! The `[vars]` map: static values and dynamic declarations.
//!
//! Every variable batfiles exposes is a string, so a static value is a TOML
//! string and nothing else. A table is therefore unambiguous: it is a
//! dynamic-variable declaration, and it must match that closed record.

use std::fmt;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::repo::duration::FriendlyDuration;

/// One entry in `[vars]`.
///
/// Hand-written for the same reason as the shared value shapes: a typo inside a
/// dynamic declaration should be reported as the unknown field it is, not as a
/// failure to match an untagged union.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(crate) enum VarDecl {
    /// A static repository value.
    Static(String),
    /// A value produced by running a command.
    Dynamic(DynamicVar),
}

impl<'de> Deserialize<'de> for VarDecl {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct VarDeclVisitor;

        impl<'de> Visitor<'de> for VarDeclVisitor {
            type Value = VarDecl;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string value or a dynamic-variable table")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<VarDecl, E> {
                Ok(VarDecl::Static(value.to_owned()))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<VarDecl, A::Error> {
                DynamicVar::deserialize(MapAccessDeserializer::new(map)).map(VarDecl::Dynamic)
            }
        }

        deserializer.deserialize_any(VarDeclVisitor)
    }
}

/// A dynamic variable: a command, and how to capture and cache its result.
///
/// `cache` and `command-timeout` stay optional rather than deserializing to
/// their documented `1d` and `5s` defaults, so a declaration that omits them
/// remains distinguishable from one that spells them out. The resolver applies
/// the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct DynamicVar {
    pub command: CommandSpec,
    #[serde(default)]
    pub capture: Capture,
    pub cache: Option<FriendlyDuration>,
    #[serde(default, deserialize_with = "positive_duration")]
    pub command_timeout: Option<FriendlyDuration>,
}

/// Accept a `command-timeout`, rejecting zero.
///
/// `command-timeout = "0s"` asks for a command that is guaranteed to fail, which
/// is a mistake to report with a file and a line rather than a semantic to
/// implement. The asymmetry with `cache = "0s"` — which is coherent, meaning
/// "never fresh", and stays valid — belongs to the two fields rather than to
/// [`FriendlyDuration`], so the type keeps accepting zero and the check lives
/// here. What the runner gets in exchange is a duration it knows is strictly
/// positive.
fn positive_duration<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<FriendlyDuration>, D::Error> {
    let timeout = Option::<FriendlyDuration>::deserialize(deserializer)?;
    if timeout.is_some_and(|value| value.as_signed().is_zero()) {
        return Err(de::Error::custom(
            "`command-timeout` must be greater than zero: a zero timeout would kill every \
             command before it could produce a value",
        ));
    }
    Ok(timeout)
}

/// What a dynamic variable's value is taken from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Capture {
    /// The command's trimmed standard output.
    #[default]
    Stdout,
    /// `"true"` for exit status zero, `"false"` otherwise.
    Status,
}

/// A dynamic variable's command: a shell command line, or a direct argument
/// vector that skips the shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(crate) enum CommandSpec {
    Shell(String),
    Args(Vec<String>),
}

impl<'de> Deserialize<'de> for CommandSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CommandSpecVisitor;

        impl<'de> Visitor<'de> for CommandSpecVisitor {
            type Value = CommandSpec;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a shell command string or a list of arguments")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<CommandSpec, E> {
                Ok(CommandSpec::Shell(value.to_owned()))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<CommandSpec, A::Error> {
                let args: Vec<String> = Vec::deserialize(SeqAccessDeserializer::new(seq))?;
                // Rejected here rather than by whatever runs the command, so the
                // diagnostic carries a file and a line. `docs/repoformat.md`
                // already documents the field as a non-empty list, so this
                // enforces the format rather than narrowing it.
                if args.is_empty() {
                    return Err(de::Error::custom(
                        "an empty `command` list has nothing to run: write the program and its \
                         arguments, such as `[\"git\", \"config\", \"user.email\"]`",
                    ));
                }
                Ok(CommandSpec::Args(args))
            }
        }

        deserializer.deserialize_any(CommandSpecVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::var::VarName;
    use std::collections::BTreeMap;

    type Vars = BTreeMap<VarName, VarDecl>;

    fn parse(document: &str) -> Result<Vars, toml::de::Error> {
        toml::from_str(document)
    }

    fn var<'a>(vars: &'a Vars, name: &str) -> &'a VarDecl {
        vars.get(&VarName::new(name).expect("valid name"))
            .expect("declared")
    }

    fn duration(text: &str) -> FriendlyDuration {
        FriendlyDuration::new(text).expect("valid duration")
    }

    #[test]
    fn a_string_is_a_static_value_and_a_table_is_a_declaration() {
        let vars = parse(
            r#"
work = "false"
email = { command = ["git", "config", "user.email"], cache = "24h" }

[has_op]
command = "command -v op >/dev/null"
capture = "status"
command-timeout = "5s"
"#,
        )
        .expect("both spellings should parse");

        assert_eq!(var(&vars, "work"), &VarDecl::Static("false".to_owned()));
        assert_eq!(
            var(&vars, "email"),
            &VarDecl::Dynamic(DynamicVar {
                command: CommandSpec::Args(vec![
                    "git".to_owned(),
                    "config".to_owned(),
                    "user.email".to_owned()
                ]),
                capture: Capture::Stdout,
                cache: Some(duration("24h")),
                command_timeout: None,
            })
        );
        assert_eq!(
            var(&vars, "has_op"),
            &VarDecl::Dynamic(DynamicVar {
                command: CommandSpec::Shell("command -v op >/dev/null".to_owned()),
                capture: Capture::Status,
                cache: None,
                command_timeout: Some(duration("5s")),
            })
        );
    }

    #[test]
    fn a_non_string_scalar_is_not_a_value() {
        // Batfiles infers no types, so `rank = 3` is a mistake rather than the
        // string "3".
        for document in ["rank = 3", "work = true", "ratio = 1.5"] {
            let error = parse(document).expect_err("{document} should not parse");
            assert!(
                error
                    .to_string()
                    .contains("expected a string value or a dynamic-variable table"),
                "{error}"
            );
        }
    }

    #[test]
    fn a_declaration_needs_a_command() {
        let error = parse("[email]\ncache = '1h'\n").expect_err("command is required");
        assert!(
            error.to_string().contains("missing field `command`"),
            "{error}"
        );
    }

    #[test]
    fn an_unknown_declaration_field_is_reported_by_name() {
        let error = parse("[email]\ncommand = 'true'\nchache = '1h'\n").expect_err("typo");
        assert!(
            error.to_string().contains("unknown field `chache`"),
            "{error}"
        );
    }

    #[test]
    fn capture_accepts_only_its_two_spellings() {
        let error =
            parse("[email]\ncommand = 'true'\ncapture = 'stderr'\n").expect_err("no stderr");
        assert!(
            error.to_string().contains("unknown variant `stderr`"),
            "{error}"
        );
    }

    #[test]
    fn an_invalid_variable_name_fails_the_map() {
        let error = parse("has-dash = 'x'\n").expect_err("dashes are not variable names");
        assert!(
            error.to_string().contains("a variable name must"),
            "{error}"
        );
    }

    #[test]
    fn an_empty_command_list_is_rejected_with_the_rule() {
        let error = parse("[email]\ncommand = []\n").expect_err("nothing to run");
        let message = error.to_string();
        assert!(message.contains("has nothing to run"), "{message}");
        assert!(
            message.contains("write the program and its arguments"),
            "{message}"
        );
    }

    #[test]
    fn a_zero_command_timeout_is_rejected_and_a_tiny_one_is_not() {
        // Deliberately not symmetric with `cache = "0s"`, which stays valid: a
        // zero cache means "never fresh", a zero timeout means "never runs".
        let error = parse("[email]\ncommand = 'true'\ncommand-timeout = '0s'\n")
            .expect_err("a zero timeout is not a coherent request");
        let message = error.to_string();
        assert!(message.contains("must be greater than zero"), "{message}");
        assert!(
            parse("[email]\ncommand = 'true'\ncommand-timeout = '1ms'\n").is_ok(),
            "a very short timeout is still a timeout"
        );
        assert!(
            parse("[email]\ncommand = 'true'\ncache = '0s'\n").is_ok(),
            "`cache = \"0s\"` means never fresh, which is a coherent thing to ask for"
        );
    }

    #[test]
    fn a_rejected_field_fails_the_document_that_contains_it() {
        // The point of checking at deserialize time: the diagnostic carries a
        // position, the way `an_invalid_duration_fails_the_document_that_contains_it`
        // does in `duration.rs`.
        let error = parse("email = { command = [] }\n").expect_err("nothing to run");
        assert!(error.to_string().contains("line 1"), "{error}");
    }

    #[test]
    fn a_declaration_round_trips() {
        let vars = parse("[email]\ncommand = ['git', 'config', 'user.email']\ncache = '24h'\n")
            .expect("parse");
        let document = toml::to_string(&vars).expect("serialize");
        assert_eq!(parse(&document).expect("reparse"), vars);
    }
}
