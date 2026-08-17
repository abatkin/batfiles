//! The value shapes `batfiles.toml` reuses across records.
//!
//! Two of them are unions of a convenient short form and an explicit long form.
//! Serde's `untagged` derive would read them, but it reports every mistake
//! inside either form as "data did not match any variant", so both deserialize
//! through a visitor instead: the short form is decided by the TOML type, and
//! the long form's own error — an unknown field, a missing field, a non-string
//! list item — survives with its location.

use std::fmt;
use std::marker::PhantomData;

use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer, StrDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use simple_expressions::parser::parse_expression;
use simple_expressions::types::error::Error as ExpressionError;
use simple_expressions::types::expression::Expr;

use crate::item::{ItemId, ItemIdError};

/// A condition expression: the text as written, and the tree it parses to.
///
/// Conditions are written in [Simple
/// Expressions](https://github.com/abatkin/expressions-rs). The language owns
/// the grammar, but the *parse* happens here, while the manifest is being read,
/// so `when = "work &&"` is a TOML error with a line number rather than a
/// surprise at evaluation time. What the identifiers in a parsed condition
/// *mean*, and what counts as true, belong to
/// [`crate::condition`](crate::condition).
///
/// **Both halves are kept, and only the text is presented.** [`Debug`],
/// [`PartialEq`], and [`Serialize`] all read [`source`](Self::source); the tree
/// is reachable only through [`expr`](Self::expr), which is evaluation's. Text
/// equality is *correct* rather than a limitation to fix later: `"a && b"` and
/// `"a&&b"` are different manifests, and batfiles never rewrites a manifest, so
/// a condition's identity is what was written.
///
/// The divergence from [`FriendlyDuration`](super::FriendlyDuration) and
/// [`RepoPath`], which both drop their source text, is deliberate. Those have a
/// canonical form and no display obligation, so `1d` and `24h` are one value. A
/// condition has neither: there is no canonical spelling, and `vars refresh
/// --interactive` has to show a user the exact text they wrote before it runs
/// anything. Reconstructing the text from the tree — [`ItemAddress`]'s trick —
/// does not transfer, because whitespace, parentheses, and quote style are not
/// recoverable from an AST.
///
/// Every record carrying a `when` also accepts `unless` as its negated alias,
/// and the two are mutually exclusive. Nothing in this module enforces that:
/// both fields deserialize independently, and rejecting the pair belongs to
/// [`BatfilesConfig::validate`](super::BatfilesConfig::validate).
///
/// [`ItemAddress`]: crate::item::ItemAddress
#[derive(Clone, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Condition {
    source: String,
    expr: Expr,
}

impl Condition {
    /// Parse `source` as a condition, or report why it is not one.
    ///
    /// The ergonomic constructor every other validated type in the crate has.
    /// Every condition batfiles actually reads arrives through [`TryFrom`]
    /// instead, since they all come out of a manifest.
    #[allow(dead_code, reason = "the tests are the only caller so far")]
    pub fn new(source: &str) -> Result<Self, ConditionError> {
        Self::try_from(source.to_owned())
    }

    /// The condition exactly as written, whitespace and quote style included.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The parsed tree, for the module that evaluates it.
    pub fn expr(&self) -> &Expr {
        &self.expr
    }
}

/// Why a candidate condition was rejected.
///
/// Carries the candidate and, when the parser reported one, the position within
/// it. The parser's own `rendered` caret diagram is deliberately *not* used: this
/// message is rendered inside TOML's caret diagram, so a second one would repeat
/// the source text and point two carets at different things. One line inside an
/// error that already carries the file, line, and column is the right shape —
/// the same judgment [`DurationError`](super::FriendlyDuration) made when it
/// declined to re-emit jiff's message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConditionError {
    candidate: String,
    /// Absent when the parser failed without a position, which only its internal
    /// error does.
    position: Option<Position>,
    message: String,
}

/// Where within the condition the parser stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Position {
    /// 1-based, and 1 for every condition written as an ordinary TOML string.
    line: usize,
    /// 1-based, in characters.
    column: usize,
}

impl ConditionError {
    fn new(candidate: &str, error: &ExpressionError) -> Self {
        // `Error` is `#[non_exhaustive]`, so the wildcard is required; everything
        // but a parse failure loses only the position, not the message.
        let (position, message) = match error {
            ExpressionError::ParseError {
                line,
                column,
                message,
                ..
            } => (
                Some(Position {
                    line: *line,
                    column: *column,
                }),
                message.clone(),
            ),
            other => (None, other.to_string()),
        };
        Self {
            candidate: candidate.to_owned(),
            position,
            message,
        }
    }
}

