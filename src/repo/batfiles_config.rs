//! `batfiles.toml`: the one file in a repository with intrinsic meaning.
//!
//! The same type reads a leaf and a remote repository. They differ in what is
//! *allowed*, not in shape: a remote may not use remote path references, and its
//! `[default-disabled]` is structurally valid but ignored. Both are validation
//! rules, so one record serves both, and the caller — which knows which
//! repository it is reading — applies them.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::item::ItemId;
use crate::repo::action::Action;
use crate::repo::default_disabled::DefaultDisabled;
use crate::repo::remote::Remote;
use crate::repo::value::Condition;
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
    /// remote's ID, so it is validated as one while the document is read.
    #[serde(default)]
    pub remotes: BTreeMap<ItemId, Remote>,
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
    ///
    /// Every manifest batfiles reads comes off disk through [`load`](Self::load),
    /// so this is the seam the schema tests use, and the one an in-memory caller
    /// would reach for if one appeared.
    #[allow(dead_code, reason = "the schema tests are the only caller so far")]
    pub fn parse(document: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(document)
    }

    /// Check the cross-record rules that hold for every repository.
    ///
    /// Two rules so far. Action IDs share a single namespace within a repository
    /// (`docs/repoformat.md`), so a repeated ID makes addressing and diagnostics
    /// ambiguous and is rejected before anything interprets the actions. And
    /// `when` excludes `unless` on every record that accepts the pair, which is
    /// a rule about two fields of one record rather than about either field, so
    /// it cannot be a deserialize-time check. The rules that depend on whether
    /// this is a leaf or a remote, or on what is on disk, stay with the code
    /// that knows those things.
    ///
    /// **One problem at a time.** The first offense returns, so a manifest with
    /// both a duplicate ID and a doubled condition reports the duplicate. That
    /// is existing behavior rather than a choice being remade here.
    ///
    /// Nothing here touches the filesystem, so the error names positions rather
    /// than a file; the caller that read the manifest supplies its path.
    pub fn validate(&self) -> Result<(), ValidationError> {
        let mut seen: BTreeMap<&ItemId, usize> = BTreeMap::new();
        for (position, action) in self.actions.iter().enumerate() {
            let Some(id) = action.id() else { continue };
            if let Some(&first_position) = seen.get(id) {
                return Err(ValidationError::DuplicateActionId {
                    id: id.clone(),
                    first_position,
                    duplicate_position: position,
                });
            }
            seen.insert(id, position);
        }

        // Every record in the manifest that accepts the pair, so a record kind
        // added later without a line here is a gap a test is meant to catch.
        for (position, action) in self.actions.iter().enumerate() {
            check_exclusive(action.conditions(), || ConditionSite::Action(position))?;
        }
        for (id, remote) in &self.remotes {
            check_exclusive(remote.conditions(), || ConditionSite::Remote(id.clone()))?;
        }
        for (position, entry) in self.default_disabled.actions.iter().enumerate() {
            check_exclusive((entry.when.as_ref(), entry.unless.as_ref()), || {
                ConditionSite::DefaultDisabledAction(position)
            })?;
        }
        for (position, entry) in self.default_disabled.groups.iter().enumerate() {
            check_exclusive((entry.when.as_ref(), entry.unless.as_ref()), || {
                ConditionSite::DefaultDisabledGroup(position)
            })?;
        }

        Ok(())
    }
}

/// Reject one record that wrote both `when` and `unless`.
///
/// The site is built lazily because it can own an [`ItemId`], and the
/// overwhelmingly common case is that nothing is wrong.
fn check_exclusive(
    conditions: (Option<&Condition>, Option<&Condition>),
    site: impl FnOnce() -> ConditionSite,
) -> Result<(), ValidationError> {
    match conditions {
        (Some(_), Some(_)) => Err(ValidationError::BothWhenAndUnless { site: site() }),
        _ => Ok(()),
    }
}

