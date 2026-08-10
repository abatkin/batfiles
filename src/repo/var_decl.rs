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
    pub command_timeout: Option<FriendlyDuration>,
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
                // The list is required to be non-empty, which the resolver
                // checks; an empty list parses here.
                Vec::deserialize(SeqAccessDeserializer::new(seq)).map(CommandSpec::Args)
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
    fn a_declaration_round_trips() {
        let vars = parse("[email]\ncommand = ['git', 'config', 'user.email']\ncache = '24h'\n")
            .expect("parse");
        let document = toml::to_string(&vars).expect("serialize");
        assert_eq!(parse(&document).expect("reparse"), vars);
    }
}