/// Render `text` so it cannot break the one-line shape of a diagnostic.
///
/// A TOML multi-line string is a legal place to write a condition, so a
/// candidate can contain real newlines. Interpolating one verbatim would put
/// extra source lines *inside* TOML's own caret report — the exact thing
/// declining to use the parser's `rendered` block was meant to avoid. Only the
/// characters that break a line are escaped, and the backslash with them so the
/// escape is unambiguous; quotes are left alone, because a condition is full of
/// them and `\'work\'` reads worse than `'work'`.
fn one_line(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

impl fmt::Display for ConditionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not a valid condition: {}",
            one_line(&self.candidate),
            self.message
        )?;
        match self.position {
            // A TOML multi-line string can hold a newline, so the line is worth
            // naming when there is more than one; otherwise it is always 1 and
            // saying so would be noise beside TOML's own line number.
            Some(Position { line, column }) if line > 1 => {
                write!(f, " at line {line}, character {column}")
            }
            Some(Position { column, .. }) => write!(f, " at character {column}"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for ConditionError {}

impl TryFrom<String> for Condition {
    type Error = ConditionError;

    fn try_from(source: String) -> Result<Self, Self::Error> {
        let expr =
            parse_expression(&source).map_err(|error| ConditionError::new(&source, &error))?;
        Ok(Self { source, expr })
    }
}

/// The source text, so an `assert_eq!` failure over a record holding one is
/// readable. A derived implementation would print the whole tree, and every
/// record carrying an `Option<Condition>` derives [`Debug`].
impl fmt::Debug for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Condition").field(&self.source).finish()
    }
}

/// Over the source text. Also forced: [`Expr`] holds an `f64` and is therefore
/// not [`Eq`], while every record holding an `Option<Condition>` derives it.
impl PartialEq for Condition {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source
    }
}

impl Eq for Condition {}

impl Serialize for Condition {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.source)
    }
}

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
    pub remote: ItemId,
    /// The path within that remote's materialization.
    pub path: String,
}

impl RemotePath {
    /// Split the `@remote/path` shorthand.
    ///
    /// Both halves are required, so `"@core"` and `"@/files"` are malformed
    /// rather than a remote with no path or a path with no remote. Whether the
    /// remote exists, and whether this repository may refer to one at all, are
    /// questions for the resolver; this is only the shorthand's grammar and the
    /// remote segment's ID syntax.
    fn from_shorthand(value: &str) -> Result<Self, ShorthandError> {
        let body = value
            .strip_prefix(REMOTE_SIGIL)
            .ok_or(ShorthandError::Shape)?;
        match body.split_once('/') {
            Some((remote, path)) if !remote.is_empty() && !path.is_empty() => Ok(Self {
                remote: ItemId::new(remote).map_err(ShorthandError::Remote)?,
                path: path.to_owned(),
            }),
            _ => Err(ShorthandError::Shape),
        }
    }
}

/// Why a `@`-prefixed string was not a remote path.
enum ShorthandError {
    /// It was not `@<remote>/<path>` at all.
    Shape,
    /// It had both halves, but the remote is not a valid ID.
    Remote(ItemIdError),
}

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
                        .map_err(|error| match error {
                            ShorthandError::Shape => E::custom(format!(
                                "`{value}` starts with `@`, so it must be `@<remote>/<path>`"
                            )),
                            ShorthandError::Remote(error) => E::custom(error),
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

/// One value or a list of them, normalized to a list.
///
/// Two field shapes are written this way — `GlobFilter` and the `include-*` /
/// `exclude-*` ID lists — and in both a bare string means a one-item list. The
/// one-or-many spelling is the only thing they share, so it lives here once and
/// the element type says which shape a field is.
///
/// The distinction between the two spellings carries no meaning, so it is not
/// preserved; the difference between an absent field and an empty list does
/// carry meaning, so callers hold an `Option<..>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub(crate) struct OneOrMany<T> {
    pub items: Vec<T>,
}

/// One glob or a list of them: the `include`/`exclude` filters on symlink, copy,
/// fetch-url, and archive remotes. Globs are matched where they are used, not
/// validated here.
pub(crate) type GlobFilter = OneOrMany<String>;

/// One ID or a list of them: the `include-remote` selection fields. Each element
/// is validated as an [`ItemId`], in either spelling.
pub(crate) type ItemIdList = OneOrMany<ItemId>;

impl<T> OneOrMany<T> {
    #[allow(
        dead_code,
        reason = "the selection and glob filters are read when actions are planned"
    )]
    pub fn as_slice(&self) -> &[T] {
        &self.items
    }
}

