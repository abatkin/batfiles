//! `[vars]` values: a static string, or a dynamic-variable declaration.
//!
//! Every variable is a string, so a table is unambiguous: it is a declaration,
//! and it must match that closed record. See
//! [`docs/repoformat.md`](../../docs/repoformat.md#variables).

use std::fmt;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::duration::FriendlyDuration;

/// One entry in `[vars]`.
///
/// Deserialized by hand, choosing the form by TOML type, so a typo inside a
/// declaration is reported as the unknown field it is rather than as a failure
/// to match an untagged union.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VarSpec {
    /// A value written in the manifest.
    Static(String),
    /// A value produced by running a command.
    Dynamic(DynamicVarSpec),
}

impl<'de> Deserialize<'de> for VarSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct VarSpecVisitor;

        impl<'de> Visitor<'de> for VarSpecVisitor {
            type Value = VarSpec;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string value or a dynamic-variable table")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<VarSpec, E> {
                Ok(VarSpec::Static(value.to_owned()))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<VarSpec, A::Error> {
                DynamicVarSpec::deserialize(MapAccessDeserializer::new(map)).map(VarSpec::Dynamic)
            }
        }

        deserializer.deserialize_any(VarSpecVisitor)
    }
}

/// A dynamic variable: a command, and how its result is captured and cached.
///
/// `cache` and `command-timeout` stay optional; the resolver and the runner
/// apply their documented defaults.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct DynamicVarSpec {
    pub command: CommandSpec,
    #[serde(default)]
    pub capture: CaptureMode,
    pub cache: Option<FriendlyDuration>,
    /// Never zero: rejected as the document is read.
    #[serde(default, deserialize_with = "positive_duration")]
    pub command_timeout: Option<FriendlyDuration>,
}

/// Accept a `command-timeout`, rejecting zero. `cache = "0s"` stays valid,
/// so the rule belongs to this field rather than to [`FriendlyDuration`].
fn positive_duration<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<FriendlyDuration>, D::Error> {
    let timeout = Option::<FriendlyDuration>::deserialize(deserializer)?;
    if timeout.is_some_and(|value| value.get().is_zero()) {
        return Err(de::Error::custom(
            "`command-timeout` must be greater than zero: a zero timeout would kill every \
             command before it could produce a value",
        ));
    }
    Ok(timeout)
}

/// What a dynamic variable's value is taken from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum CaptureMode {
    /// The command's trimmed standard output.
    #[default]
    Stdout,
    /// `"true"` for exit status zero, `"false"` otherwise.
    Status,
}

/// A dynamic variable's command: a shell command line, or an argument vector
/// run without a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandSpec {
    Shell(String),
    /// Never empty: rejected as the document is read.
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

    type Vars = BTreeMap<VarName, VarSpec>;

    fn parse(document: &str) -> Result<Vars, toml::de::Error> {
        toml::from_str(document)
    }

    fn var<'a>(vars: &'a Vars, name: &str) -> &'a VarSpec {
        vars.get(name).expect("declared")
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

        assert_eq!(var(&vars, "work"), &VarSpec::Static("false".to_owned()));
        assert_eq!(
            var(&vars, "email"),
            &VarSpec::Dynamic(DynamicVarSpec {
                command: CommandSpec::Args(vec![
                    "git".to_owned(),
                    "config".to_owned(),
                    "user.email".to_owned()
                ]),
                capture: CaptureMode::Stdout,
                cache: Some(duration("24h")),
                command_timeout: None,
            })
        );
        assert_eq!(
            var(&vars, "has_op"),
            &VarSpec::Dynamic(DynamicVarSpec {
                command: CommandSpec::Shell("command -v op >/dev/null".to_owned()),
                capture: CaptureMode::Status,
                cache: None,
                command_timeout: Some(duration("5s")),
            })
        );
    }

    #[test]
    fn a_non_string_scalar_is_not_a_value() {
        for document in ["rank = 3", "work = true", "ratio = 1.5", "list = ['a']"] {
            let error = parse(document).expect_err("should not parse");
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
    fn a_command_is_a_string_or_a_list_of_strings() {
        let error = parse("[email]\ncommand = 3\n").expect_err("not a command");
        assert!(
            error
                .to_string()
                .contains("a shell command string or a list of arguments"),
            "{error}"
        );
    }

    #[test]
    fn an_empty_command_list_is_rejected_with_the_rule() {
        let error = parse("[email]\ncommand = []\n").expect_err("nothing to run");
        let message = error.to_string();
        assert!(message.contains("has nothing to run"), "{message}");
    }

    #[test]
    fn a_zero_command_timeout_is_rejected_and_a_zero_cache_is_not() {
        let error = parse("[email]\ncommand = 'true'\ncommand-timeout = '0s'\n")
            .expect_err("a zero timeout is not a coherent request");
        assert!(
            error.to_string().contains("must be greater than zero"),
            "{error}"
        );
        assert!(parse("[email]\ncommand = 'true'\ncommand-timeout = '1ms'\n").is_ok());
        assert!(parse("[email]\ncommand = 'true'\ncache = '0s'\n").is_ok());
    }

    #[test]
    fn a_rejected_field_names_its_line() {
        let error = parse("email = { command = [] }\n").expect_err("nothing to run");
        assert!(error.to_string().contains("line 1"), "{error}");
    }
}
