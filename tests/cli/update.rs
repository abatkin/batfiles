//! `update`: a link to the built binary, replaced from a loopback release tree of compiled
//! stand-ins.

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use assert_cmd::Command;
use tempfile::TempDir;

use crate::support::{
    Reply, Server, batfiles, batfiles_at, canonical, compiled_stand_in, entries, file_url, sha256,
    stderr_of, stdout_of,
};

/// A release newer than any this crate builds.
const NEWER: &str = "99.0.0";

/// A release older than any this crate builds.
const OLDER: &str = "0.0.1";

/// The release target whose asset this machine takes, worked out independently of batfiles:
/// Linux takes the static musl build, and Windows the x86_64 one.
fn target() -> String {
    let arch = std::env::consts::ARCH;
    match std::env::consts::OS {
        "linux" => format!("{arch}-unknown-linux-musl"),
        "macos" => format!("{arch}-apple-darwin"),
        "windows" => "x86_64-pc-windows-msvc".to_owned(),
        other => panic!("no release asset for {other}"),
    }
}

/// The asset a release publishes for this machine.
fn asset() -> String {
    format!("batfiles-{}{}", target(), std::env::consts::EXE_SUFFIX)
}

/// A stand-in release binary that reports `version`.
fn stand_in(version: &str) -> Vec<u8> {
    compiled_stand_in(&target(), version)
}

/// The name `update` stages a release under, beside the executable.
const STAGED: &str = if cfg!(windows) {
    "batfiles.exe.batfiles-update.exe"
} else {
    "batfiles.batfiles-update"
};

/// The name of the executable Windows sets aside.
#[cfg(windows)]
const SET_ASIDE: &str = "batfiles.exe.batfiles-old";

/// The version the built binary reports.
fn running() -> String {
    let assertion = batfiles().arg("version").assert().success();
    stdout_of(&assertion)
        .trim_end()
        .strip_prefix("batfiles ")
        .expect("a version line")
        .to_owned()
}

/// One release in a tree: its version, the binary it serves for this machine, and the digest
/// its `SHA256SUMS` lists for that binary.
struct Release {
    version: &'static str,
    binary: Vec<u8>,
    listed: Option<String>,
    latest: bool,
}

impl Release {
    /// A release whose binary reports its version and matches its checksum.
    fn of(version: &'static str) -> Self {
        let binary = stand_in(version);
        Self {
            version,
            listed: Some(sha256(&binary)),
            binary,
            latest: false,
        }
    }

    fn latest(self) -> Self {
        Self {
            latest: true,
            ..self
        }
    }

    /// Every path the release is served at, and what is there.
    fn files(&self) -> Vec<(String, Vec<u8>)> {
        let version = format!("{}\n", self.version);
        let sums = self
            .listed
            .as_ref()
            .map_or_else(String::new, |digest| format!("{digest}  {}\n", asset()));
        let dir = format!("download/v{}", self.version);
        let mut files = vec![
            (format!("{dir}/VERSION"), version.clone().into_bytes()),
            (format!("{dir}/SHA256SUMS"), sums.into_bytes()),
            (format!("{dir}/{}", asset()), self.binary.clone()),
        ];
        if self.latest {
            files.push(("latest/download/VERSION".into(), version.into_bytes()));
        }
        files
    }
}

/// Serve `releases` over loopback HTTP.
fn serve(releases: &[Release]) -> Server {
    Server::serving(
        releases
            .iter()
            .flat_map(Release::files)
            .map(|(path, body)| (format!("/{path}"), Reply::Bytes(body)))
            .collect(),
    )
}

/// A machine with the built binary at `bin/batfiles`, which `update` may replace.
struct Machine {
    dir: TempDir,
    original: Vec<u8>,
}

