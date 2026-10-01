//! The release scripts under `dist/`, run with `sh` against stand-in binaries and `file://`
//! release trees. `dist/binary.sh` and `dist/publish.sh` need real toolchains and GitHub, so the
//! release workflow is what exercises them.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;

const LINUX: &str = "x86_64-unknown-linux-musl";
const WINDOWS: &str = "x86_64-pc-windows-msvc";

/// Run `dist/<script>` with `args`.
fn script(script: &str, args: &[&str]) -> Command {
    let mut command = Command::new("sh");
    command
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("dist")
                .join(script),
        )
        .args(args)
        .env_remove("CARGO_TARGET_DIR");
    command
}

fn stderr_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stderr).into_owned()
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A scratch directory holding stand-in binaries in `bin/`.
struct Scratch {
    dir: TempDir,
}

impl Scratch {
    /// Stand-ins for the Linux and Windows binaries.
    fn new() -> Self {
        let scratch = Self {
            dir: TempDir::new().expect("a scratch directory"),
        };
        fs::create_dir(scratch.bin()).expect("a binaries directory");
        scratch.binary(&format!("batfiles-{LINUX}"));
        scratch.binary(&format!("batfiles-{WINDOWS}.exe"));
        scratch
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    fn bin(&self) -> PathBuf {
        self.path("bin")
    }

    fn binary(&self, name: &str) {
        fs::write(self.bin().join(name), format!("stand-in for {name}\n")).expect("a binary");
    }

    /// `dist/assemble.sh` for version 1.2.3 into `out/`, requiring `targets`.
    fn assemble(&self, base: &str, targets: &str) -> Command {
        let out = self.path("out");
        let bin = self.bin();
        script(
            "assemble.sh",
            &[
                "1.2.3",
                base,
                out.to_str().expect("UTF-8"),
                bin.to_str().expect("UTF-8"),
                targets,
            ],
        )
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative)).expect("an assembled file")
    }

    /// Serve `out/` as release 1.2.3 and as the latest release, returning the tree's base URL.
    fn publish(&self) -> String {
        let tree = self.path("tree");
        for dir in ["download/v1.2.3", "latest/download"] {
            fs::create_dir_all(tree.join(dir)).expect("a release directory");
            for entry in fs::read_dir(self.path("out")).expect("the assembled release") {
                let entry = entry.expect("an assembled file");
                fs::copy(entry.path(), tree.join(dir).join(entry.file_name())).expect("a copy");
            }
        }
        format!("file://{}", tree.display())
    }
}

#[test]
fn an_assembled_release_is_the_whole_asset_set() {
    let scratch = Scratch::new();
    scratch
        .assemble(
            "https://example.com/batfiles/",
            &format!("{LINUX} {WINDOWS}"),
        )
        .assert()
        .success();

    let mut names: Vec<_> = fs::read_dir(scratch.path("out"))
        .expect("the assembled release")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("UTF-8")
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "SHA256SUMS",
            "VERSION",
            &format!("batfiles-{WINDOWS}.exe"),
            &format!("batfiles-{LINUX}"),
            "install.ps1",
            "install.sh",
        ]
    );
    assert_eq!(scratch.read("out/VERSION"), "1.2.3\n");

    let sums: String = [
        format!("batfiles-{WINDOWS}.exe"),
        format!("batfiles-{LINUX}"),
    ]
    .iter()
    .map(|name| {
        let digest = sha256(&fs::read(scratch.path("out").join(name)).expect("a binary"));
        format!("{digest}  {name}\n")
    })
    .collect();
    assert_eq!(scratch.read("out/SHA256SUMS"), sums);

    // The trailing slash is not part of the base.
    let install = scratch.read("out/install.sh");
    assert!(install.contains("\nbatfiles_stamped_base='https://example.com/batfiles'\n"));
    assert!(!install.contains("batfiles_stamped_base=unstamped"));
    let install = scratch.read("out/install.ps1");
    assert!(install.contains("\n$BatfilesStampedBase = 'https://example.com/batfiles'\n"));
    assert!(!install.contains("= 'unstamped'"));
}

#[test]
fn a_stamped_installer_names_its_base_and_an_unstamped_one_refuses() {
    let scratch = Scratch::new();
    scratch
        .assemble("https://example.com/batfiles", "")
        .assert()
        .success();

    let stamped = Command::new("sh")
        .arg(scratch.path("out/install.sh"))
        .env_remove("BATFILES_BASE")
        .assert()
        .code(1);
    assert!(stderr_of(&stamped).contains("https://example.com/batfiles/latest/download/"));

    let unstamped = Command::new("sh")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("dist/install.sh"))
        .assert()
        .code(1);
    assert!(stderr_of(&unstamped).contains("never stamped"));
}

