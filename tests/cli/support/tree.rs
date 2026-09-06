//! Temporary repository, home, config, and cache roots.

use super::{BareRepo, Server, batfiles, copy_tree, display};
use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A throwaway tree standing in for the four location roots, with an empty leaf
/// manifest in place.
pub(crate) struct Tree {
    dir: TempDir,
}

impl Tree {
    /// The four roots as sibling directories, with `repo/batfiles.toml` empty
    /// but present, which is what a command needs to get past reading it.
    pub(crate) fn new() -> Self {
        let tree = Self::roots();
        tree.repository("repo");
        tree
    }

    /// The same roots, with the leaf repository copied from
    /// `tests/fixtures/<name>` rather than holding a manifest written inline.
    pub(crate) fn fixture(name: &str) -> Self {
        let tree = Self::roots();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        copy_tree(&source, &tree.path("repo"));
        tree
    }

    /// The three roots that are directories in their own right, with nothing in
    /// the repository yet.
    pub(crate) fn roots() -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for name in ["home", "config", "cache"] {
            fs::create_dir(dir.path().join(name)).expect("a root directory");
        }
        Self { dir }
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// The tree the four roots sit in, for the cases that run from inside it.
    #[cfg(unix)]
    pub(crate) fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Create a repository directory holding an empty manifest, and return it.
    pub(crate) fn repository(&self, relative: &str) -> PathBuf {
        let repo = self.path(relative);
        fs::create_dir_all(&repo).expect("a repository directory");
        fs::write(repo.join("batfiles.toml"), "").expect("a manifest");
        repo
    }

    pub(crate) fn manifest(&self) -> PathBuf {
        self.path("repo").join("batfiles.toml")
    }

    /// Replace the leaf manifest.
    pub(crate) fn write_manifest(&self, contents: &str) {
        fs::write(self.manifest(), contents).expect("a manifest");
    }

    /// Point a copied fixture's `{server}` placeholders at a running server.
    pub(crate) fn point_at(&self, server: &Server) {
        self.fill_in("server", server.address());
    }

    /// The same, for a fixture whose sources are repositories rather than URLs:
    /// a bare repository lands in a temporary directory no fixture can name.
    pub(crate) fn point_at_origin(&self, origin: &BareRepo) {
        self.fill_in("origin", &display(&origin.origin()));
    }

    /// The same, for a placeholder in a repository file that is not the
    /// manifest: a clone list names its repositories itself, so that is where
    /// its `{origin}`s are written.
    pub(crate) fn point_file_at(&self, relative: &str, placeholder: &str, value: &str) {
        self.fill_in_file(relative, placeholder, value);
    }

    /// Replace every `{placeholder}` in the leaf manifest, insisting there was
    /// one: a fixture that stopped carrying it would otherwise be driven against
    /// a source the test never set.
    fn fill_in(&self, placeholder: &str, value: &str) {
        self.fill_in_file("batfiles.toml", placeholder, value);
    }

    fn fill_in_file(&self, relative: &str, placeholder: &str, value: &str) {
        let path = self.path("repo").join(relative);
        let text = fs::read_to_string(&path).expect("the fixture file");
        let written = format!("{{{placeholder}}}");
        assert!(
            text.contains(&written),
            "{relative} has no `{written}` to point at anything"
        );
        fs::write(&path, text.replace(&written, value)).expect("the fixture file");
    }

    /// Put a file in the leaf repository, and return where it landed.
    pub(crate) fn repo_file(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.path("repo").join(relative);
        fs::create_dir_all(path.parent().expect("a parent")).expect("a source directory");
        fs::write(&path, contents).expect("a source file");
        path
    }

    /// A path inside the selected home, which need not exist.
    pub(crate) fn home(&self, relative: &str) -> PathBuf {
        self.path("home").join(relative)
    }

    /// The machine-local disabled lists, which need not exist.
    pub(crate) fn disabled(&self) -> PathBuf {
        self.path("config").join("disabled.toml")
    }

    /// `disabled.toml` as it stands, which a command must have written.
    pub(crate) fn disabled_document(&self) -> String {
        fs::read_to_string(self.disabled()).expect("disabled.toml should exist")
    }

    /// Put a `disabled.toml` in place verbatim, including shapes batfiles would
    /// never write itself.
    pub(crate) fn write_disabled(&self, document: &str) {
        fs::write(self.disabled(), document).expect("a disabled document");
    }

    /// A command with all four roots selected inside this tree.
    pub(crate) fn batfiles(&self) -> Command {
        let mut command = batfiles();
        command
            .env("BATFILES_HOME", self.path("home"))
            .env("BATFILES_DIR", self.path("repo"))
            .env("BATFILES_CONFIG_DIR", self.path("config"))
            .env("BATFILES_CACHE_DIR", self.path("cache"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_CACHE_HOME");
        command
    }
}
