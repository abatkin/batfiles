//! Temporary repository, home, config, and cache roots.

use super::{BareRepo, Server, batfiles, canonical, copy_tree, display, fixture_tree};
use assert_cmd::Command;
use std::fs;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use tempfile::TempDir;

/// Temporary repository, home, config, and cache roots for CLI tests.
pub(crate) struct Tree {
    /// Held so the directory lives as long as the tree.
    _dir: TempDir,
    /// The directory's canonical path, which is how batfiles and the shell report a working
    /// directory reached through a symlink, such as macOS's `/var`.
    root: PathBuf,
}

impl Tree {
    /// Create sibling roots with an empty leaf manifest.
    pub(crate) fn new() -> Self {
        let tree = Self::roots();
        tree.repository("repo");
        tree
    }

    /// Create roots and copy `tests/fixtures/<name>` into the leaf repository.
    pub(crate) fn fixture(name: &str) -> Self {
        let tree = Self::roots();
        copy_tree(&fixture_tree(name), &tree.path("repo"));
        tree
    }

    /// Create home, config, and cache directories, leaving the repository path absent.
    pub(crate) fn roots() -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let root = canonical(dir.path());
        for name in ["home", "config", "cache"] {
            fs::create_dir(root.join(name)).expect("a root directory");
        }
        Self { _dir: dir, root }
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// The tree the four roots sit in, for the cases that run from inside it.
    #[cfg(unix)]
    pub(crate) fn root(&self) -> &Path {
        &self.root
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

    /// Replace `{origin}` placeholders in the manifest with the bare repository path.
    pub(crate) fn point_at_origin(&self, origin: &BareRepo) {
        self.fill_in("origin", &display(&origin.origin()));
    }

    /// Replace a named manifest placeholder with the bare repository path.
    pub(crate) fn point_remote_at(&self, placeholder: &str, origin: &BareRepo) {
        self.fill_in(placeholder, &display(&origin.origin()));
    }

    /// Replace a named placeholder in a repository file.
    pub(crate) fn point_file_at(&self, relative: &str, placeholder: &str, value: &str) {
        self.fill_in_file(relative, placeholder, value);
    }

    /// Replace every `{placeholder}` in the leaf manifest; panic if none exists.
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

    /// The machine-local variable values, which need not exist.
    pub(crate) fn machine_vars(&self) -> PathBuf {
        self.path("config").join("vars.toml")
    }

    /// `vars.toml` as it stands, which a command must have written.
    pub(crate) fn machine_vars_document(&self) -> String {
        fs::read_to_string(self.machine_vars()).expect("vars.toml should exist")
    }

    /// Put a `vars.toml` in place verbatim, including shapes batfiles would
    /// never write itself.
    pub(crate) fn write_machine_vars(&self, document: &str) {
        fs::write(self.machine_vars(), document).expect("a vars document");
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
