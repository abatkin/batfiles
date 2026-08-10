//! Reading a repository off disk into the [model](super::model).
//!
//! Strictly read-only: nothing is fetched, materialized, or created, and the
//! loader prints nothing and takes no [`Reporter`](crate::output::Reporter).
//! Severity belongs to the caller, and there is nothing to report anyway —
//! what was found is a value on the model.
//!
//! Discovery is inclusion-driven rather than `[remotes]`-driven: a remote's
//! `batfiles.toml` is read only when an `include-remote` selects it
//! (`docs/repoformat.md`).
//!
//! Reachability *at this level* is structural, and deliberately so: this walk is
//! the **input** to condition evaluation rather than a consumer of it.
//! `when`/`unless` do decide which remotes are ultimately in play, but they are
//! applied one layer up, against the leaf scope that this output is what makes
//! it possible to build — so nothing here could evaluate them without a cycle.
//! `disabled.toml` never participates at all: it decides which actions get
//! planned, not what is reachable.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::config::Roots;
use crate::item::ItemId;
use crate::repo::action::Action;
use crate::repo::batfiles_config::{BatfilesConfig, ValidationError};
use crate::repo::model::{IncludedRemote, Inclusion, Leaf, RemoteState, Repository};
use crate::repo::remote::Remote;
use crate::tomlfile;

/// Load the leaf repository and every remote an `include-remote` selects.
///
/// `roots` supplies the selected leaf repository, its manifest, and its
/// materialization tree, so a caller cannot pair one leaf with another leaf's
/// materializations. Nothing is fetched, materialized, or created.
pub(crate) fn leaf(roots: &Roots) -> Result<Leaf, LoadError> {
    let root = roots.batfiles_dir.clone();
    let config = match manifest(&roots.batfiles_config()) {
        // The leaf is the one repository whose manifest is required, so its
        // absence is a diagnostic about the directory rather than the file.
        Err(LoadError::Manifest(error)) if error.is_not_found() => {
            return Err(LoadError::NotARepository(root));
        }
        other => other?,
    };

    let remotes_dir = roots.remotes_dir();
    let mut included: BTreeMap<ItemId, IncludedRemote> = BTreeMap::new();
    let mut inclusions = Vec::new();
    // How many id-less inclusions of each remote have been labeled so far. An
    // inclusion with an `id` is labeled by it and does not consume a number, so
    // the suffixes of the id-less ones stay contiguous.
    let mut unlabeled: BTreeMap<ItemId, usize> = BTreeMap::new();

    for (position, action) in config.actions.iter().enumerate() {
        let Action::IncludeRemote(include) = action else {
            continue;
        };

        // An inclusion's remote reference is how the loader finds a
        // materialization root and an allow flag, so an unusable reference has
        // no fact to record and fails the load — as it would fail `sync`.
        let declared =
            config
                .remotes
                .get(&include.remote)
                .ok_or_else(|| LoadError::UnknownRemote {
                    position,
                    remote: include.remote.clone(),
                })?;
        let Remote::Git(git) = declared else {
            return Err(LoadError::NotAGitRemote {
                position,
                remote: include.remote.clone(),
                kind: declared.kind(),
            });
        };

        // Read once per declared remote: two inclusions of one remote share a
        // single entry, and therefore a single capture.
        if !included.contains_key(&include.remote) {
            let remote = remote(&remotes_dir, &include.remote, git.allow_dynamic_vars)?;
            included.insert(include.remote.clone(), remote);
        }

        let label = match &include.id {
            Some(id) => id.to_string(),
            None => {
                let occurrence = unlabeled.entry(include.remote.clone()).or_insert(0);
                *occurrence += 1;
                match *occurrence {
                    1 => format!("remote={}", include.remote),
                    nth => format!("remote={} #{nth}", include.remote),
                }
            }
        };

        inclusions.push(Inclusion {
            position,
            id: include.id.clone(),
            remote: include.remote.clone(),
            vars: include.vars.clone(),
            label,
        });
    }

    Ok(Leaf {
        repo: Repository { root, config },
        included,
        inclusions,
    })
}

