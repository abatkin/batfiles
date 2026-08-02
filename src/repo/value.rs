//! The value shapes `batfiles.toml` reuses across records.
//!
//! Two of them are unions of a convenient short form and an explicit long form
//! (`docs/repoformat.md#shared-value-types`). Serde's `untagged` derive would
//! read them, but it reports every mistake inside either form as "data did not
//! match any variant", so both deserialize through a visitor instead: the short
//! form is decided by the TOML type, and the long form's own error — an unknown
//! field, a missing field, a non-string list item — survives with its location.

use std::fmt;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

/// A condition expression, held verbatim.
///
/// Conditions are written in [Simple
/// Expressions](https://github.com/abatkin/expressions-rs) and are parsed by
/// that language, not here (`docs/repoformat.md#condition`). The alias exists so
/// every `when`/`unless` field says what its string is for.
///
/// Every record carrying a `when` also accepts `unless` as its negated alias,
/// and the two are mutually exclusive. Nothing in this module enforces that:
/// both fields deserialize independently, and rejecting the pair belongs to the
/// validation that reads them.
pub(crate) type Condition = String;

/// A friendly duration such as `30s`, `1h 30m`, or `90 minutes`, held verbatim.
///
/// The grammar and the rejection of negative values live with the code that
/// interprets one (`docs/repoformat.md#duration-values`).
pub(crate) type DurationString = String;

/// A path read from a repository: `RepoPath = string | { remote, path }`.
///
/// Three spellings, two meanings. `"@core/files/zshrc"` is defined as shorthand
/// for `{ remote = "core", path = "files/zshrc" }`, so it is split here and the
/// variants are the two meanings rather than the three spellings. Everything
/// downstream — resolution, and the rule that only a leaf repository's own
/// actions may name a remote at all — then asks which variant it has instead of
/// asking that *and* re-testing a string for a leading `@`.
///
/// Consequently a repository-relative path cannot begin with `@`: the format
/// defines no escape, and reading the shorthand is what gives it that meaning
/// wherever it appears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub(crate) enum RepoPath {
    /// A path relative to the repository that declared it, such as
    /// `"files/zshrc"`. Never begins with `@`.
    Relative(String),
    /// A path in a declared remote, written either as `"@core/files/zshrc"` or
    /// as `{ remote = "core", path = "files/zshrc" }`.
    Remote(RemotePath),
}

/// The character introducing the remote shorthand.
const REMOTE_SIGIL: char = '@';

/// A path in a declared remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RemotePath {
    /// The remote's ID: its key in the leaf repository's `[remotes]` map.
    pub remote: String,
    /// The path within that remote's materialization.
    pub path: String,
}

impl RemotePath {
    /// Split the `@remote/path` shorthand.
    ///
    /// Both halves are required, so `"@core"` and `"@/files"` are malformed
    /// rather than a remote with no path or a path with no remote. Whether the
    /// remote exists, and whether this repository may refer to one at all, are
    /// questions for the resolver; this is only the shorthand's grammar.
    fn from_shorthand(value: &str) -> Result<Self, ShorthandError> {
        let body = value.strip_prefix(REMOTE_SIGIL).ok_or(ShorthandError)?;
        match body.split_once('/') {
            Some((remote, path)) if !remote.is_empty() && !path.is_empty() => Ok(Self {
                remote: remote.to_owned(),
                path: path.to_owned(),
            }),
            _ => Err(ShorthandError),
        }
    }
}

/// A string that began with `@` but was not `@<remote>/<path>`.
struct ShorthandError;

impl<'de> Deserialize<'de> for RepoPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RepoPathVisitor;

        impl<'de> Visitor<'de> for RepoPathVisitor {
            type Value = RepoPath;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a repository path, `@<remote>/<path>`, or a remote/path table")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<RepoPath, E> {
                if value.starts_with(REMOTE_SIGIL) {
                    return RemotePath::from_shorthand(value)
                        .map(RepoPath::Remote)
                        .map_err(|ShorthandError| {
                            E::custom(format!(
                                "`{value}` starts with `@`, so it must be `@<remote>/<path>`"
                            ))
                        });
                }
                Ok(RepoPath::Relative(value.to_owned()))
            }

            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<RepoPath, A::Error> {
                RemotePath::deserialize(MapAccessDeserializer::new(map)).map(RepoPath::Remote)
            }
        }

        deserializer.deserialize_any(RepoPathVisitor)
    }
}

/// One string or a list of them, normalized to a list.
///
/// Both `GlobFilter` and the `include-*`/`exclude-*` ID lists are written this
/// way, and a bare string means a one-item list
/// (`docs/repoformat.md#glob-filter`). The distinction between the two spellings
/// carries no meaning, so it is not preserved; the difference between an absent
/// field and an empty list does carry meaning, so callers hold an
/// `Option<StringList>`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct StringList {
    pub items: Vec<String>,
}

impl StringList {
    pub fn as_slice(&self) -> &[String] {
        &self.items
    }
}

impl<T: Into<String>> FromIterator<T> for StringList {
    fn from_iter<I: IntoIterator<Item = T>>(items: I) -> Self {
        Self {
            items: items.into_iter().map(Into::into).collect(),
        }
    }
}

impl<'de> Deserialize<'de> for StringList {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StringListVisitor;

        impl<'de> Visitor<'de> for StringListVisitor {
            type Value = StringList;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string or a list of strings")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<StringList, E> {
                Ok(StringList {
                    items: vec![value.to_owned()],
                })
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<StringList, A::Error> {
                Vec::deserialize(SeqAccessDeserializer::new(seq)).map(|items| StringList { items })
            }
        }

