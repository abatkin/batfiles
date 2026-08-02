//! `batfiles.toml`: the one file in a repository with intrinsic meaning.
//!
//! The same type reads a leaf and a remote repository. They differ in what is
//! *allowed*, not in shape: a remote may not use remote path references, and its
//! `[default-disabled]` is structurally valid but ignored. Both are validation
//! rules, so one record serves both, and the caller — which knows which
//! repository it is reading — applies them.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::repo::action::Action;
use crate::repo::default_disabled::DefaultDisabled;
use crate::repo::remote::Remote;
use crate::repo::var_decl::VarDecl;
use crate::tomlfile;
use crate::var::VarName;

/// A parsed `batfiles.toml`.
///
/// Every section is optional, and there is no format-version field. The document
/// is a closed record: an unknown top-level key is a configuration error rather
/// than something to ignore.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct BatfilesConfig {
    /// Named sources this repository may materialize. The map key is the
    /// remote's ID.
    #[serde(default)]
    pub remotes: BTreeMap<String, Remote>,
    /// Repository variable declarations.
    #[serde(default)]
    pub vars: BTreeMap<VarName, VarDecl>,
    /// Bootstrap-time disabled candidates, honored only in a leaf repository.
    #[serde(default)]
    pub default_disabled: DefaultDisabled,
    /// The ordered action list. Order is significant, so this is the one
    /// top-level section that is a sequence rather than a map.
    #[serde(default)]
    pub actions: Vec<Action>,
}

impl BatfilesConfig {
    /// The manifest's name within a repository root.
    ///
    /// The name travels with the parser, but *where* a repository is does not:
    /// a leaf comes from the resolved roots and a remote from its
    /// materialization, so callers pass a full path to [`load`](Self::load).
    pub const FILE_NAME: &'static str = "batfiles.toml";

    /// Load and parse a manifest.
    ///
    /// A missing file is reported like any other read failure
    /// ([`tomlfile::Error::is_not_found`]). Whether that is fatal depends on the
    /// repository: a leaf without a manifest is not a batfiles repository, while
    /// a remote's manifest is optional.
    pub fn load(path: &Path) -> Result<Self, tomlfile::Error> {
        tomlfile::read(path)
    }

    /// Parse a manifest already in memory.
    pub fn parse(document: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(document)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::remote::GitRemote;

    /// A manifest exercising every top-level section at once.
    const EXAMPLE: &str = r#"
[remotes.core]
type = "git"
url = "git@github.com:me/dotfiles-core.git"
branch = "main"
when = "facts.os != 'windows'"
allow-dynamic-vars = true

[remotes.pathogen]
type = "file"
url = "https://example.com/pathogen.vim"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

[remotes.fzf]
type = "archive"
url = "https://example.com/fzf.tar.gz"
archive-root = "*"
include = ["bin/*"]
exclude = "*.md"

[vars]
work = "false"
email = { command = ["git", "config", "user.email"], cache = "24h" }

[[default-disabled.actions]]
id = "core.work-tools"
when = "work"

[[default-disabled.groups]]
group = "gui"
unless = "facts.os == 'darwin'"

[[actions]]
type = "include-remote"
id = "core"
remote = "core"

[[actions]]
type = "symlink"
source = "shell/zshrc"
dest = "~/.zshrc"
"#;

    #[test]
    fn every_section_parses() {
        let config = BatfilesConfig::parse(EXAMPLE).expect("the example should parse");

        assert_eq!(config.remotes.len(), 3);
        assert_eq!(config.vars.len(), 2);
        assert_eq!(config.actions.len(), 2);
        assert_eq!(config.default_disabled.actions.len(), 1);
        assert_eq!(config.default_disabled.groups.len(), 1);
    }

    #[test]
    fn an_empty_manifest_is_valid() {
        assert_eq!(
            BatfilesConfig::parse("").expect("empty"),
            BatfilesConfig::default()
        );
    }

    #[test]
    fn an_unknown_top_level_section_is_rejected() {
        let error = BatfilesConfig::parse("version = 1\n").expect_err("there is no version field");
        assert!(
            error.to_string().contains("unknown field `version`"),
            "{error}"
        );
    }

    #[test]
    fn a_remote_key_is_data_rather_than_a_schema_field() {
        // Anything can name a remote; only its value has to match a known shape.
        let config = BatfilesConfig::parse("[remotes.'my remote']\ntype = 'file'\nurl = 'u'\n")
            .expect("map keys are user data");
        assert!(config.remotes.contains_key("my remote"));
    }

    #[test]
    fn the_sections_reach_the_records_that_own_them() {
        let config = BatfilesConfig::parse(EXAMPLE).expect("parse");
        assert!(matches!(
            config.remotes["core"],
            Remote::Git(GitRemote { .. })
        ));
        assert_eq!(
            config.default_disabled.groups[0].unless.as_deref(),
            Some("facts.os == 'darwin'")
        );
    }

    #[test]
    fn a_manifest_round_trips_through_serialization() {
        let config = BatfilesConfig::parse(EXAMPLE).expect("parse");
        let document = toml::to_string(&config).expect("serialize");
        assert_eq!(BatfilesConfig::parse(&document).expect("reparse"), config);
    }

    #[test]
    fn loading_reads_a_manifest_from_the_path_it_is_given() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(BatfilesConfig::FILE_NAME);
        std::fs::write(&path, EXAMPLE).expect("fixture");

        assert_eq!(
            BatfilesConfig::load(&path).expect("load"),
            BatfilesConfig::parse(EXAMPLE).expect("parse")
        );
    }

    #[test]
    fn a_missing_manifest_is_reported_as_such() {
        let dir = tempfile::tempdir().expect("temp dir");
        let error = BatfilesConfig::load(&dir.path().join(BatfilesConfig::FILE_NAME))
            .expect_err("no manifest");
        assert!(error.is_not_found());
        assert!(error.to_string().contains("batfiles.toml"));
    }
}