/// Inspect one declared remote's materialization root and read what is there.
///
/// The root is meaningful even when nothing is at it: it is where `sync` will
/// materialize.
fn remote(
    remotes_dir: &Path,
    id: &ItemId,
    allow_dynamic_vars: bool,
) -> Result<IncludedRemote, LoadError> {
    let root = remotes_dir.join(id.as_ref());

    let (config, state) = match fs::metadata(&root) {
        Ok(metadata) if metadata.is_dir() => {
            match manifest(&root.join(BatfilesConfig::FILE_NAME)) {
                Ok(config) => (config, RemoteState::Present),
                // A remote's manifest is optional: absent means it declares
                // nothing. Anything else keeps its normal classification, because
                // no caller can proceed past a manifest it cannot parse.
                Err(LoadError::Manifest(error)) if error.is_not_found() => {
                    (BatfilesConfig::default(), RemoteState::NoManifest)
                }
                Err(error) => return Err(error),
            }
        }
        // An ordinary file at a remote's root is an anomaly in a tool-owned tree
        // that `sync` will replace, and nothing usable is there in the meantime.
        Ok(_) => (BatfilesConfig::default(), RemoteState::NotMaterialized),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            (BatfilesConfig::default(), RemoteState::NotMaterialized)
        }
        // Being unable to look is not the same as having looked and found
        // nothing, so it must not collapse into that false fact.
        Err(source) => return Err(LoadError::RemoteRoot { path: root, source }),
    };

    Ok(IncludedRemote {
        id: id.clone(),
        allow_dynamic_vars,
        repo: Repository { root, config },
        state,
    })
}

/// Read one manifest and check the rules that span its records.
///
/// Syntax decoding and semantic validation stay adjacent here rather than
/// moving cross-record rules into serde, and every manifest the loader reads —
/// the leaf's and each included remote's — goes through it. Checking a remote's
/// too is deliberate: this step reads no remote action IDs, but the alternative
/// is a `vars list` that succeeds over a manifest `sync` will reject minutes
/// later.
fn manifest(path: &Path) -> Result<BatfilesConfig, LoadError> {
    let config = BatfilesConfig::load(path)?;
    config
        .validate()
        .map_err(|source| LoadError::InvalidManifest {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(config)
}

/// Why a repository could not be loaded.
#[derive(Debug)]
pub(crate) enum LoadError {
    /// No `batfiles.toml` at the leaf root.
    NotARepository(PathBuf),
    /// A manifest — leaf or remote — could not be read or parsed.
    Manifest(tomlfile::Error),
    /// A manifest parsed but violated a cross-record invariant.
    InvalidManifest {
        path: PathBuf,
        source: ValidationError,
    },
    /// A remote materialization root could not be inspected.
    RemoteRoot { path: PathBuf, source: io::Error },
    /// An `include-remote` names a remote the leaf does not declare.
    UnknownRemote { position: usize, remote: ItemId },
    /// An `include-remote` names a declared remote that is not a Git remote.
    NotAGitRemote {
        position: usize,
        remote: ItemId,
        kind: &'static str,
    },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // The diagnostic a user meets when `BATFILES_DIR` or
            // `--batfiles-dir` points at the wrong place, so it names a way out.
            Self::NotARepository(root) => write!(
                f,
                "`{}` is not a batfiles repository: no `{}` there — run `batfiles init` there, \
                 or point `--batfiles-dir` at the right directory",
                root.display(),
                BatfilesConfig::FILE_NAME
            ),
            // The document error already names the file and the line.
            Self::Manifest(error) => error.fmt(f),
            Self::InvalidManifest { path, source } => {
                write!(f, "invalid configuration in {}: {source}", path.display())
            }
            Self::RemoteRoot { path, source } => write!(
                f,
                "could not inspect remote materialization {}: {source}",
                path.display()
            ),
            // Positions render one-based, counting actions as written. They are
            // carried rather than labels because an inclusion can fail before
            // the labeling pass has anything to work with.
            Self::UnknownRemote { position, remote } => write!(
                f,
                "action #{} includes `{remote}`, but the leaf declares no remote named `{remote}`",
                position + 1
            ),
            Self::NotAGitRemote {
                position,
                remote,
                kind,
            } => write!(
                f,
                "action #{} includes `{remote}`, which is a {kind} remote: \
                 only a git remote can be included",
                position + 1
            ),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotARepository(_) | Self::UnknownRemote { .. } | Self::NotAGitRemote { .. } => {
                None
            }
            Self::Manifest(error) => Some(error),
            Self::InvalidManifest { source, .. } => Some(source),
            Self::RemoteRoot { source, .. } => Some(source),
        }
    }
}