impl Machine {
    fn new() -> Self {
        // Beside the build, so the binary can be linked rather than written: a file this process
        // had open for writing could be held open by a sibling test's child, and fail to run.
        let dir = TempDir::new_in(env!("CARGO_TARGET_TMPDIR")).expect("a scratch directory");
        let built = assert_cmd::cargo::cargo_bin("batfiles");
        let machine = Self {
            dir,
            original: fs::read(&built).expect("the binary"),
        };
        fs::create_dir(machine.bin_dir()).expect("a bin directory");
        // `update` renames over the link, which leaves the built binary alone. Windows holds a
        // file open under every name while it runs, so a link to the binary the other tests run
        // could never be removed once set aside; a copy is written and closed instead.
        if cfg!(windows) {
            fs::copy(&built, machine.exe()).expect("a copy of the binary");
        } else {
            fs::hard_link(&built, machine.exe()).expect("a link to the binary");
        }
        machine
    }

    fn bin_dir(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    fn exe(&self) -> PathBuf {
        self.bin_dir()
            .join(format!("batfiles{}", std::env::consts::EXE_SUFFIX))
    }

    /// Its `update` against `base`, with every root left unresolvable.
    fn update(&self, base: &str) -> Command {
        self.update_at(&self.exe(), base)
    }

    fn update_at(&self, program: &Path, base: &str) -> Command {
        let mut command = batfiles_at(program);
        command
            .arg("update")
            .env("BATFILES_BASE", base)
            .env_remove("HOME")
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_CACHE_HOME")
            .current_dir(self.dir.path());
        command
    }

    /// Whether the copy is still the binary it started as.
    fn unchanged(&self) -> bool {
        fs::read(self.exe()).expect("the installed binary") == self.original
    }

    /// Nothing is left beside the binary but the binary, once whatever Windows set aside is
    /// gone, which the process `update` left to remove it does soon after `update` exits.
    fn assert_alone(&self) {
        self.assert_alone_after("");
    }

    /// [`Self::assert_alone`], after an `update` that said `said`.
    fn assert_alone_after(&self, said: &str) {
        let alone = [format!("batfiles{}", std::env::consts::EXE_SUFFIX)];
        let deadline = Instant::now() + Duration::from_secs(30);
        while entries(&self.bin_dir()) != alone && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(
            entries(&self.bin_dir()),
            alone,
            "after update said:\n{said}"
        );
    }
}

#[test]
fn a_newer_release_replaces_the_running_binary() {
    let server = serve(&[Release::of(NEWER).latest()]);
    let machine = Machine::new();
    let assertion = machine
        .update(server.address())
        .arg("-v")
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "installed batfiles {NEWER} at {}, replacing batfiles {}",
            canonical(&machine.exe()).display(),
            running()
        )),
        "{stderr}"
    );
    assert!(stderr.contains(&format!("{}/download/v{NEWER}", server.address())));

    assert_eq!(
        fs::read(machine.exe()).expect("the binary"),
        stand_in(NEWER)
    );
    #[cfg(unix)]
    {
        let mode = fs::metadata(machine.exe())
            .expect("its metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o7777, 0o755);
    }
    machine.assert_alone_after(&stderr);
    Command::new(machine.exe())
        .arg("version")
        .assert()
        .success()
        .stdout(format!("batfiles {NEWER}\n"));
}

#[test]
fn a_latest_release_that_is_not_newer_is_reported_and_nothing_is_downloaded() {
    let current: &'static str = running().leak();
    for latest in [current, OLDER] {
        let server = serve(&[Release::of(latest).latest()]);
        let machine = Machine::new();
        let assertion = machine.update(server.address()).assert().success();
        assert!(stderr_of(&assertion).contains(&format!(
            "batfiles {current} is up to date: the latest release is {latest}"
        )));
        assert_eq!(server.requests(), 1, "only VERSION is read");
        assert!(machine.unchanged());
        machine.assert_alone();
    }
}

#[test]
fn a_named_release_is_installed_even_when_it_is_older() {
    let server = serve(&[Release::of(NEWER).latest(), Release::of(OLDER)]);
    let machine = Machine::new();
    machine
        .update(server.address())
        .arg(format!("v{OLDER}"))
        .assert()
        .success();
    assert_eq!(
        fs::read(machine.exe()).expect("the binary"),
        stand_in(OLDER)
    );
}

