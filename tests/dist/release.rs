//! `dist/tag.sh` and `dist/release.sh`: which tags a release may have, and making them.

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;

use super::support::{dist, stderr_of};

/// A checkout of version 1.2.3 whose `origin` is a local bare repository, with `main` pushed.
struct Checkout {
    dir: TempDir,
}

impl Checkout {
    fn new() -> Self {
        let checkout = Self {
            dir: TempDir::new().expect("a scratch directory"),
        };
        fs::create_dir(checkout.work()).expect("a work tree");
        checkout.git(&[
            "init",
            "--quiet",
            "--bare",
            "--initial-branch=main",
            "../origin.git",
        ]);
        checkout.git(&["init", "--quiet", "--initial-branch=main"]);
        checkout.git(&["remote", "add", "origin", "../origin.git"]);
        checkout.commit(
            "Cargo.toml",
            "[package]\nname = \"x\"\nversion = \"1.2.3\"\n",
        );
        checkout.git(&["push", "--quiet", "origin", "main"]);
        checkout.git(&["fetch", "--quiet", "origin"]);
        checkout
    }

    fn work(&self) -> PathBuf {
        self.dir.path().join("work")
    }

    /// Git in the work tree, isolated from the developer's configuration.
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(self.work())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com");
        command
    }

    fn git(&self, args: &[&str]) {
        self.command("git").args(args).assert().success();
    }

    fn commit(&self, file: &str, contents: &str) {
        fs::write(self.work().join(file), contents).expect("a file");
        self.git(&["add", file]);
        self.git(&["commit", "--quiet", "-m", file]);
    }

    /// `dist/<script>` run in the work tree.
    fn script(&self, script: &str, args: &[&str]) -> Command {
        let mut command = self.command("sh");
        command.arg(dist(script)).args(args);
        command
    }

    /// The tags in the work tree.
    fn tags(&self) -> String {
        let output = self
            .command("git")
            .args(["tag", "--list"])
            .output()
            .expect("git");
        String::from_utf8(output.stdout).expect("UTF-8")
    }
}

#[test]
fn a_stable_tag_is_the_cargo_version_on_main() {
    let checkout = Checkout::new();
    checkout
        .script("tag.sh", &["v1.2.3"])
        .assert()
        .success()
        .stdout("version=1.2.3\nprerelease=false\n");

    checkout.commit("unmerged", "");
    let assertion = checkout.script("tag.sh", &["v1.2.3"]).assert().failure();
    assert!(stderr_of(&assertion).contains("is not on origin/main"));
}

#[test]
fn a_prerelease_tag_suffixes_the_cargo_version_from_any_commit() {
    let checkout = Checkout::new();
    checkout.commit("unmerged", "");
    checkout
        .script("tag.sh", &["v1.2.3-rc.4"])
        .assert()
        .success()
        .stdout("version=1.2.3-rc.4\nprerelease=true\n");
}

#[test]
fn a_tag_the_cargo_version_does_not_allow_is_refused() {
    let checkout = Checkout::new();
    for tag in [
        "1.2.3",
        "v1.2.4",
        "v1.2.3.1",
        "v1.2.3-",
        "v1.2.3-rc/1",
        "v1.2.3-rc..1",
        "v1.2.3-rc.01",
        "v1.2.3-rc.",
        "v1.2.3+build",
    ] {
        checkout.script("tag.sh", &[tag]).assert().failure();
    }

    checkout.commit("Cargo.toml", "[package]\nversion = \"1.2.3-rc.1\"\n");
    let assertion = checkout
        .script("tag.sh", &["v1.2.3-rc.1"])
        .assert()
        .failure();
    assert!(stderr_of(&assertion).contains("is not X.Y.Z"));
}

#[test]
fn release_candidates_number_past_every_tag_here_and_on_origin() {
    let checkout = Checkout::new();
    checkout.script("release.sh", &["rc"]).assert().success();
    assert_eq!(checkout.tags(), "v1.2.3-rc.1\n");

    checkout.git(&["push", "--quiet", "origin", "v1.2.3-rc.1"]);
    checkout.git(&["tag", "--delete", "v1.2.3-rc.1"]);
    checkout.git(&["tag", "v1.2.3-rc.9"]);
    checkout.git(&["push", "--quiet", "origin", "v1.2.3-rc.9"]);
    checkout.git(&["tag", "--delete", "v1.2.3-rc.9"]);
    checkout.git(&["tag", "v1.2.3-rc.10"]);
    checkout.git(&["tag", "v1.2.30-rc.50"]);
    checkout.script("release.sh", &["rc"]).assert().success();
    assert!(checkout.tags().contains("v1.2.3-rc.11\n"));
}

#[test]
fn a_stable_release_is_tagged_once_from_main() {
    let checkout = Checkout::new();
    checkout.commit("unmerged", "");
    let off_main = checkout
        .script("release.sh", &["stable"])
        .assert()
        .failure();
    assert!(stderr_of(&off_main).contains("is not on origin/main"));
    assert_eq!(checkout.tags(), "");

    checkout.git(&["reset", "--quiet", "--hard", "origin/main"]);
    checkout
        .script("release.sh", &["stable"])
        .assert()
        .success();
    assert_eq!(checkout.tags(), "v1.2.3\n");

    for kind in ["stable", "rc"] {
        let again = checkout.script("release.sh", &[kind]).assert().failure();
        assert!(stderr_of(&again).contains("v1.2.3 is already tagged"));
    }
}

#[test]
fn a_release_is_not_tagged_with_uncommitted_changes() {
    let checkout = Checkout::new();
    fs::write(checkout.work().join("Cargo.toml"), "edited").expect("an edit");
    let assertion = checkout.script("release.sh", &["rc"]).assert().failure();
    assert!(stderr_of(&assertion).contains("uncommitted changes"));
    assert_eq!(checkout.tags(), "");
}
