//! Parse local and remote repository paths for resolution by
//! [`RunContext`](crate::action::RunContext). Path validation belongs to
//! [`manifest::check`](crate::manifest::check).

use std::fmt;

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, Visitor};
use thiserror::Error;

use crate::item::{ItemId, ItemIdError};

/// The character that introduces a remote reference, reserved at the start of a
/// repository path and ordinary anywhere else in one.
pub(crate) const REMOTE_PREFIX: char = '@';

/// The two fields the structured spelling accepts, for the message an unknown
/// one gets.
const FIELDS: &[&str] = &["remote", "path"];

/// A path within a repository, and which repository that is.
///
/// Written three ways, all of which arrive here as the same two parts:
///
/// ```toml
/// source = "files/zshrc"
/// source = "@core/files/zshrc"
/// source = { remote = "core", path = "files/zshrc" }
/// ```
///
/// A leading `@` is what tells the first spelling from the second, so it cannot
/// also start an ordinary path. The remote is an [`ItemId`] by construction; that
/// a manifest declares one under that ID is a question for the document, not for
/// a single value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepoPath {
    /// The remote whose materialization the path is read from, or `None` for
    /// the repository that declared it.
    remote: Option<ItemId>,
    /// The path within that tree, as written and otherwise unexamined.
    path: String,
}

impl RepoPath {
    /// A path within the repository that declared it.
    pub fn local(path: String) -> Self {
        Self { remote: None, path }
    }

    /// The remote this path is read from, or `None` for the declaring repository.
    pub fn remote(&self) -> Option<&ItemId> {
        self.remote.as_ref()
    }

    /// The path within whichever tree that is. Unvalidated: what it may say is
    /// `check_repo_path`'s question, in [`manifest::check`](crate::manifest::check).
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Read the string spelling, which is a remote reference when it starts with
    /// `@` and a path in the declaring repository otherwise.
    fn parse(value: &str) -> Result<Self, RepoPathError> {
        let Some(reference) = value.strip_prefix(REMOTE_PREFIX) else {
            return Ok(Self::local(value.to_owned()));
        };
        // Everything up to the first separator names the remote; the rest is the
        // path within it, which is empty where the reference names no path at all.
        let (id, path) = reference.split_once('/').unwrap_or((reference, ""));
        if id.is_empty() {
            return Err(RepoPathError::RemoteUnnamed {
                candidate: value.to_owned(),
            });
        }
        Ok(Self {
            remote: Some(ItemId::try_from(id.to_owned())?),
            path: path.to_owned(),
        })
    }
}

/// One spelling for a diagnostic to quote, whichever of the three was written.
impl fmt::Display for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.remote {
            None => f.write_str(&self.path),
            // A reference that names no path is written back without the
            // separator it never had.
            Some(id) if self.path.is_empty() => write!(f, "{REMOTE_PREFIX}{id}"),
            Some(id) => write!(f, "{REMOTE_PREFIX}{id}/{}", self.path),
        }
    }
}

/// Why a value is not a repository path at all. The rules a path that parses
/// still has to satisfy are the manifest's, and are reported as part of the
/// record that wrote it.
#[derive(Debug, Error)]
pub(crate) enum RepoPathError {
    #[error("`{candidate}` names no remote; write `@<remote>/<path>`")]
    RemoteUnnamed { candidate: String },

    #[error(transparent)]
    RemoteId(#[from] ItemIdError),
}

/// Read a string or a closed table, preserving field-specific parse errors.
impl<'de> Deserialize<'de> for RepoPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(RepoPathVisitor)
    }
}

struct RepoPathVisitor;