#[test]
fn check_reads_only_the_version_and_prints_both() {
    let server = serve(&[Release::of(NEWER).latest(), Release::of(OLDER)]);
    let machine = Machine::new();
    machine
        .update(server.address())
        .arg("--check")
        .assert()
        .success()
        .stdout(format!("running {}\navailable {NEWER}\n", running()));
    assert_eq!(server.requests(), 1);

    machine
        .update(server.address())
        .args([OLDER, "--check"])
        .assert()
        .success()
        .stdout(format!("running {}\navailable {OLDER}\n", running()));
    assert_eq!(server.requests(), 2);

    let assertion = machine
        .update(server.address())
        .args(["9.9.9", "--check"])
        .assert()
        .code(1);
    assert!(stderr_of(&assertion).contains("download/v9.9.9/VERSION"));
    assert!(machine.unchanged());
    machine.assert_alone();
}

#[test]
fn a_binary_that_does_not_match_its_checksum_is_not_installed() {
    let mut release = Release::of(NEWER).latest();
    release.listed = Some(sha256(b"something else"));
    let server = serve(&[release]);
    let machine = Machine::new();
    let assertion = machine.update(server.address()).assert().code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "{}/download/v{NEWER}/{} does not match {}/download/v{NEWER}/SHA256SUMS",
            server.address(),
            asset(),
            server.address()
        )),
        "{stderr}"
    );
    assert!(machine.unchanged());
    machine.assert_alone();
}

#[test]
fn a_release_that_lists_no_binary_for_this_machine_is_not_installed() {
    let mut release = Release::of(NEWER).latest();
    release.listed = None;
    let server = serve(&[release]);
    let machine = Machine::new();
    let assertion = machine.update(server.address()).assert().code(1);
    assert!(stderr_of(&assertion).contains(&format!("SHA256SUMS lists no {}", asset())));
    assert_eq!(server.requests(), 2, "the binary is never downloaded");
    assert!(machine.unchanged());
    machine.assert_alone();
}

#[test]
fn a_binary_that_does_not_report_its_release_is_not_installed() {
    let mut release = Release::of(NEWER).latest();
    release.binary = stand_in("98.0.0");
    release.listed = Some(sha256(&release.binary));
    let server = serve(&[release]);
    let machine = Machine::new();
    let assertion = machine.update(server.address()).assert().code(1);
    assert!(stderr_of(&assertion).contains(&format!(
        "reports `batfiles 98.0.0`, not `batfiles {NEWER}`; nothing was installed"
    )));
    assert!(machine.unchanged());
    machine.assert_alone();

    let mut release = Release::of(NEWER).latest();
    release.binary = b"\x7fELF not a program".to_vec();
    release.listed = Some(sha256(&release.binary));
    let server = serve(&[release]);
    let assertion = machine.update(server.address()).assert().code(1);
    assert!(stderr_of(&assertion).contains("nothing was installed"));
    assert!(machine.unchanged());
    machine.assert_alone();
}

#[test]
#[cfg(unix)]
fn a_directory_that_cannot_be_written_fails_before_anything_is_downloaded() {
    let server = serve(&[Release::of(NEWER).latest()]);
    let machine = Machine::new();
    fs::set_permissions(machine.bin_dir(), fs::Permissions::from_mode(0o555)).expect("read-only");
    if fs::write(machine.bin_dir().join("probe"), "").is_ok() {
        // Permissions do not bind this user, as when the tests run as root.
        return;
    }
    let assertion = machine.update(server.address()).assert().code(1);
    fs::set_permissions(machine.bin_dir(), fs::Permissions::from_mode(0o755)).expect("writable");
    assert!(stderr_of(&assertion).contains(&format!(
        "cannot replace {}: could not create a file in {}",
        machine.exe().display(),
        machine.bin_dir().display()
    )));
    assert_eq!(server.requests(), 0);
    assert!(machine.unchanged());
}