/// A manifest that parsed but broke a rule spanning its records.
///
/// Positions are zero-based indices into `actions`, matching how the loader
/// indexes them, and are rendered one-based.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ValidationError {
    /// Two actions were written with the same `id`.
    DuplicateActionId {
        id: ItemId,
        first_position: usize,
        duplicate_position: usize,
    },
    /// One record wrote `when` and `unless` together
    /// (`docs/repoformat.md`), which are aliases of one gate rather than two.
    BothWhenAndUnless { site: ConditionSite },
}

/// Which record broke a rule about conditions.
///
/// Positional for the records the format identifies by order, and by ID for a
/// remote, whose map key *is* its ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ConditionSite {
    Action(usize),
    Remote(ItemId),
    DefaultDisabledAction(usize),
    DefaultDisabledGroup(usize),
}

impl fmt::Display for ConditionSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Action(position) => write!(f, "action #{}", position + 1),
            Self::Remote(id) => write!(f, "remote `{id}`"),
            Self::DefaultDisabledAction(position) => {
                write!(f, "default-disabled action #{}", position + 1)
            }
            Self::DefaultDisabledGroup(position) => {
                write!(f, "default-disabled group #{}", position + 1)
            }
        }
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateActionId {
                id,
                first_position,
                duplicate_position,
            } => write!(
                f,
                "action ID `{id}` is used by actions #{} and #{}; action IDs must be unique",
                first_position + 1,
                duplicate_position + 1
            ),
            Self::BothWhenAndUnless { site } => {
                write!(f, "{site} may contain `when` or `unless` but not both")
            }
        }
    }
}