/// An absent list and an empty one differ, so this is the empty *list* rather
/// than a default element; it is written by hand because `T` need not be
/// `Default`.
impl<T> Default for OneOrMany<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T, U: Into<T>> FromIterator<U> for OneOrMany<T> {
    fn from_iter<I: IntoIterator<Item = U>>(items: I) -> Self {
        Self {
            items: items.into_iter().map(Into::into).collect(),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for OneOrMany<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OneOrManyVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for OneOrManyVisitor<T> {
            type Value = OneOrMany<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a string or a list of strings")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                // The scalar goes through `T`'s own deserializer rather than
                // being wrapped directly, so an element type that validates —
                // `ItemId` — rejects a bad value in this spelling too.
                let item = T::deserialize(StrDeserializer::<E>::new(value))?;
                Ok(OneOrMany { items: vec![item] })
            }

            fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Self::Value, A::Error> {
                Vec::deserialize(SeqAccessDeserializer::new(seq)).map(|items| OneOrMany { items })
            }
        }

        deserializer.deserialize_any(OneOrManyVisitor(PhantomData))
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

    fn id(id: &str) -> ItemId {
        ItemId::new(id).expect("valid id")
    }

    fn remote(remote: &str, path: &str) -> RepoPath {
        RepoPath::Remote(RemotePath {
            remote: id(remote),
            path: path.to_owned(),
        })
    }

    #[test]
    fn a_condition_keeps_the_text_it_was_written_as() {
        let condition = parse::<Condition>("value = \"work && facts.os == 'macos'\"")
            .expect("a valid condition");
        assert_eq!(condition.source(), "work && facts.os == 'macos'");

        // Whitespace is part of the text, and two spellings of one expression
        // are two manifests: batfiles never rewrites one, so its identity is
        // what was written.
        let spaced = parse::<Condition>("value = 'a && b'").expect("spaced");
        let tight = parse::<Condition>("value = 'a&&b'").expect("tight");
        assert_eq!(spaced.source(), "a && b");
        assert_eq!(tight.source(), "a&&b");
        assert_ne!(spaced, tight);
        assert_eq!(
            spaced,
            parse::<Condition>("value = 'a && b'").expect("again")
        );
    }

    #[test]
    fn a_malformed_condition_is_rejected_where_it_is_written() {
        // The whole point of the newtype: `when = "work &&"` is a load error
        // with a line number rather than a surprise at evaluation time.
        let error = parse::<Condition>("value = 'work &&'")
            .expect_err("a trailing operator is not an expression")
            .to_string();

        assert!(
            error.contains("`work &&` is not a valid condition"),
            "{error}"
        );
        assert!(error.contains("expected unary"), "{error}");
        // The position within the condition, which is what the crate's
        // structured parse error buys over a bare message.
        assert!(error.contains("character 8"), "{error}");
        // And TOML's own location, pointing at the value in the file.
        assert!(error.contains("line 1"), "{error}");
    }

    #[test]
    fn a_rejected_condition_is_one_line_even_when_the_condition_is_not() {
        // Decision 2's "do not use `rendered`", pinned as a property rather
        // than as an intention: the crate's caret diagram would sit inside
        // TOML's caret diagram, pointing two carets at different things.
        let error = Condition::new("work &&")
            .expect_err("malformed")
            .to_string();
        assert!(!error.contains('\n'), "{error}");

        // A TOML multi-line string is a legal place to write a condition, so
        // the candidate itself can carry newlines into the message. It is the
        // same defect by a different route, so it gets the same guarantee.
        let error = Condition::new("a &&\nb &&")
            .expect_err("malformed")
            .to_string();
        assert!(!error.contains('\n'), "{error}");
        assert!(error.contains("`a &&\\nb &&`"), "{error}");
    }

    #[test]
    fn a_multi_line_condition_names_its_line_as_well_as_its_character() {
        // A TOML multi-line string can hold a newline, which is the only way
        // `line` is ever anything but 1.
        let error = Condition::new("a &&\nb &&")
            .expect_err("malformed")
            .to_string();
        assert!(error.contains("line 2, character 5"), "{error}");
    }

    #[test]
    fn a_condition_serializes_back_to_its_source_text() {
        let condition = parse::<Condition>("value = 'a && b'").expect("parse");
        let document = toml::to_string(&Wrapper { value: condition }).expect("serialize");

        assert_eq!(document, "value = \"a && b\"\n");
        assert_eq!(
            parse::<Condition>(&document).expect("reparse").source(),
            "a && b"
        );
    }

    #[test]
    fn a_condition_debugs_as_its_text_rather_than_as_a_tree() {
        // Every record carrying an `Option<Condition>` derives `Debug`, so a
        // derived tree here would make every `assert_eq!` failure across
        // `src/repo/` unreadable.
        let condition = parse::<Condition>("value = 'a && b'").expect("parse");
        assert_eq!(format!("{condition:?}"), "Condition(\"a && b\")");
    }

    #[test]
    fn an_identifier_may_begin_with_a_boolean_keyword() {
        // `docs/repoformat.md` makes a user variable name
        // `[A-Za-z_][A-Za-z0-9_]*` minus five reserved words, so `trueish` and
        // `false_value` are legal names — and they have to parse as *names*.
        //
        // They did not before `simple-expressions` 0.4.1: the grammar spelled
        // the literals with no word boundary, and PEG ordered choice tries
        // `boolean` before `ident`, so `trueish` matched `true` and then choked
        // on the trailing `ish`. This is the regression test for the boundary.
        for name in [
            "trueish",
            "false_value",
            "falsey",
            "true_",
            "_true",
            "x_true",
        ] {
            Condition::new(name).unwrap_or_else(|error| panic!("`{name}`: {error}"));
        }

        // The other half of the same fix, and the direction it could overshoot:
        // a boundary that swallowed the keywords would make `true` an
        // identifier, which is a reserved name nothing can declare — so every
        // `when = "true"` in the wild would start failing as undeclared.
        assert!(
            Condition::new("true").expect("a literal").expr()
                == &Expr::Literal(simple_expressions::types::primitive::Primitive::Bool(true))
        );
        assert!(
            Condition::new("false").expect("a literal").expr()
                == &Expr::Literal(simple_expressions::types::primitive::Primitive::Bool(false))
        );
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
    fn a_remote_segment_must_be_a_valid_id_in_either_spelling() {
        // The remote names a key in the `[remotes]` map, so it obeys the ID rule
        // wherever it is written — and the shorthand's own errors survive.
        let error = parse::<RepoPath>("value = '@my remote/files'").expect_err("invalid remote");
        assert!(error.to_string().contains("`my remote`"), "{error}");
        assert!(error.to_string().contains("not a valid ID"), "{error}");

        let error = parse::<RepoPath>("value = { remote = 'my remote', path = 'files' }")
            .expect_err("invalid remote");
        assert!(error.to_string().contains("`my remote`"), "{error}");
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
    fn a_glob_filter_accepts_one_string_or_many() {
        assert_eq!(
            parse::<GlobFilter>("value = '*.toml'").expect("one"),
            GlobFilter::from_iter(["*.toml"])
        );
        assert_eq!(
            parse::<GlobFilter>("value = ['private/*', '*.bak']").expect("many"),
            GlobFilter::from_iter(["private/*", "*.bak"])
        );
        assert_eq!(
            parse::<GlobFilter>("value = []").expect("empty"),
            GlobFilter::default()
        );
    }

    #[test]
    fn a_one_or_many_list_rejects_a_non_string_item_at_its_position() {
        let error = parse::<GlobFilter>("value = ['ok', 2]").expect_err("integers are not globs");
        assert!(
            error.to_string().contains("invalid type: integer"),
            "{error}"
        );
        assert!(error.to_string().contains("column 16"), "{error}");
    }

    #[test]
    fn an_id_list_takes_the_same_shapes_as_a_glob_filter() {
        assert_eq!(
            parse::<ItemIdList>("value = 'p10k'").expect("one"),
            ItemIdList::from_iter([id("p10k")])
        );
        assert_eq!(
            parse::<ItemIdList>("value = ['p10k', 'oh-my-zsh']").expect("many"),
            ItemIdList::from_iter([id("p10k"), id("oh-my-zsh")])
        );
        assert_eq!(
            parse::<ItemIdList>("value = []").expect("empty"),
            ItemIdList::default()
        );
    }

    #[test]
    fn an_id_list_validates_its_elements_in_either_spelling() {
        // The scalar spelling is a one-item list, so it cannot be the loophole
        // that lets an invalid ID through.
        for document in ["value = 'core.zshrc'", "value = ['ok', 'core.zshrc']"] {
            let error = parse::<ItemIdList>(document).expect_err("dots are not part of an ID");
            assert!(error.to_string().contains("`core.zshrc`"), "{error}");
        }
    }

    #[test]
    fn both_spellings_of_a_one_or_many_list_serialize_as_a_list() {
        let globs = Wrapper {
            value: GlobFilter::from_iter(["*.toml"]),
        };
        assert_eq!(
            toml::to_string(&globs).expect("serialize"),
            "value = [\"*.toml\"]\n"
        );

        let ids = Wrapper {
            value: ItemIdList::from_iter([id("p10k")]),
        };
        assert_eq!(
            toml::to_string(&ids).expect("serialize"),
            "value = [\"p10k\"]\n"
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
            value: remote("core", "files/zshrc"),
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