#[test]
fn a_base_with_no_stable_release_suggests_naming_one() {
    let server = serve(&[Release::of("99.0.0-rc.1")]);
    let machine = Machine::new();
    let assertion = machine.update(server.address()).assert().code(1);
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("latest/download/VERSION"), "{stderr}");
    assert!(
        stderr.contains("name one, as `batfiles update <version>`"),
        "{stderr}"
    );
    machine.assert_alone();

    machine
        .update(server.address())
        .arg("99.0.0-rc.1")
        .assert()
        .success();
    assert_eq!(
        fs::read(machine.exe()).expect("the binary"),
        stand_in("99.0.0-rc.1")
    );
}

#[test]
fn a_version_document_that_holds_no_version_is_refused() {
    let server = Server::serving(vec![(
        "/latest/download/VERSION".into(),
        Reply::Body("1.2\n"),
    )]);
    let machine = Machine::new();
    let assertion = machine.update(server.address()).assert().code(1);
    assert!(stderr_of(&assertion).contains("holds `1.2`, which is not a release version"));
    assert!(machine.unchanged());
}

#[test]
fn a_file_url_base_is_a_release_tree_too() {
    let machine = Machine::new();
    let tree = machine.dir.path().join("releases");
    for (path, body) in Release::of(NEWER).latest().files() {
        let path = tree.join(path);
        fs::create_dir_all(path.parent().expect("a parent")).expect("a release directory");
        fs::write(path, body).expect("a release asset");
    }
    machine.update(&file_url(&tree)).assert().success();
    assert_eq!(
        fs::read(machine.exe()).expect("the binary"),
        stand_in(NEWER)
    );
}

#[test]
#[cfg(unix)]
fn a_symlink_to_batfiles_stays_a_link_to_the_replaced_binary() {
    let server = serve(&[Release::of(NEWER).latest()]);
    let machine = Machine::new();
    let link = machine.dir.path().join("link");
    std::os::unix::fs::symlink(machine.exe(), &link).expect("a link");
    machine
        .update_at(&link, server.address())
        .assert()
        .success();
    assert!(fs::symlink_metadata(&link).expect("the link").is_symlink());
    assert_eq!(
        fs::read(machine.exe()).expect("the binary"),
        stand_in(NEWER)
    );
}

#[test]
fn a_staged_file_left_in_the_way_is_never_replaced() {
    let server = serve(&[Release::of(NEWER).latest()]);
    let machine = Machine::new();
    let leftover = machine.bin_dir().join(STAGED);
    fs::write(&leftover, "an interrupted update").expect("a leftover");
    let assertion = machine.update(server.address()).assert().code(1);
    assert!(stderr_of(&assertion).contains(&format!(
        "{}{}{STAGED}",
        canonical(&machine.bin_dir()).display(),
        std::path::MAIN_SEPARATOR
    )));
    assert_eq!(server.requests(), 0);
    assert_eq!(
        fs::read_to_string(&leftover).expect("the leftover"),
        "an interrupted update"
    );
    assert!(machine.unchanged());
}

#[test]
fn a_malformed_version_or_base_is_refused() {
    let machine = Machine::new();
    machine
        .update("https://example.invalid")
        .arg("1.2")
        .assert()
        .code(2);
    let assertion = machine
        .update("https://example.invalid/it's")
        .arg("--check")
        .assert()
        .code(1);
    assert!(stderr_of(&assertion).contains("check BATFILES_BASE"));
    assert!(machine.unchanged());
}

#[test]
#[cfg(windows)]
fn what_an_earlier_update_set_aside_is_removed_by_the_next() {
    let server = serve(&[Release::of(NEWER).latest()]);
    let machine = Machine::new();
    fs::write(machine.bin_dir().join(SET_ASIDE), "an earlier batfiles").expect("a leftover");
    let assertion = machine
        .update(server.address())
        .arg("-v")
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("left by an earlier update"), "{stderr}");
    assert_eq!(
        fs::read(machine.exe()).expect("the binary"),
        stand_in(NEWER)
    );
    machine.assert_alone_after(&stderr);
}