impl std::error::Error for ValidationError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::remote::GitRemote;

    fn id(value: &str) -> ItemId {
        ItemId::new(value).expect("valid id")
    }

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
unless = "facts.os == 'macos'"

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
    fn a_remote_key_is_user_data_that_still_obeys_the_id_rule() {
        // A remote's map key *is* its ID. Being user data rather than a schema
        // field exempts it from `deny_unknown_fields`, not from the name rule.
        let error = BatfilesConfig::parse("[remotes.'my remote']\ntype = 'file'\nurl = 'u'\n")
            .expect_err("a remote ID cannot contain a space");
        assert!(error.to_string().contains("`my remote`"), "{error}");

        let config = BatfilesConfig::parse("[remotes.oh-my-zsh]\ntype = 'file'\nurl = 'u'\n")
            .expect("a valid ID is still just user data");
        assert!(config.remotes.contains_key(&id("oh-my-zsh")));
        assert!(
            toml::to_string(&config)
                .expect("serialize")
                .contains("[remotes.oh-my-zsh]")
        );
    }

    #[test]
    fn the_sections_reach_the_records_that_own_them() {
        let config = BatfilesConfig::parse(EXAMPLE).expect("parse");
        assert!(matches!(
            config.remotes[&id("core")],
            Remote::Git(GitRemote { .. })
        ));
        assert_eq!(
            config.default_disabled.groups[0]
                .unless
                .as_ref()
                .map(Condition::source),
            Some("facts.os == 'macos'")
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
    fn the_example_manifest_is_valid() {
        BatfilesConfig::parse(EXAMPLE)
            .expect("parse")
            .validate()
            .expect("the example breaks no cross-record rule");
    }

    #[test]
    fn action_ids_share_one_namespace_across_the_variants() {
        // The duplicate spans two different action types, which is what makes
        // this the repository-wide namespace rather than a per-variant check.
        let config = BatfilesConfig::parse(
            r#"
[[actions]]
type = "create-dir"
dest = "~/.config"

[[actions]]
type = "include-remote"
id = "core"
remote = "core"

[[actions]]
type = "symlink"
id = "core"
source = "shell/zshrc"
dest = "~/.zshrc"
"#,
        )
        .expect("a duplicate ID is still syntactically valid");

        assert_eq!(
            config.validate().expect_err("duplicate"),
            ValidationError::DuplicateActionId {
                id: id("core"),
                first_position: 1,
                duplicate_position: 2,
            }
        );
    }

    #[test]
    fn an_id_less_action_never_collides_with_another() {
        let config = BatfilesConfig::parse(
            "[[actions]]\ntype = 'create-dir'\ndest = 'a'\n\n\
             [[actions]]\ntype = 'create-dir'\ndest = 'b'\n",
        )
        .expect("parse");
        assert!(config.validate().is_ok());
    }

    #[test]
    fn a_duplicate_action_id_renders_one_based_positions() {
        assert_eq!(
            ValidationError::DuplicateActionId {
                id: id("core"),
                first_position: 1,
                duplicate_position: 4,
            }
            .to_string(),
            "action ID `core` is used by actions #2 and #5; action IDs must be unique"
        );
    }

    #[test]
    fn when_excludes_unless_on_every_record_that_accepts_the_pair() {
        // Four record kinds, so the walk is proven to reach all four rather
        // than only the one that was written first.
        let cases = [
            (
                "[[actions]]\ntype = 'create-dir'\ndest = 'x'\nwhen = 'a'\nunless = 'b'\n",
                ConditionSite::Action(0),
            ),
            (
                "[remotes.core]\ntype = 'git'\nurl = 'u'\nwhen = 'a'\nunless = 'b'\n",
                ConditionSite::Remote(id("core")),
            ),
            (
                "[[default-disabled.actions]]\nid = 'p10k'\nwhen = 'a'\nunless = 'b'\n",
                ConditionSite::DefaultDisabledAction(0),
            ),
            (
                "[[default-disabled.groups]]\ngroup = 'gui'\nwhen = 'a'\nunless = 'b'\n",
                ConditionSite::DefaultDisabledGroup(0),
            ),
        ];

        for (document, site) in cases {
            let config = BatfilesConfig::parse(document)
                .expect("the pair is a rule about two fields, so it still parses");
            assert_eq!(
                config.validate().expect_err("both were written"),
                ValidationError::BothWhenAndUnless { site },
                "{document}"
            );
        }
    }

    #[test]
    fn either_condition_alone_is_fine_everywhere() {
        for field in ["when", "unless"] {
            let document = format!(
                "[remotes.core]\ntype = 'git'\nurl = 'u'\n{field} = 'work'\n\n\
                 [[actions]]\ntype = 'create-dir'\ndest = 'x'\n{field} = 'work'\n\n\
                 [[default-disabled.actions]]\nid = 'p10k'\n{field} = 'work'\n\n\
                 [[default-disabled.groups]]\ngroup = 'gui'\n{field} = 'work'\n"
            );
            BatfilesConfig::parse(&document)
                .expect("parse")
                .validate()
                .unwrap_or_else(|error| panic!("{field}: {error}"));
        }
    }

    #[test]
    fn a_doubled_condition_names_the_record_it_is_on() {
        // Positional records render one-based, as the duplicate-ID rule does; a
        // remote renders by ID, because its map key is its ID.
        assert_eq!(
            ValidationError::BothWhenAndUnless {
                site: ConditionSite::Action(3),
            }
            .to_string(),
            "action #4 may contain `when` or `unless` but not both"
        );
        assert!(
            ValidationError::BothWhenAndUnless {
                site: ConditionSite::Remote(id("core")),
            }
            .to_string()
            .starts_with("remote `core` may contain")
        );
        assert!(
            ValidationError::BothWhenAndUnless {
                site: ConditionSite::DefaultDisabledAction(0),
            }
            .to_string()
            .starts_with("default-disabled action #1 may contain")
        );
        assert!(
            ValidationError::BothWhenAndUnless {
                site: ConditionSite::DefaultDisabledGroup(1),
            }
            .to_string()
            .starts_with("default-disabled group #2 may contain")
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
