//! `[remotes]`: the sources a repository may materialize.
//!
//! Declaring a remote only names and fetches a source. Actions decide whether
//! and where its content is installed, and only a Git remote can be spliced in
//! with `include-remote`.

use serde::{Deserialize, Serialize};

use crate::repo::value::{Condition, GlobFilter};

/// One entry of `[remotes]`, tagged by `type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum Remote {
    Git(GitRemote),
    File(FileRemote),
    Archive(ArchiveRemote),
}

impl Remote {
    /// The tool-owned tree that materialized remotes live in, directly under
    /// the leaf repository root.
    ///
    /// The name travels with the type it describes, the way a document's name
    /// travels with its parser ([`BatfilesConfig::FILE_NAME`]). Which *root* the
    /// tree sits under is location policy and belongs to
    /// [`Roots::remotes_dir`](crate::config::Roots::remotes_dir); `init` reads
    /// this to exclude the tree from Git. It is spelled once so those two can
    /// never disagree — if they did, `init` would exclude one directory while
    /// the loader read another, and materializations would quietly land in
    /// history.
    pub const TREE_NAME: &'static str = "remotes";

    /// The `type` tag this remote was written with.
    ///
    /// A diagnostic about the wrong kind of remote has to say which kind was
    /// declared, and a `&'static str` is the whole of what it needs, so the
    /// error carries this rather than a copy of the record.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Git(_) => "git",
            Self::File(_) => "file",
            Self::Archive(_) => "archive",
        }
    }
}

/// A Git repository, which an `include-remote` action may also include.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GitRemote {
    pub url: String,
    /// A branch name. Tags and commit pins are deliberately not part of the
    /// remote schema.
    pub branch: Option<String>,
    /// Whether this remote's dynamic variable declarations may run. Off by
    /// default: including a repository should not by itself let it execute
    /// commands.
    #[serde(default)]
    pub allow_dynamic_vars: bool,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
}

/// A single fetched file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct FileRemote {
    /// An `https://`, `http://`, or `file://` URL.
    pub url: String,
    /// The expected digest of the fetched bytes, as 64 hexadecimal digits.
    pub sha256: Option<String>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
}

/// A fetched archive, unpacked into the remote's materialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct ArchiveRemote {
    pub url: String,
    pub sha256: Option<String>,
    /// An archive path prefix to strip, or `"*"` to detect a single root.
    pub archive_root: Option<String>,
    pub include: Option<GlobFilter>,
    pub exclude: Option<GlobFilter>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn parse(document: &str) -> Result<BTreeMap<String, Remote>, toml::de::Error> {
        toml::from_str(document)
    }

    fn parse_one(document: &str) -> Remote {
        parse(document)
            .expect("the remote should parse")
            .remove("r")
            .expect("declared")
    }

    #[test]
    fn the_type_tag_selects_the_variant() {
        assert_eq!(
            parse_one("[r]\ntype = 'git'\nurl = 'u'\nbranch = 'main'\nallow-dynamic-vars = true\n"),
            Remote::Git(GitRemote {
                url: "u".to_owned(),
                branch: Some("main".to_owned()),
                allow_dynamic_vars: true,
                when: None,
                unless: None,
            })
        );
        assert!(matches!(
            parse_one("[r]\ntype = 'file'\nurl = 'u'\n"),
            Remote::File(_)
        ));
        assert!(matches!(
            parse_one("[r]\ntype = 'archive'\nurl = 'u'\narchive-root = '*'\n"),
            Remote::Archive(_)
        ));
    }

    #[test]
    fn every_variant_names_its_tag() {
        assert_eq!(parse_one("[r]\ntype = 'git'\nurl = 'u'\n").kind(), "git");
        assert_eq!(parse_one("[r]\ntype = 'file'\nurl = 'u'\n").kind(), "file");
        assert_eq!(
            parse_one("[r]\ntype = 'archive'\nurl = 'u'\n").kind(),
            "archive"
        );
    }

    #[test]
    fn dynamic_variables_are_not_executable_by_default() {
        let Remote::Git(git) = parse_one("[r]\ntype = 'git'\nurl = 'u'\n") else {
            unreachable!()
        };
        assert!(!git.allow_dynamic_vars);
    }

    #[test]
    fn a_remote_may_be_conditional() {
        let Remote::File(file) = parse_one("[r]\ntype = 'file'\nurl = 'u'\nwhen = 'work'\n") else {
            unreachable!()
        };
        assert_eq!(file.when.as_deref(), Some("work"));
    }

    #[test]
    fn an_archive_filter_takes_one_glob_or_many() {
        let Remote::Archive(archive) =
            parse_one("[r]\ntype = 'archive'\nurl = 'u'\ninclude = 'bin/*'\nexclude = ['*.md']\n")
        else {
            unreachable!()
        };
        assert_eq!(archive.include, Some(GlobFilter::from_iter(["bin/*"])));
        assert_eq!(archive.exclude, Some(GlobFilter::from_iter(["*.md"])));
    }

    #[test]
    fn an_unknown_type_is_rejected() {
        let error = parse("[r]\ntype = 'svn'\nurl = 'u'\n").expect_err("there is no svn remote");
        assert!(
            error.to_string().contains("unknown variant `svn`"),
            "{error}"
        );
    }

    #[test]
    fn an_unknown_field_is_rejected() {
        let error =
            parse("[r]\ntype = 'git'\nurl = 'u'\ntag = 'v1'\n").expect_err("closed records");
        assert!(error.to_string().contains("unknown field `tag`"), "{error}");
    }

    #[test]
    fn a_field_belonging_to_another_variant_is_rejected() {
        let error = parse("[r]\ntype = 'git'\nurl = 'u'\nsha256 = 'abc'\n")
            .expect_err("a git remote has no digest");
        assert!(
            error.to_string().contains("unknown field `sha256`"),
            "{error}"
        );
    }

    #[test]
    fn a_url_is_required() {
        let error = parse("[r]\ntype = 'git'\n").expect_err("no url");
        assert!(error.to_string().contains("missing field `url`"), "{error}");
    }
}