impl From<tomlfile::Error> for LoadError {
    fn from(error: tomlfile::Error) -> Self {
        Self::Manifest(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::var::VarName;
    use std::error::Error as _;
    use tempfile::TempDir;

    /// A temporary tree with resolved roots pointing into it.
    ///
    /// The config and cache roots are ordinary paths that never appear on disk:
    /// this loader deliberately does not read them, and a fixture that omitted
    /// them would hide that it could not.
    struct Fixture {
        dir: TempDir,
        roots: Roots,
    }

    impl Fixture {
        /// A leaf repository holding `manifest`.
        fn new(manifest: &str) -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            let base = dir.path();
            let roots = Roots {
                home: base.join("home"),
                batfiles_dir: base.join("dotfiles"),
                config_dir: base.join("config"),
                cache_dir: base.join("cache"),
            };
            fs::create_dir_all(&roots.batfiles_dir).expect("leaf root");
            fs::write(roots.batfiles_config(), manifest).expect("leaf manifest");
            Self { dir, roots }
        }

        /// A directory that is not a batfiles repository at all.
        fn bare() -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            let base = dir.path();
            let roots = Roots {
                home: base.join("home"),
                batfiles_dir: base.join("dotfiles"),
                config_dir: base.join("config"),
                cache_dir: base.join("cache"),
            };
            fs::create_dir_all(&roots.batfiles_dir).expect("leaf root");
            Self { dir, roots }
        }

        /// Materialize `id`, with a `batfiles.toml` when one is given.
        fn materialize(&self, id: &str, manifest: Option<&str>) -> PathBuf {
            let root = self.roots.remotes_dir().join(id);
            fs::create_dir_all(&root).expect("remote root");
            if let Some(manifest) = manifest {
                fs::write(root.join(BatfilesConfig::FILE_NAME), manifest).expect("remote manifest");
            }
            root
        }

        fn load(&self) -> Result<Leaf, LoadError> {
            leaf(&self.roots)
        }

        fn loaded(&self) -> Leaf {
            self.load().expect("the fixture should load")
        }

        fn error(&self) -> LoadError {
            self.load().expect_err("the fixture should fail to load")
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }
    }

    fn id(value: &str) -> ItemId {
        ItemId::new(value).expect("valid id")
    }

    /// A leaf declaring one Git remote, plus whatever actions are appended.
    fn leaf_with(actions: &str) -> String {
        format!("[remotes.core]\ntype = 'git'\nurl = 'git@example.com:me/core.git'\n\n{actions}")
    }

    #[test]
    fn a_directory_without_a_manifest_is_not_a_repository() {
        let fixture = Fixture::bare();
        let error = fixture.error();
        assert!(matches!(error, LoadError::NotARepository(_)));
        let message = error.to_string();
        assert!(
            message.contains(&fixture.roots.batfiles_dir.display().to_string()),
            "{message}"
        );
        assert!(message.contains("batfiles init"), "{message}");
    }

    #[test]
    fn a_broken_repository_is_not_reported_as_an_absent_one() {
        let fixture = Fixture::new("actions = \n");
        let error = fixture.error();
        assert!(matches!(error, LoadError::Manifest(_)));
        let message = error.to_string();
        assert!(message.contains("batfiles.toml"), "{message}");
        assert!(message.contains("line 1"), "{message}");
    }

    #[test]
    fn a_leaf_with_no_inclusions_creates_nothing() {
        let fixture = Fixture::new("[[actions]]\ntype = 'create-dir'\ndest = '~/.config'\n");
        let leaf = fixture.loaded();

        assert_eq!(leaf.repo.root, fixture.roots.batfiles_dir);
        assert!(leaf.included.is_empty());
        assert!(leaf.inclusions.is_empty());
        // The read-only guarantee, asserted rather than assumed.
        assert!(!fixture.roots.remotes_dir().exists());
    }