        deserializer.deserialize_any(StringListVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both value shapes only ever appear as a field, and a bare value is not a
    /// TOML document, so the tests wrap them in one.
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Wrapper<T> {
        value: T,
    }

    fn parse<T: serde::de::DeserializeOwned>(line: &str) -> Result<T, toml::de::Error> {
        toml::from_str::<Wrapper<T>>(line).map(|wrapper| wrapper.value)
    }

    fn remote(remote: &str, path: &str) -> RepoPath {
        RepoPath::Remote(RemotePath {
            remote: remote.to_owned(),
            path: path.to_owned(),
        })
    }

    #[test]
    fn a_plain_string_is_relative_to_its_own_repository() {
        assert_eq!(
            parse::<RepoPath>("value = 'files/zshrc'").expect("string form"),
            RepoPath::Relative("files/zshrc".to_owned())
        );
    }

    #[test]
    fn the_shorthand_and_the_table_produce_the_same_value() {
        // `@core/files/zshrc` is *defined* as the table, so nothing downstream
        // should have to know which spelling it was written in.
        let shorthand = parse::<RepoPath>("value = '@core/files/zshrc'").expect("shorthand");
        let table = parse::<RepoPath>("value = { remote = 'core', path = 'files/zshrc' }")
            .expect("table form");

        assert_eq!(shorthand, remote("core", "files/zshrc"));
        assert_eq!(shorthand, table);
    }

    #[test]
    fn only_the_first_slash_of_the_shorthand_separates_it() {
        assert_eq!(
            parse::<RepoPath>("value = '@core/files/nested/zshrc'").expect("shorthand"),
            remote("core", "files/nested/zshrc")
        );
    }

    #[test]
    fn a_shorthand_missing_either_half_is_rejected() {
        // Silently keeping these as relative paths would turn a typo into a
        // lookup for a directory literally named `@core`.
        for malformed in ["@core", "@/files/zshrc", "@core/", "@", "@/"] {
            let error = parse::<RepoPath>(&format!("value = '{malformed}'"))
                .expect_err("{malformed} should not parse")
                .to_string();
            assert!(error.contains("must be `@<remote>/<path>`"), "{error}");
            assert!(error.contains(malformed), "{error}");
        }
    }

    #[test]
    fn a_rejected_shorthand_points_at_the_value() {
        let error = parse::<RepoPath>("value = '@core'").expect_err("malformed");
        assert!(error.to_string().contains("line 1"), "{error}");
    }

    #[test]
    fn a_structured_repo_path_reports_its_own_mistakes() {
        let error = parse::<RepoPath>("value = { remote = 'core' }").expect_err("missing path");
        assert!(
            error.to_string().contains("missing field `path`"),
            "{error}"
        );

        let error = parse::<RepoPath>("value = { remote = 'core', path = 'p', extra = 1 }")
            .expect_err("unknown field");
        assert!(
            error.to_string().contains("unknown field `extra`"),
            "{error}"
        );
    }

    #[test]
    fn a_repo_path_of_the_wrong_type_says_what_was_expected() {
        let error = parse::<RepoPath>("value = 7").expect_err("integers are not paths");
        assert!(
            error
                .to_string()
                .contains("expected a repository path, `@<remote>/<path>`, or a remote/path table"),
            "{error}"
        );
    }

    #[test]
    fn a_string_list_accepts_one_string_or_many() {
        assert_eq!(
            parse::<StringList>("value = '*.toml'").expect("one"),
            StringList::from_iter(["*.toml"])
        );
        assert_eq!(
            parse::<StringList>("value = ['private/*', '*.bak']").expect("many"),
            StringList::from_iter(["private/*", "*.bak"])
        );
        assert_eq!(
            parse::<StringList>("value = []").expect("empty"),
            StringList::default()
        );
    }

    #[test]
    fn a_string_list_rejects_a_non_string_item_at_its_position() {
        let error = parse::<StringList>("value = ['ok', 2]").expect_err("integers are not globs");
        assert!(
            error.to_string().contains("invalid type: integer"),
            "{error}"
        );
        assert!(error.to_string().contains("column 16"), "{error}");
    }

    #[test]
    fn both_spellings_of_a_string_list_serialize_as_a_list() {
        let one = Wrapper {
            value: StringList::from_iter(["*.toml"]),
        };
        assert_eq!(
            toml::to_string(&one).expect("serialize"),
            "value = [\"*.toml\"]\n"
        );
    }

    #[test]
    fn a_repo_path_serializes_back_to_the_form_it_holds() {
        let path = Wrapper {
            value: RepoPath::Relative("files/zshrc".to_owned()),
        };
        assert_eq!(
            toml::to_string(&path).expect("serialize"),
            "value = \"files/zshrc\"\n"
        );

        let remote = Wrapper {
            value: RepoPath::Remote(RemotePath {
                remote: "core".to_owned(),
                path: "files/zshrc".to_owned(),
            }),
        };
        assert_eq!(
            toml::to_string(&remote).expect("serialize"),
            "[value]\nremote = \"core\"\npath = \"files/zshrc\"\n"
        );
    }

    #[test]
    fn a_shorthand_serializes_as_the_table_it_means() {
        // Writing a manifest is not something batfiles does, so canonicalizing
        // the shorthand costs nothing and keeps one value with one spelling.
        let shorthand = parse::<RepoPath>("value = '@core/files/zshrc'").expect("shorthand");
        let document = toml::to_string(&Wrapper { value: shorthand }).expect("serialize");

        assert_eq!(
            document,
            "[value]\nremote = \"core\"\npath = \"files/zshrc\"\n"
        );
        assert_eq!(
            parse::<RepoPath>(&document).expect("reparse"),
            remote("core", "files/zshrc")
        );
    }
}
