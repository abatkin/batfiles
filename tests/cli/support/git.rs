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
    /// A repository with one commit on `main`, holding one file: what a test
    /// clones when the clone itself is the subject and the content is not.
    pub(crate) fn new() -> Self {
        let repo = Self::empty();
        repo.publish("README.md", "a plugin\n", "first");
        repo
    }

    /// A repository holding the fixture tree at `tests/fixtures/<name>`, for
    /// tests where the repository's content matters.
    pub(crate) fn from_fixture(name: &str) -> Self {
        let repo = Self::empty();
        repo.stand_on("main");
        copy_tree(&fixture_tree(name), &repo.work());
        repo.record("main", &format!("the {name} repository"), &["add", "-A"]);
        repo
    }

    /// The two repositories and the link between them, with no commit yet.
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

    /// Commit a file and push it, for the tests about what an update brings.
    pub(crate) fn publish(&self, name: &str, contents: &str, message: &str) {
        self.publish_on("main", name, contents, message);
    }

    /// The same, on a branch other than `main`, for the tests about a declared
    /// `ref`. The branch is created at whatever the working clone is standing on
    /// the first time it is named, and extended after that.
    pub(crate) fn publish_on(&self, branch: &str, name: &str, contents: &str, message: &str) {
        self.commit(branch, name, contents, message, &["add", "-A"]);
    }

    /// Start tracking a file the repository's own `.gitignore` covers, which
    /// `publish` cannot do — `git add -A` passes an ignored path over.
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
        // So that a name may be a path: a repository an action installs from
        // keeps its files in directories like any other.
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

    /// Commit whatever `add` stages and push it, for both the one-file case and
    /// the whole-fixture one.
    fn record(&self, branch: &str, message: &str, add: &[&str]) {
        let work = self.work();
        git(&work, add);
        git(&work, &["commit", "-m", message]);
        git(&work, &["push", "origin", branch]);
    }

    /// Tag what the working clone is standing on, and push the tag: a `ref` that
    /// is not a branch and therefore never moves.
    pub(crate) fn tag(&self, name: &str) {
        git(&self.work(), &["tag", name]);
        git(&self.work(), &["push", "origin", name]);
    }

    /// The bare repository, which is what a manifest names as a `source`.
    pub(crate) fn origin(&self) -> PathBuf {
        self.dir.path().join("origin.git")
    }

    /// A second bare repository beside the first, for a list that has to name
    /// more than one.
    pub(crate) fn another(&self, name: &str) -> PathBuf {
        let bare = format!("{name}.git");
        git(self.dir.path(), &["init", "--bare", "-b", "main", &bare]);
        let origin = self.dir.path().join(&bare);
        git(&self.work(), &["push", &display(&origin), "main"]);
        origin
    }

    fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }
}

/// A branch's full ref name, which is what a fixture asks about rather than the
/// short name a tag of the same spelling would also answer to.
fn heads(branch: &str) -> String {
    format!("refs/heads/{branch}")
}

/// Whether a git command succeeded, for the one fixture question that has two
/// legitimate answers: whether a branch is there yet.
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

/// The invocation both of the above share, with the identity and the cleared
/// redirects that make a fixture build the same way on every machine.
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