    #[test]
    fn a_materialized_remote_with_a_manifest_is_present() {
        let fixture = Fixture::new(
            "[remotes.core]\ntype = 'git'\nurl = 'u'\nallow-dynamic-vars = true\n\n\
             [[actions]]\ntype = 'include-remote'\nremote = 'core'\n",
        );
        let root = fixture.materialize("core", Some("[vars]\ntheme = 'dark'\n"));

        let leaf = fixture.loaded();
        let core = &leaf.included[&id("core")];
        assert_eq!(core.state, RemoteState::Present);
        assert_eq!(core.repo.root, root);
        assert!(core.allow_dynamic_vars);
        assert!(
            core.repo
                .config
                .vars
                .contains_key(&VarName::new("theme").expect("valid"))
        );
    }

    #[test]
    fn a_materialized_remote_without_a_manifest_declares_nothing() {
        let fixture = Fixture::new(&leaf_with(
            "[[actions]]\ntype = 'include-remote'\nremote = 'core'\n",
        ));
        fixture.materialize("core", None);

        let core = fixture.loaded().included.remove(&id("core")).expect("core");
        assert_eq!(core.state, RemoteState::NoManifest);
        assert_eq!(core.repo.config, BatfilesConfig::default());
    }

    #[test]
    fn an_unmaterialized_remote_still_knows_where_it_would_go() {
        let manifest = leaf_with("[[actions]]\ntype = 'include-remote'\nremote = 'core'\n");
        let expected_root = |fixture: &Fixture| fixture.roots.remotes_dir().join("core");

        let fixture = Fixture::new(&manifest);
        let core = fixture.loaded().included.remove(&id("core")).expect("core");
        assert_eq!(core.state, RemoteState::NotMaterialized);
        assert_eq!(core.repo.root, expected_root(&fixture));

        // A plain file sitting at that exact root is the same fact: nothing
        // usable is there, and `sync` will replace it.
        let fixture = Fixture::new(&manifest);
        fs::create_dir_all(fixture.roots.remotes_dir()).expect("remotes dir");
        fs::write(expected_root(&fixture), "not a repository").expect("fixture");

        let core = fixture.loaded().included.remove(&id("core")).expect("core");
        assert_eq!(core.state, RemoteState::NotMaterialized);
        assert_eq!(core.repo.root, expected_root(&fixture));
    }

    /// Windows maps a path traversing a file to `ERROR_PATH_NOT_FOUND`, and
    /// therefore to `ErrorKind::NotFound` — the one classification that
    /// legitimately *is* `NotMaterialized` — so the assertion inverts there. The
    /// behavior under test is the `NotFound`-versus-everything-else split, which
    /// one platform can prove.
    #[cfg(unix)]
    #[test]
    fn being_unable_to_inspect_a_root_is_not_the_same_as_finding_nothing() {
        let fixture = Fixture::new(&leaf_with(
            "[[actions]]\ntype = 'include-remote'\nremote = 'core'\n",
        ));
        // A plain file where the materialization tree belongs, so inspecting a
        // child of it fails with something other than "not found".
        fs::write(fixture.roots.remotes_dir(), "not a directory").expect("fixture");

        let error = fixture.error();
        let LoadError::RemoteRoot { path, source } = &error else {
            panic!("expected a RemoteRoot failure, got {error:?}");
        };
        assert_eq!(*path, fixture.roots.remotes_dir().join("core"));
        assert_ne!(source.kind(), io::ErrorKind::NotFound);
        assert!(error.source().is_some());
        assert!(error.to_string().contains("core"), "{error}");
    }

    #[test]
    fn a_malformed_remote_manifest_fails_the_load() {
        let fixture = Fixture::new(&leaf_with(
            "[[actions]]\ntype = 'include-remote'\nremote = 'core'\n",
        ));
        let root = fixture.materialize("core", Some("vars = \n"));

        let error = fixture.error();
        assert!(matches!(error, LoadError::Manifest(_)));
        assert!(
            error
                .to_string()
                .contains(&root.join(BatfilesConfig::FILE_NAME).display().to_string()),
            "{error}"
        );
    }

