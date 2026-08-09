//! `[[actions]]`: the ordered, heterogeneous list of things a repository does.
//!
//! Each action is one closed record selected by its `type` tag. Every variant
//! repeats the four common fields — `id`, `when`, `unless`, `group` — instead of
//! sharing a flattened record, because `#[serde(flatten)]` silently disables
//! `deny_unknown_fields`, and a closed record is exactly what the format
//! promises.
//!
//! Records here mirror the file. Constraints that span fields — a `symlink`
//! being in single *or* directory mode, `include-remote`'s allowed combinations
//! of selection fields, `when` excluding `unless` — parse into these types
//! unchecked and are rejected by validation.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::item::ItemId;
use crate::repo::value::{Condition, GlobFilter, ItemIdList, RepoPath};
use crate::var::VarName;

/// One entry of `[[actions]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum Action {
    Symlink(SymlinkAction),
    Copy(CopyAction),
    CreateDir(CreateDirAction),
    GitCloneList(GitCloneListAction),
    GitClone(GitCloneAction),
    FetchUrl(FetchUrlAction),
    IncludeRemote(IncludeRemoteAction),
}

/// `symlink`: one symlink, or a shallow set of them.
///
/// The two modes share one record: single mode uses `source`/`dest`, directory
/// mode uses `source-dir`/`dest-dir` and the filters. Exactly one mode is valid,
/// which validation decides.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct SymlinkAction {
    pub id: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    pub group: Option<ItemId>,
    /// Single mode: the source file, symlink, or directory.
    pub source: Option<RepoPath>,
    /// Single mode: the exact destination path.
    pub dest: Option<String>,
    /// Directory mode: the directory whose direct children are selected.
    pub source_dir: Option<RepoPath>,
    /// Directory mode: where those children are linked.
    pub dest_dir: Option<String>,
    /// Directory mode: direct child names to include.
    pub include: Option<GlobFilter>,
    /// Directory mode: direct child names to exclude.
    pub exclude: Option<GlobFilter>,
    /// Directory mode: prefix the first destination segment with `.`.
    #[serde(default)]
    pub dot_prefix: bool,
}

/// `copy`: a missing-only seed of a file-like item or a directory's contents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CopyAction {
    pub id: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    pub group: Option<ItemId>,
    pub source: RepoPath,
    /// The exact destination for a file-like source, or the destination root
    /// for a directory source.
    pub dest: String,
    /// Recursive selection, for a directory source.
    pub include: Option<GlobFilter>,
    /// Recursive exclusion, for a directory source.
    pub exclude: Option<GlobFilter>,
    #[serde(default)]
    pub dot_prefix: bool,
}

/// `create-dir`: create one directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CreateDirAction {
    pub id: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    pub group: Option<ItemId>,
    pub dest: String,
}

/// `git-clone-list`: clone every entry of a line-oriented manifest below one
/// directory.
///
/// The manifest itself is not TOML and is not read while planning; this record
/// only says where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GitCloneListAction {
    /// Required only when individual manifest entries need qualified addresses.
    pub id: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    pub group: Option<ItemId>,
    /// The manifest file.
    pub source: RepoPath,
    /// The parent directory for the derived clone destinations.
    pub dest: String,
}

/// `git-clone`: clone one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GitCloneAction {
    pub id: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    pub group: Option<ItemId>,
    /// A literal Git URL — not a [`RepoPath`].
    pub source: String,
    pub dest: String,
    /// A branch, tag, or commit selector.
    pub r#ref: Option<String>,
}

/// `fetch-url`: a missing-only seed that fetches a file or extracts an archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct FetchUrlAction {
    pub id: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    pub group: Option<ItemId>,
    /// An `https://`, `http://`, or `file://` URL.
    pub source: String,
    /// The file destination, or the destination directory when extracting.
    pub dest: String,
    #[serde(default)]
    pub extract: bool,
    /// The expected digest of the fetched bytes, as 64 hexadecimal digits.
    pub sha256: Option<String>,
    /// An archive prefix to strip, or `"*"` to detect a single root.
    pub archive_root: Option<String>,
    pub include: Option<GlobFilter>,
    pub exclude: Option<GlobFilter>,
}

