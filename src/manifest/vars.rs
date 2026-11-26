//! Static strings and dynamic-variable declarations in `[vars]`. See [variable
//! syntax](../../docs/repoformat.md#variables).

use std::fmt;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::duration::FriendlyDuration;
use super::template::TemplateString;

/// A static string or dynamic-variable declaration from `[vars]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VarSpec {
    /// A value written in the manifest: a template, evaluated when the variable is first read.
    Static(TemplateString),
    /// A value captured by running a command or looking up a release.
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
                Ok(VarSpec::Static(TemplateString::from(value.to_owned())))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<VarSpec, A::Error> {
                let fields = DeclarationFields::deserialize(MapAccessDeserializer::new(map))?;
                fields
                    .into_spec()
                    .map(VarSpec::Dynamic)
                    .map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_any(VarSpecVisitor)
    }
}

/// A dynamic variable: what captures its value, and how long a captured value is reused.
///
/// `cache` stays optional; the resolver applies its documented default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DynamicVarSpec {
    pub source: DynamicSource,
    pub cache: Option<FriendlyDuration>,
}

/// What captures a dynamic variable's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DynamicSource {
    /// A command, run in the declaring repository.
    Command(CommandSource),
    /// The tag of a GitHub repository's latest release.
    GithubRelease(GithubReleaseSource),
}

/// A command declaration's fields. `command-timeout` stays optional; the runner applies its
/// documented default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandSource {
    pub command: CommandSpec,
    pub capture: CaptureMode,
    /// Never zero: rejected as the document is read.
    pub command_timeout: Option<FriendlyDuration>,
}

/// A `github-release` declaration's fields. `timeout` stays optional; the lookup applies its
/// documented default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GithubReleaseSource {
    pub repository: GithubRepository,
    /// The host to ask, if not github.com. Checked by manifest validation, which also refuses
    /// one reading anything but `env` and `facts`.
    pub github_url: Option<TemplateString>,
    /// Never zero: rejected as the document is read.
    pub timeout: Option<FriendlyDuration>,
}

/// Every field a dynamic-variable table may hold, before its kind is decided.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct DeclarationFields {
    command: Option<CommandSpec>,
    capture: Option<CaptureMode>,
    command_timeout: Option<FriendlyDuration>,
    github_release: Option<GithubRepository>,
    github_url: Option<TemplateString>,
    timeout: Option<FriendlyDuration>,
    cache: Option<FriendlyDuration>,
}

impl DeclarationFields {
    /// Decide the declaration's kind, refusing a table with no source or fields of both kinds.
    fn into_spec(self) -> Result<DynamicVarSpec, String> {
        let source = match (self.command, self.github_release) {
            (None, None) => {
                return Err(
                    "a dynamic-variable table needs `command` or `github-release`".to_owned(),
                );
            }
            (Some(command), None) => {
                refuse_beside(
                    "command",
                    [
                        ("github-url", self.github_url.is_some()),
                        ("timeout", self.timeout.is_some()),
                    ],
                )?;
                DynamicSource::Command(CommandSource {
                    command,
                    capture: self.capture.unwrap_or_default(),
                    command_timeout: positive("command-timeout", self.command_timeout)?,
                })
            }
            (command, Some(repository)) => {
                refuse_beside(
                    "github-release",
                    [
                        ("command", command.is_some()),
                        ("capture", self.capture.is_some()),
                        ("command-timeout", self.command_timeout.is_some()),
                    ],
                )?;
                DynamicSource::GithubRelease(GithubReleaseSource {
                    repository,
                    github_url: self.github_url,
                    timeout: positive("timeout", self.timeout)?,
                })
            }
        };
        Ok(DynamicVarSpec {
            source,
            cache: self.cache,
        })
    }
}

/// Refuse the first of `others` that is present beside the source field `source`.
fn refuse_beside<const N: usize>(source: &str, others: [(&str, bool); N]) -> Result<(), String> {
    match others.into_iter().find(|(_, present)| *present) {
        Some((other, _)) => Err(format!(
            "`{other}` cannot be written beside `{source}`: a dynamic variable either runs a \
             `command` or looks up a `github-release`, and each takes only its own fields"
        )),
        None => Ok(()),
    }
}

/// An optional timeout, refused at zero.
fn positive(
    field: &str,
    timeout: Option<FriendlyDuration>,
) -> Result<Option<FriendlyDuration>, String> {
    if timeout.is_some_and(|value| value.get().is_zero()) {
        return Err(format!(
            "`{field}` must be greater than zero: a zero timeout would stop every capture \
             before it could produce a value"
        ));
    }
    Ok(timeout)
}

/// A GitHub repository as `OWNER/REPO`, validated as GitHub names them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct GithubRepository(String);

impl GithubRepository {
    /// The repository exactly as written.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for GithubRepository {
    type Error = String;