impl<'de> Visitor<'de> for RepoPathVisitor {
    type Value = RepoPath;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a repository path, `@remote/path`, or a table with `remote` and `path`")
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        RepoPath::parse(value).map_err(E::custom)
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut remote: Option<ItemId> = None;
        let mut path: Option<String> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "remote" => {
                    if remote.is_some() {
                        return Err(de::Error::duplicate_field("remote"));
                    }
                    remote = Some(map.next_value()?);
                }
                "path" => {
                    if path.is_some() {
                        return Err(de::Error::duplicate_field("path"));
                    }
                    path = Some(map.next_value()?);
                }
                unknown => return Err(de::Error::unknown_field(unknown, FIELDS)),
            }
        }
        // The structured spelling is the one that names a remote outright, so
        // `remote` is required: a table with only a path is the string spelling
        // written the long way around, and means nothing else. Without `path`
        // it is `@<remote>`, which names a file remote whole.
        Ok(RepoPath {
            remote: Some(remote.ok_or_else(|| de::Error::missing_field("remote"))?),
            path: path.unwrap_or_default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Record {
        source: RepoPath,
    }

    fn read(document: &str) -> Result<RepoPath, toml::de::Error> {
        toml::from_str::<Record>(document).map(|record| record.source)
    }

    fn parsed(document: &str) -> RepoPath {
        read(document).unwrap_or_else(|error| panic!("{error}"))
    }

    fn refused(document: &str) -> String {
        read(document)
            .expect_err("expected the path to be refused")
            .to_string()
    }

    fn remote(id: &str) -> ItemId {
        ItemId::try_from(id.to_owned()).expect("valid ID")
    }

    #[test]
    fn a_plain_string_is_a_path_in_the_declaring_repository() {
        let source = parsed("source = \"files/zshrc\"\n");
        assert_eq!(source.remote(), None);
        assert_eq!(source.path(), "files/zshrc");
    }

    #[test]
    fn both_remote_spellings_read_as_the_same_two_parts() {
        let shorthand = parsed("source = \"@core/files/zshrc\"\n");
        let structured = parsed("source = { remote = \"core\", path = \"files/zshrc\" }\n");
        assert_eq!(shorthand, structured);
        assert_eq!(shorthand.remote(), Some(&remote("core")));
        assert_eq!(shorthand.path(), "files/zshrc");
        // A full table is the same record as an inline one.
        assert_eq!(
            parsed("[source]\nremote = \"core\"\npath = \"files/zshrc\"\n"),
            structured
        );
    }

    #[test]
    fn only_a_leading_at_sign_names_a_remote() {
        // Anywhere else it is an ordinary character in a path, which is why the
        // reservation is stated as a rule about the first one.
        let source = parsed("source = \"files/@work/zshrc\"\n");
        assert_eq!(source.remote(), None);
        assert_eq!(source.path(), "files/@work/zshrc");
    }

    #[test]
    fn a_reference_may_name_a_remote_and_no_path() {
        // Accepted here and refused by the manifest, which is where naming the
        // whole of a tree is a rule about sources rather than about spelling.
        let source = parsed("source = \"@core\"\n");
        assert_eq!(source.remote(), Some(&remote("core")));
        assert_eq!(source.path(), "");
    }

    #[test]
    fn a_reference_naming_no_remote_is_refused() {
        for candidate in ["@", "@/files/zshrc"] {
            let message = refused(&format!("source = \"{candidate}\"\n"));
            assert!(message.contains("names no remote"), "{message}");
        }
    }

    #[test]
    fn a_remote_is_named_by_an_id() {
        // The ID rule reported as itself, so one sentence explains the name
        // wherever it was written.
        let message = refused("source = \"@core.old/files/zshrc\"\n");
        assert!(message.contains("`core.old`"), "{message}");
        assert!(message.contains("not a valid ID"), "{message}");
        let structured = refused("source = { remote = \"core.old\", path = \"a\" }\n");
        assert!(structured.contains("not a valid ID"), "{structured}");
    }

    #[test]
    fn the_structured_spelling_is_closed_and_needs_a_remote() {
        let unknown = refused("source = { remote = \"core\", path = \"a\", ref = \"main\" }\n");
        assert!(unknown.contains("unknown field `ref`"), "{unknown}");
        let missing = refused("source = { path = \"files/zshrc\" }\n");
        assert!(missing.contains("missing field `remote`"), "{missing}");
        // What `@core` says, the way a file remote is named whole.
        assert_eq!(
            parsed("source = { remote = \"core\" }\n"),
            parsed("source = \"@core\"\n")
        );
    }

    #[test]
    fn a_value_that_is_neither_spelling_says_what_one_looks_like() {
        let message = refused("source = 3\n");
        assert!(message.contains("`@remote/path`"), "{message}");
    }

    #[test]
    fn a_path_is_quoted_the_way_the_shorthand_spells_it() {
        // Diagnostics quote one form, so a structured reference and the
        // shorthand for it read alike.
        for (document, written) in [
            ("source = \"files/zshrc\"\n", "files/zshrc"),
            ("source = \"@core/files/zshrc\"\n", "@core/files/zshrc"),
            (
                "source = { remote = \"core\", path = \"files/zshrc\" }\n",
                "@core/files/zshrc",
            ),
            ("source = \"@core\"\n", "@core"),
        ] {
            assert_eq!(parsed(document).to_string(), written);
        }
    }
}