    #[test]
    fn a_duplicate_action_id_fails_before_any_inclusion_is_processed() {
        // The inclusion below names an undeclared remote, so reaching it would
        // report `UnknownRemote` instead — which is how this asserts ordering.
        let fixture = Fixture::new(
            "[[actions]]\ntype = 'create-dir'\nid = 'core'\ndest = '~/.config'\n\n\
             [[actions]]\ntype = 'include-remote'\nid = 'core'\nremote = 'absent'\n",
        );

        let error = fixture.error();
        let LoadError::InvalidManifest { path, source } = &error else {
            panic!("expected an InvalidManifest failure, got {error:?}");
        };
        assert_eq!(*path, fixture.roots.batfiles_config());
        assert_eq!(
            *source,
            ValidationError::DuplicateActionId {
                id: id("core"),
                first_position: 0,
                duplicate_position: 1,
            }
        );
        let message = error.to_string();
        assert!(message.contains("actions #1 and #2"), "{message}");
    }

    #[test]
    fn an_included_remote_manifest_obeys_the_same_id_rule() {
        let fixture = Fixture::new(&leaf_with(
            "[[actions]]\ntype = 'include-remote'\nremote = 'core'\n",
        ));
        let root = fixture.materialize(
            "core",
            Some(
                "[[actions]]\ntype = 'create-dir'\nid = 'shell'\ndest = 'a'\n\n\
                 [[actions]]\ntype = 'create-dir'\nid = 'shell'\ndest = 'b'\n",
            ),
        );

        let error = fixture.error();
        let LoadError::InvalidManifest { path, .. } = &error else {
            panic!("expected an InvalidManifest failure, got {error:?}");
        };
        assert_eq!(*path, root.join(BatfilesConfig::FILE_NAME));
    }

    #[test]
    fn two_inclusions_of_one_remote_share_a_single_entry() {
        let fixture = Fixture::new(&leaf_with(
            "[[actions]]\ntype = 'include-remote'\nremote = 'core'\n\n\
             [[actions]]\ntype = 'include-remote'\nid = 'again'\nremote = 'core'\n",
        ));
        let leaf = fixture.loaded();

        assert_eq!(leaf.included.len(), 1);
        assert_eq!(leaf.inclusions.len(), 2);
        assert!(
            leaf.inclusions
                .iter()
                .all(|inclusion| inclusion.remote == id("core"))
        );
    }

    #[test]
    fn position_indexes_the_action_list_rather_than_the_inclusions() {
        // The interleaved `create-dir` actions are what an implementation
        // counting inclusions would silently skip over.
        let fixture = Fixture::new(&leaf_with(
            "[[actions]]\ntype = 'create-dir'\ndest = 'a'\n\n\
             [[actions]]\ntype = 'include-remote'\nid = 'first'\nremote = 'core'\n\n\
             [[actions]]\ntype = 'create-dir'\ndest = 'b'\n\n\
             [[actions]]\ntype = 'include-remote'\nid = 'second'\nremote = 'core'\n",
        ));
        let leaf = fixture.loaded();

        let positions: Vec<_> = leaf
            .inclusions
            .iter()
            .map(|inclusion| inclusion.position)
            .collect();
        assert_eq!(positions, [1, 3]);
        let ids: Vec<_> = leaf
            .inclusions
            .iter()
            .map(|inclusion| inclusion.id.clone())
            .collect();
        assert_eq!(ids, [Some(id("first")), Some(id("second"))]);
        assert!(matches!(
            leaf.repo.config.actions[1],
            Action::IncludeRemote(_)
        ));
    }