    fn try_from(value: String) -> Result<Self, String> {
        let owner_char = |c: char| c.is_ascii_alphanumeric() || c == '-';
        let repo_char = |c: char| owner_char(c) || c == '_' || c == '.';
        let valid = value.split_once('/').is_some_and(|(owner, repo)| {
            !owner.is_empty()
                && owner.chars().all(owner_char)
                && !repo.is_empty()
                && repo.chars().all(repo_char)
                && repo != "."
                && repo != ".."
                && !repo.ends_with(".git")
        });
        if valid {
            Ok(Self(value))
        } else {
            Err(format!(
                "`github-release` names a repository as `OWNER/REPO`, such as \
                 `junegunn/fzf`: an owner of ASCII letters, digits, and `-`, and a repository \
                 of those, `_`, and `.`, with no scheme, host, or `.git` suffix; `{}` is not one",
                value.escape_default()
            ))
        }
    }
}

impl fmt::Display for GithubRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What a dynamic variable's value is taken from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
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

/// Written as it was declared: a string, or a list of arguments.
impl Serialize for CommandSpec {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Shell(line) => serializer.serialize_str(line),
            Self::Args(args) => args.serialize(serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vars::name::VarName;
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

        assert_eq!(
            var(&vars, "work"),
            &VarSpec::Static(TemplateString::from("false".to_owned()))
        );
        assert_eq!(
            var(&vars, "email"),
            &VarSpec::Dynamic(DynamicVarSpec {
                source: DynamicSource::Command(CommandSource {
                    command: CommandSpec::Args(vec![
                        "git".to_owned(),
                        "config".to_owned(),
                        "user.email".to_owned()
                    ]),
                    capture: CaptureMode::Stdout,
                    command_timeout: None,
                }),
                cache: Some(duration("24h")),
            })
        );
        assert_eq!(
            var(&vars, "has_op"),
            &VarSpec::Dynamic(DynamicVarSpec {
                source: DynamicSource::Command(CommandSource {
                    command: CommandSpec::Shell("command -v op >/dev/null".to_owned()),
                    capture: CaptureMode::Status,
                    command_timeout: Some(duration("5s")),
                }),
                cache: None,
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
    fn a_declaration_needs_a_source() {
        let error = parse("[email]\ncache = '1h'\n").expect_err("a source is required");
        assert!(
            error
                .to_string()
                .contains("needs `command` or `github-release`"),
            "{error}"
        );
    }

    #[test]
    fn a_release_declaration_names_a_repository_and_its_host() {
        let vars = parse(
            r#"
[fzf]
github-release = "junegunn/fzf"
github-url = "https://ghe.example/"
timeout = "2s"
cache = "1h"
"#,
        )
        .expect("a release declaration");
        let VarSpec::Dynamic(DynamicVarSpec {
            source: DynamicSource::GithubRelease(release),
            cache,
        }) = var(&vars, "fzf")
        else {
            panic!("not a release declaration");
        };
        assert_eq!(release.repository.as_str(), "junegunn/fzf");
        assert_eq!(
            release.github_url.as_ref().map(TemplateString::source),
            Some("https://ghe.example/")
        );
        assert_eq!(release.timeout, Some(duration("2s")));
        assert_eq!(*cache, Some(duration("1h")));
    }

    #[test]
    fn a_declaration_is_exactly_one_kind() {
        for (document, field) in [
            ("github-release = 'a/b'\ncommand = 'true'", "command"),
            ("github-release = 'a/b'\ncapture = 'status'", "capture"),
            (
                "github-release = 'a/b'\ncommand-timeout = '1s'",
                "command-timeout",
            ),
            (
                "command = 'true'\ngithub-url = 'https://e.example'",
                "github-url",
            ),
            ("command = 'true'\ntimeout = '1s'", "timeout"),
        ] {
            let error = parse(&format!("[v]\n{document}\n")).expect_err("mixed kinds");
            assert!(
                error
                    .to_string()
                    .contains(&format!("`{field}` cannot be written beside")),
                "{error}"
            );
        }
    }

    #[test]
    fn a_repository_is_owner_slash_repo_as_github_spells_them() {
        let error = parse("[v]\ngithub-release = \"a/\u{e9}\"\n").expect_err("not ASCII");
        assert!(
            error.to_string().contains("`a/\\u{e9}` is not one"),
            "{error}"
        );
        for good in ["a/b", "Jun-egunn/fzf", "o/re_po.rs", "o/..x", "0/-"] {
            assert!(
                parse(&format!("[v]\ngithub-release = '{good}'\n")).is_ok(),
                "{good}"
            );
        }
        for bad in [
            "",
            "a",
            "a/",
            "/b",
            "a/b/c",
            "a_b/c",
            "a.b/c",
            "a/.",
            "a/..",
            "a/b.git",
            "https://github.com/a/b",
            "a/b c",
            "a/\u{e9}",
        ] {
            let error =
                parse(&format!("[v]\ngithub-release = \"{bad}\"\n")).expect_err("not a repository");
            let message = error.to_string();
            assert!(message.contains("as `OWNER/REPO`"), "{bad}: {message}");
        }
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
            error
                .to_string()
                .contains("`command-timeout` must be greater than zero"),
            "{error}"
        );
        let error = parse("[tag]\ngithub-release = 'a/b'\ntimeout = '0s'\n")
            .expect_err("a zero timeout is not a coherent request");
        assert!(
            error
                .to_string()
                .contains("`timeout` must be greater than zero"),
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