/// `include-remote`: splice a Git remote's actions in at this position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct IncludeRemoteAction {
    /// The prefix that makes included actions, groups, and manifest entries
    /// addressable as `<id>.<name>`. It need not match `remote`.
    pub id: Option<ItemId>,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
    pub group: Option<ItemId>,
    /// The declared Git remote to include.
    pub remote: ItemId,
    /// Each selection field is absent, one ID, or a list of IDs. Absent is not
    /// the same as empty — with none of the four present, every action in the
    /// remote is selected — so each stays an `Option`.
    pub install_actions: Option<ItemIdList>,
    pub install_groups: Option<ItemIdList>,
    pub exclude_actions: Option<ItemIdList>,
    pub exclude_groups: Option<ItemIdList>,
    /// Per-inclusion variable overrides.
    #[serde(default)]
    pub vars: BTreeMap<VarName, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::value::RemotePath;

    fn id(id: &str) -> ItemId {
        ItemId::new(id).expect("valid id")
    }

    fn parse(document: &str) -> Result<Vec<Action>, toml::de::Error> {
        #[derive(Deserialize)]
        struct Document {
            #[serde(default)]
            actions: Vec<Action>,
        }
        toml::from_str::<Document>(document).map(|document| document.actions)
    }

    fn parse_one(document: &str) -> Action {
        let mut actions = parse(document).expect("the action should parse");
        assert_eq!(actions.len(), 1);
        actions.remove(0)
    }

    #[test]
    fn the_type_tag_selects_the_variant() {
        let actions = parse(
            r#"
[[actions]]
type = "symlink"
source = "shell/zshrc"
dest = "~/.zshrc"

[[actions]]
type = "copy"
source = "local-files"
dest = "~"

[[actions]]
type = "create-dir"
dest = "~/.config"

[[actions]]
type = "git-clone-list"
id = "zsh-plugins"
source = "manifests/zsh-plugins.txt"
dest = "~/.local/share/zsh-plugins"

[[actions]]
type = "git-clone"
source = "https://github.com/ohmyzsh/ohmyzsh.git"
dest = "~/.oh-my-zsh"
ref = "refs/heads/master"

[[actions]]
type = "fetch-url"
source = "https://example.com/pathogen.vim"
dest = "~/.vim/autoload/pathogen.vim"

[[actions]]
type = "include-remote"
remote = "core"
"#,
        )
        .expect("every variant should parse");

        assert!(matches!(
            actions.as_slice(),
            [
                Action::Symlink(_),
                Action::Copy(_),
                Action::CreateDir(_),
                Action::GitCloneList(_),
                Action::GitClone(_),
                Action::FetchUrl(_),
                Action::IncludeRemote(_),
            ]
        ));
    }

    #[test]
    fn the_ordered_list_keeps_its_order() {
        let actions = parse(
            r#"
[[actions]]
type = "create-dir"
dest = "first"

[[actions]]
type = "create-dir"
dest = "second"
"#,
        )
        .expect("parse");
        let dests: Vec<_> = actions
            .iter()
            .map(|action| match action {
                Action::CreateDir(create) => create.dest.as_str(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(dests, ["first", "second"]);
    }

    #[test]
    fn an_unknown_type_names_the_variants_that_exist() {
        let error =
            parse("[[actions]]\ntype = 'teleport'\ndest = 'x'\n").expect_err("no such action");
        assert!(
            error.to_string().contains("unknown variant `teleport`"),
            "{error}"
        );
    }

    #[test]
    fn the_type_tag_is_not_itself_an_unknown_field() {
        // `deny_unknown_fields` on a variant of an internally tagged enum must
        // not reject the tag that selected it.
        parse("[[actions]]\ntype = 'create-dir'\ndest = '~/.config'\n").expect("parse");
    }

    #[test]
    fn an_unknown_action_field_is_rejected() {
        let error = parse("[[actions]]\ntype = 'create-dir'\ndest = 'x'\nmode = '0755'\n")
            .expect_err("records are closed");
        assert!(
            error.to_string().contains("unknown field `mode`"),
            "{error}"
        );
    }

    #[test]
    fn a_field_belonging_to_another_variant_is_rejected() {
        let error = parse("[[actions]]\ntype = 'create-dir'\ndest = 'x'\nsource = 'y'\n")
            .expect_err("create-dir has no source");
        assert!(
            error.to_string().contains("unknown field `source`"),
            "{error}"
        );
    }

    #[test]
    fn kebab_case_is_the_spelling_on_disk() {
        let action = parse_one(
            r#"
[[actions]]
type = "symlink"
source-dir = "files"
dest-dir = "~"
dot-prefix = true
"#,
        );
        let Action::Symlink(symlink) = action else {
            unreachable!()
        };
        assert_eq!(
            symlink.source_dir,
            Some(RepoPath::Relative("files".to_owned()))
        );
        assert_eq!(symlink.dest_dir.as_deref(), Some("~"));
        assert!(symlink.dot_prefix);
    }

    #[test]
    fn both_symlink_modes_parse_into_the_one_record() {
        // Which fields are set says which mode was written; rejecting a mix is
        // validation's job, not parsing's.
        let Action::Symlink(single) =
            parse_one("[[actions]]\ntype = 'symlink'\nsource = 'a'\ndest = 'b'\n")
        else {
            unreachable!()
        };
        assert!(single.source.is_some() && single.source_dir.is_none());

        let Action::Symlink(mixed) = parse_one(
            "[[actions]]\ntype = 'symlink'\nsource = 'a'\ndest = 'b'\nsource-dir = 'c'\n",
        ) else {
            unreachable!()
        };
        assert!(mixed.source.is_some() && mixed.source_dir.is_some());
    }

    #[test]
    fn the_common_fields_are_available_on_every_variant() {
        let Action::CreateDir(create) = parse_one(
            r#"
[[actions]]
type = "create-dir"
id = "config-dir"
group = "shell"
when = "work && facts.os == 'darwin'"
dest = "~/.config"
"#,
        ) else {
            unreachable!()
        };
        assert_eq!(create.id, Some(id("config-dir")));
        assert_eq!(create.group, Some(id("shell")));
        assert_eq!(create.when.as_deref(), Some("work && facts.os == 'darwin'"));
        assert_eq!(create.unless, None);
    }

    #[test]
    fn unless_is_accepted_wherever_when_is() {
        let Action::CreateDir(create) =
            parse_one("[[actions]]\ntype = 'create-dir'\ndest = 'x'\nunless = 'work'\n")
        else {
            unreachable!()
        };
        assert_eq!(create.unless.as_deref(), Some("work"));

        // Both at once is invalid, but that is a rule about the pair rather than
        // about either field, so it parses here.
        let Action::CreateDir(both) =
            parse_one("[[actions]]\ntype = 'create-dir'\ndest = 'x'\nwhen = 'a'\nunless = 'b'\n")
        else {
            unreachable!()
        };
        assert!(both.when.is_some() && both.unless.is_some());
    }

    #[test]
    fn a_git_clone_source_is_a_url_rather_than_a_repository_path() {
        let error = parse(
            "[[actions]]\ntype = 'git-clone'\nsource = { remote = 'core', path = 'x' }\ndest = 'y'\n",
        )
        .expect_err("git-clone takes a literal URL");
        assert!(error.to_string().contains("invalid type"), "{error}");
    }

    #[test]
    fn an_include_remote_carries_its_selections_and_overrides() {
        let Action::IncludeRemote(include) = parse_one(
            r#"
[[actions]]
type = "include-remote"
id = "core"
remote = "core"
install-groups = ["editor"]
exclude-actions = "p10k"
vars = { profile = "personal" }
"#,
        ) else {
            unreachable!()
        };
        assert_eq!(include.remote, id("core"));
        assert_eq!(
            include.install_groups,
            Some(ItemIdList::from_iter([id("editor")]))
        );
        // A bare string is the one-item list.
        assert_eq!(
            include.exclude_actions,
            Some(ItemIdList::from_iter([id("p10k")]))
        );
        assert_eq!(include.install_actions, None);
        assert_eq!(
            include.vars.get(&VarName::new("profile").expect("valid")),
            Some(&"personal".to_owned())
        );
    }

    #[test]
    fn an_absent_selection_differs_from_an_empty_one() {
        let Action::IncludeRemote(empty) = parse_one(
            "[[actions]]\ntype = 'include-remote'\nremote = 'core'\ninstall-actions = []\n",
        ) else {
            unreachable!()
        };
        assert_eq!(empty.install_actions, Some(ItemIdList::default()));
    }

    #[test]
    fn a_selection_list_holds_ids_rather_than_addresses() {
        // The selection fields name unqualified IDs inside the remote, so a
        // dotted address is a mistake rather than a deeper selection.
        let error = parse(
            "[[actions]]\ntype = 'include-remote'\nremote = 'core'\ninstall-actions = ['core.p10k']\n",
        )
        .expect_err("dots are not part of an ID");
        assert!(error.to_string().contains("`core.p10k`"), "{error}");
    }

    #[test]
    fn a_common_id_or_group_must_be_a_valid_id() {
        for field in ["id", "group"] {
            let error = parse(&format!(
                "[[actions]]\ntype = 'create-dir'\ndest = 'x'\n{field} = '_hidden'\n"
            ))
            .expect_err("an ID cannot start with an underscore");
            assert!(error.to_string().contains("`_hidden`"), "{error}");
        }
    }

    #[test]
    fn a_remote_source_reaches_the_action_in_either_spelling() {
        let expected = RepoPath::Remote(RemotePath {
            remote: id("core"),
            path: "files".to_owned(),
        });

        let Action::Copy(structured) = parse_one(
            "[[actions]]\ntype = 'copy'\nsource = { remote = 'core', path = 'files' }\ndest = '~'\n",
        ) else {
            unreachable!()
        };
        assert_eq!(structured.source, expected);

        let Action::Copy(shorthand) =
            parse_one("[[actions]]\ntype = 'copy'\nsource = '@core/files'\ndest = '~'\n")
        else {
            unreachable!()
        };
        assert_eq!(shorthand.source, expected);
    }

    #[test]
    fn ref_survives_being_a_rust_keyword() {
        let Action::GitClone(clone) = parse_one(
            "[[actions]]\ntype = 'git-clone'\nsource = 'https://example.com/x.git'\ndest = 'y'\nref = 'master'\n",
        ) else {
            unreachable!()
        };
        assert_eq!(clone.r#ref.as_deref(), Some("master"));
    }
}
