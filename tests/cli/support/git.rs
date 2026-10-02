//! Local Git repositories for clone and update tests.

use super::{copy_tree, display, fixture_tree};
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A local bare git repository, standing in for one on the network.
pub(crate) struct BareRepo {
    dir: TempDir,
}

impl BareRepo {
    /// Create a repository with one file and one commit on `main`.
    pub(crate) fn new() -> Self {
        let repo = Self::empty();
        repo.publish("README.md", "a plugin\n", "first");
        repo
    }

    /// Publish `tests/fixtures/<name>` as a bare repository.
    pub(crate) fn from_fixture(name: &str) -> Self {
        let repo = Self::empty();
        repo.stand_on("main");
        copy_tree(&fixture_tree(name), &repo.work());
        repo.record("main", &format!("the {name} repository"), &["add", "-A"]);
        repo
    }

    /// Create an empty bare origin and working repository with its origin configured.
    fn empty() -> Self {
        let repo = Self {
            dir: tempfile::tempdir().expect("a temporary directory"),
        };
        git(
            repo.dir.path(),
            &["init", "--bare", "-b", "main", "origin.git"],
        );
        git(repo.dir.path(), &["init", "-b", "main", "work"]);
        git(
            &repo.work(),
            &["remote", "add", "origin", &display(&repo.origin())],
        );
        repo
    }

    /// Commit a file on `main` and push it.
    pub(crate) fn publish(&self, name: &str, contents: &str, message: &str) {
        self.publish_on("main", name, contents, message);
    }

    /// Commit and push a file on `branch`. Create a missing branch at the working repository's
    /// current commit.
    pub(crate) fn publish_on(&self, branch: &str, name: &str, contents: &str, message: &str) {
        self.commit(branch, name, contents, message, &["add", "-A"]);
    }

    /// Force-add an ignored file, commit it, and push the branch.
    pub(crate) fn publish_ignored(&self, branch: &str, name: &str, contents: &str) {
        self.commit(
            branch,
            name,
            contents,
            "start tracking it",
            &["add", "-f", name],
        );
    }

    /// Move a tracked file to another name and push it.
    pub(crate) fn publish_renamed(&self, from: &str, to: &str) {
        let work = self.work();
        git(&work, &["checkout", "main"]);
        git(&work, &["mv", from, to]);
        git(&work, &["commit", "-m", &format!("rename {from} to {to}")]);
        git(&work, &["push", "origin", "main"]);
    }

    fn commit(&self, branch: &str, name: &str, contents: &str, message: &str, add: &[&str]) {
        self.stand_on(branch);
        let path = self.work().join(name);
        fs::create_dir_all(path.parent().expect("a parent")).expect("a directory to commit into");
        fs::write(&path, contents).expect("a file to commit");
        self.record(branch, message, add);
    }

    /// Put the working clone on `branch`, creating it where it is not there yet.
    fn stand_on(&self, branch: &str) {
        let work = self.work();
        if git_succeeds(&work, &["rev-parse", "--verify", "--quiet", &heads(branch)]) {
            git(&work, &["checkout", branch]);
        } else {
            git(&work, &["checkout", "-b", branch]);
        }
    }

    /// Stage using `add`, commit with `message`, and push `branch`.
    fn record(&self, branch: &str, message: &str, add: &[&str]) {
        let work = self.work();
        git(&work, add);
        git(&work, &["commit", "-m", message]);
        git(&work, &["push", "origin", branch]);
    }

    /// Tag the working repository's current commit and push the tag.
    pub(crate) fn tag(&self, name: &str) {
        git(&self.work(), &["tag", name]);
        git(&self.work(), &["push", "origin", name]);
    }

    /// The bare repository, which is what a manifest names as a `source`.
    pub(crate) fn origin(&self) -> PathBuf {
        as_written(self.dir.path().join("origin.git"))
    }

    /// A second bare repository beside the first, for a list that has to name
    /// more than one.
    pub(crate) fn another(&self, name: &str) -> PathBuf {
        let bare = format!("{name}.git");
        git(self.dir.path(), &["init", "--bare", "-b", "main", &bare]);
        let origin = as_written(self.dir.path().join(&bare));
        git(&self.work(), &["push", &display(&origin), "main"]);
        origin
    }

    fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }
}

/// `path` as a manifest or clone list writes a repository: on Windows with forward slashes,
/// which Git and Windows both accept, which a TOML string holds without escapes, and after
/// which a clone list derives a clone's name.
fn as_written(path: PathBuf) -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(display(&path).replace('\\', "/"))
    } else {
        path
    }
}

/// Return the full `refs/heads/<branch>` name.
fn heads(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

/// Run a fixture Git command and return whether it succeeded.
pub(crate) fn git_succeeds(dir: &Path, args: &[&str]) -> bool {
    run_git(dir, args).status.success()
}

/// Run one `git` command while building a fixture, and insist it worked.
pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
    let output = run_git(dir, args);
    assert!(
        output.status.success(),
        "git {args:?} in {} failed: {}",
        display(dir),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// Run Git with an isolated fixture environment and fixed author identity, capturing output.
fn run_git(dir: &Path, args: &[&str]) -> std::process::Output {
    let mut command = std::process::Command::new("git");
    // Fixture commands use only their repository's config and explicit options.
    for (name, _) in std::env::vars_os() {
        if name
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(name);
        }
    }
    command.env("GIT_CONFIG_NOSYSTEM", "1").env(
        "GIT_CONFIG_GLOBAL",
        if cfg!(windows) { "NUL" } else { "/dev/null" },
    );
    command
        .args([
            "-c",
            "user.name=batfiles tests",
            "-c",
            "user.email=tests@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "tag.gpgsign=false",
            "-c",
            "init.templateDir=",
        ])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git should be on PATH for the cloning tests")
}