#[test]
fn assembly_refuses_what_would_not_be_a_release() {
    let base = "https://example.com/batfiles";
    let cases: [([&str; 3], &str); 5] = [
        (["1.2", base, "out"], "is not X.Y.Z"),
        (["01.2.3", base, "out"], "is not X.Y.Z"),
        (["1.2.3", "example.com", "out"], "is not a URL"),
        (["1.2.3", "https://example.com/a'b", "out"], "cannot quote"),
        (["1.2.3", "https://example.com/a&b", "out"], "cannot quote"),
    ];
    for (args, message) in cases {
        let assertion = script("assemble.sh", &args).assert().failure();
        assert!(stderr_of(&assertion).contains(message), "{args:?}");
    }
}

#[test]
fn a_required_target_list_is_matched_exactly() {
    let base = "https://example.com/batfiles";
    let scratch = Scratch::new();

    let missing = scratch
        .assemble(base, &format!("{LINUX} {WINDOWS} aarch64-apple-darwin"))
        .assert()
        .failure();
    assert!(stderr_of(&missing).contains("no binary for aarch64-apple-darwin"));

    let unexpected = scratch.assemble(base, LINUX).assert().failure();
    assert!(stderr_of(&unexpected).contains(&format!("batfiles-{WINDOWS}, which is not")));
    assert!(!scratch.path("out").exists());
}

#[test]
fn assembly_refuses_misnamed_binaries_and_an_occupied_output() {
    let base = "https://example.com/batfiles";

    let scratch = Scratch::new();
    scratch.binary("batfiles-aarch64-pc-windows-msvc");
    let assertion = scratch.assemble(base, "").assert().failure();
    assert!(stderr_of(&assertion).contains("a Windows binary without .exe"));

    let scratch = Scratch::new();
    fs::create_dir(scratch.path("out")).expect("an output directory");
    fs::write(scratch.path("out/leftover"), "").expect("a leftover");
    let assertion = scratch.assemble(base, "").assert().failure();
    assert!(stderr_of(&assertion).contains("is not empty"));

    let scratch = Scratch::new();
    for entry in fs::read_dir(scratch.bin()).expect("the binaries") {
        fs::remove_file(entry.expect("a binary").path()).expect("a removal");
    }
    let assertion = scratch.assemble(base, "").assert().failure();
    assert!(stderr_of(&assertion).contains("no batfiles-<target> binaries"));
}

#[test]
fn a_published_tree_verifies() {
    let scratch = Scratch::new();
    scratch
        .assemble("https://example.com/batfiles", "")
        .assert()
        .success();
    let url = scratch.publish();

    script(
        "verify.sh",
        &[&url, "1.2.3", "yes", "https://example.com/batfiles"],
    )
    .assert()
    .success();
}

#[test]
fn verification_catches_a_corrupt_binary_a_wrong_stamp_and_a_stale_latest() {
    let scratch = Scratch::new();
    scratch
        .assemble("https://example.com/batfiles", "")
        .assert()
        .success();
    let url = scratch.publish();
    let stamped = "https://example.com/batfiles";

    let wrong_stamp = script("verify.sh", &[&url, "1.2.3", "no"])
        .assert()
        .failure();
    assert!(stderr_of(&wrong_stamp).contains("install.sh is not stamped"));

    fs::write(scratch.path("tree/latest/download/VERSION"), "1.2.2\n").expect("a stale latest");
    let stale = script("verify.sh", &[&url, "1.2.3", "yes", stamped])
        .assert()
        .failure();
    assert!(stderr_of(&stale).contains("serves version 1.2.2"));
    script("verify.sh", &[&url, "1.2.3", "no", stamped])
        .assert()
        .success();

    fs::write(
        scratch.path(&format!("tree/download/v1.2.3/batfiles-{LINUX}")),
        "tampered\n",
    )
    .expect("a corrupt binary");
    let corrupt = script("verify.sh", &[&url, "1.2.3", "no", stamped])
        .assert()
        .failure();
    assert!(stderr_of(&corrupt).contains("does not match SHA256SUMS"));

    let absent = script("verify.sh", &[&url, "9.9.9", "no", stamped])
        .assert()
        .failure();
    assert!(stderr_of(&absent).contains("cannot fetch"));
}

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
        command
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("dist")
                    .join(script),
            )
            .args(args);
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
    for tag in ["1.2.3", "v1.2.4", "v1.2.3.1", "v1.2.3-", "v1.2.3-rc/1"] {
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