    #[test]
    fn every_inclusion_gets_a_unique_label() {
        let fixture = Fixture::new(&format!(
            "{}\n{}",
            leaf_with("[[actions]]\ntype = 'include-remote'\nremote = 'core'\n"),
            "[[actions]]\ntype = 'include-remote'\nid = 'named'\nremote = 'core'\n\n\
             [[actions]]\ntype = 'include-remote'\nremote = 'core'\n\n\
             [[actions]]\ntype = 'include-remote'\nremote = 'core'\n"
        ));
        let leaf = fixture.loaded();

        let labels: Vec<_> = leaf
            .inclusions
            .iter()
            .map(|inclusion| inclusion.label.clone())
            .collect();
        // An id-ful inclusion is labeled by its id and does not consume a
        // number, so the id-less suffixes stay contiguous around it.
        assert_eq!(
            labels,
            ["remote=core", "named", "remote=core #2", "remote=core #3"]
        );

        let unique: std::collections::BTreeSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), labels.len());
    }

    #[test]
    fn including_an_undeclared_remote_fails() {
        let fixture = Fixture::new(&leaf_with(
            "[[actions]]\ntype = 'create-dir'\ndest = 'a'\n\n\
             [[actions]]\ntype = 'include-remote'\nremote = 'absent'\n",
        ));

        let error = fixture.error();
        assert!(matches!(
            error,
            LoadError::UnknownRemote { position: 1, .. }
        ));
        let message = error.to_string();
        assert!(message.contains("action #2"), "{message}");
        assert!(message.contains("`absent`"), "{message}");
    }

    #[test]
    fn including_a_non_git_remote_names_the_kind_it_actually_is() {
        for (kind, declaration) in [
            ("file", "[remotes.other]\ntype = 'file'\nurl = 'u'\n"),
            ("archive", "[remotes.other]\ntype = 'archive'\nurl = 'u'\n"),
        ] {
            let fixture = Fixture::new(&format!(
                "{declaration}\n[[actions]]\ntype = 'include-remote'\nremote = 'other'\n"
            ));

            let error = fixture.error();
            assert!(
                matches!(error, LoadError::NotAGitRemote { kind: found, .. } if found == kind),
                "{error:?}"
            );
            let message = error.to_string();
            assert!(message.contains(&format!("a {kind} remote")), "{message}");
            assert!(message.contains("action #1"), "{message}");
        }
    }

    #[test]
    fn the_allow_flag_comes_from_the_leafs_remote_declaration() {
        let inclusion = "[[actions]]\ntype = 'include-remote'\nremote = 'core'\n";

        let permitted = Fixture::new(&format!(
            "[remotes.core]\ntype = 'git'\nurl = 'u'\nallow-dynamic-vars = true\n\n{inclusion}"
        ));
        assert!(permitted.loaded().included[&id("core")].allow_dynamic_vars);

        // Omitting the field is the default: including a repository does not by
        // itself let it execute commands.
        let default = Fixture::new(&leaf_with(inclusion));
        assert!(!default.loaded().included[&id("core")].allow_dynamic_vars);
    }

    #[test]
    fn an_inclusions_overrides_reach_the_model_verbatim() {
        let fixture = Fixture::new(&leaf_with(
            "[[actions]]\ntype = 'include-remote'\nremote = 'core'\n\
             vars = { profile = 'personal', empty = '' }\n",
        ));
        let leaf = fixture.loaded();

        let vars = &leaf.inclusions[0].vars;
        assert_eq!(vars.len(), 2);
        assert_eq!(
            vars[&VarName::new("profile").expect("valid")],
            "personal".to_owned()
        );
        assert_eq!(vars[&VarName::new("empty").expect("valid")], String::new());
    }

    #[test]
    fn conditions_do_not_affect_structural_discovery() {
        // Both strings would evaluate to false if anything evaluated them.
        // Nothing here does: reachability is structural.
        let fixture = Fixture::new(
            "[remotes.core]\ntype = 'git'\nurl = 'u'\nwhen = 'false'\n\n\
             [[actions]]\ntype = 'include-remote'\nremote = 'core'\nunless = 'true'\n",
        );
        let leaf = fixture.loaded();

        assert_eq!(leaf.included.len(), 1);
        assert_eq!(leaf.inclusions.len(), 1);
        assert_eq!(
            leaf.included[&id("core")].state,
            RemoteState::NotMaterialized
        );
    }

    #[test]
    fn the_loader_reads_only_the_roots_it_is_given() {
        // A stray tree under another root must not be mistaken for this leaf's
        // materialization, which is what pairing the three roots together buys.
        let fixture = Fixture::new(&leaf_with(
            "[[actions]]\ntype = 'include-remote'\nremote = 'core'\n",
        ));
        let elsewhere = fixture.path().join("elsewhere/remotes/core");
        fs::create_dir_all(&elsewhere).expect("stray tree");
        fs::write(
            elsewhere.join(BatfilesConfig::FILE_NAME),
            "[vars]\ntheme = 'dark'\n",
        )
        .expect("stray manifest");

        assert_eq!(
            fixture.loaded().included[&id("core")].state,
            RemoteState::NotMaterialized
        );
    }
}
