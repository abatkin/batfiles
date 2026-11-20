//! A `run` action's `command` and `trigger`. See [the `run`
//! action](../../docs/actions/run.md).

use std::fmt;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::duration::FriendlyDuration;
use crate::manifest::item::ItemId;
use crate::manifest::template::TemplateString;

/// A `run` action's command: a shell command line, or a program and its arguments run without
/// a shell. Every part is a template.
#[derive(Debug, Clone)]
pub(crate) enum RunCommand {
    Shell(TemplateString),
    /// Never empty: rejected as the document is read.
    Args(Vec<TemplateString>),
}

impl RunCommand {
    /// Every template the command is made of, in order.
    pub fn parts(&self) -> &[TemplateString] {
        match self {
            Self::Shell(line) => std::slice::from_ref(line),
            Self::Args(args) => args,
        }
    }
}

/// The command as written, templates unresolved: a shell line as is, and list elements joined
/// by spaces, each quoted if it is empty or holds whitespace or a quote.
impl fmt::Display for RunCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Shell(line) => f.write_str(line.source()),
            Self::Args(args) => {
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        f.write_str(" ")?;
                    }
                    let text = arg.source();
                    if text.is_empty() || text.contains(|c: char| c.is_whitespace() || c == '\'') {
                        write!(f, "\"{}\"", text.replace('"', "\\\""))?;
                    } else {
                        f.write_str(text)?;
                    }
                }
                Ok(())
            }
        }
    }
}

impl<'de> Deserialize<'de> for RunCommand {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RunCommandVisitor;

        impl<'de> Visitor<'de> for RunCommandVisitor {
            type Value = RunCommand;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a shell command string or a list of arguments")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<RunCommand, E> {
                Ok(RunCommand::Shell(TemplateString::from(value.to_owned())))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<RunCommand, A::Error> {
                let args: Vec<String> = Vec::deserialize(SeqAccessDeserializer::new(seq))?;
                if args.is_empty() {
                    return Err(de::Error::custom(
                        "an empty `command` list has nothing to run: write the program and its \
                         arguments, such as `[\"mise\", \"install\"]`",
                    ));
                }
                Ok(RunCommand::Args(
                    args.into_iter().map(TemplateString::from).collect(),
                ))
            }
        }

        deserializer.deserialize_any(RunCommandVisitor)
    }
}

/// The name of an environment variable a `run` action sets: never empty, and holding neither
/// `=` nor NUL.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct EnvName(String);

impl EnvName {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EnvName {
    type Error = String;

    fn try_from(name: String) -> Result<Self, String> {
        if name.is_empty() || name.contains(['=', '\0']) {
            Err(format!(
                "`{}` is not an environment variable name: a name is not empty and holds \
                 neither `=` nor NUL",
                name.escape_default()
            ))
        } else {
            Ok(Self(name))
        }
    }
}

/// When a `run` action's command runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Trigger {
    /// Every run that admits the action.
    #[default]
    Always,
    /// Only `clone` and `sync --bootstrap`.
    Bootstrap,
    /// Until it first succeeds.
    Once,
    /// When it has never succeeded, or its last success is at least this old.
    Every(FriendlyDuration),
    /// When it has never succeeded, a firing is pending, or one of these changed in this run.
    /// Never empty: rejected as the document is read.
    After(Vec<AfterTarget>),
}

impl Trigger {
    /// Whether the trigger consults and updates `runs.toml`, and so needs a stable address.
    pub fn keeps_state(&self) -> bool {
        !matches!(self, Self::Always | Self::Bootstrap)
    }

    /// How a message names the trigger's kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Bootstrap => "bootstrap",
            Self::Once => "once",
            Self::Every(_) => "every",
            Self::After(_) => "after",
        }
    }
}

impl<'de> Deserialize<'de> for Trigger {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct TriggerVisitor;

        impl<'de> Visitor<'de> for TriggerVisitor {
            type Value = Trigger;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "`\"always\"`, `\"bootstrap\"`, `\"once\"`, `{ every = \"<duration>\" }`, or \
                     `{ after = <targets> }`",
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Trigger, E> {
                match value {
                    "always" => Ok(Trigger::Always),
                    "bootstrap" => Ok(Trigger::Bootstrap),
                    "once" => Ok(Trigger::Once),
                    other => Err(E::custom(format!(
                        "`{other}` is not a trigger; write `\"always\"`, `\"bootstrap\"`, \
                         `\"once\"`, `{{ every = \"24h\" }}`, or `{{ after = \"<id>\" }}`"
                    ))),
                }
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Trigger, A::Error> {
                let table = TriggerTable::deserialize(MapAccessDeserializer::new(map))?;
                match (table.every, table.after) {
                    (Some(every), None) => Ok(Trigger::Every(every)),
                    (None, Some(AfterTargets(targets))) => Ok(Trigger::After(targets)),
                    (Some(_), Some(_)) => Err(de::Error::custom(
                        "a trigger table writes `every` or `after`, not both",
                    )),
                    (None, None) => Err(de::Error::custom(
                        "a trigger table writes `every` or `after`",
                    )),
                }
            }
        }

        deserializer.deserialize_any(TriggerVisitor)
    }
}

/// A trigger written as a table.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TriggerTable {
    every: Option<FriendlyDuration>,
    after: Option<AfterTargets>,
}

/// An `after` value: one target, or a non-empty list of them.
struct AfterTargets(Vec<AfterTarget>);

impl<'de> Deserialize<'de> for AfterTargets {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AfterVisitor;

        impl<'de> Visitor<'de> for AfterVisitor {
            type Value = AfterTargets;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an action ID, `@` and a remote ID, or a list of them")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<AfterTargets, E> {
                AfterTarget::parse(value)
                    .map(|target| AfterTargets(vec![target]))
                    .map_err(E::custom)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<AfterTargets, A::Error> {
                let written: Vec<String> = Vec::deserialize(SeqAccessDeserializer::new(seq))?;
                if written.is_empty() {
                    return Err(de::Error::custom(
                        "an empty `after` list follows nothing: name at least one action or \
                         `@remote`",
                    ));
                }
                written
                    .iter()
                    .map(|text| AfterTarget::parse(text).map_err(de::Error::custom))
                    .collect::<Result<_, _>>()
                    .map(AfterTargets)
            }
        }

        deserializer.deserialize_any(AfterVisitor)
    }
}

/// What an `after` trigger follows.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AfterTarget {
    /// An action declared earlier in the same manifest.
    Action(ItemId),
    /// A remote, resolved as a repository path's `@remote` is.
    Remote(ItemId),
}

impl AfterTarget {
    fn parse(text: &str) -> Result<Self, String> {
        let (remote, id) = match text.strip_prefix('@') {
            Some(id) => (true, id),
            None => (false, text),
        };
        let id = ItemId::try_from(id.to_owned())
            .map_err(|error| format!("`after` target `{text}`: {error}"))?;
        Ok(if remote {
            Self::Remote(id)
        } else {
            Self::Action(id)
        })
    }
}

impl fmt::Display for AfterTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Action(id) => write!(f, "{id}"),
            Self::Remote(id) => write!(f, "@{id}"),
        }
    }
}

#[cfg(test)]
mod tests;
