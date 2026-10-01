//! The stub `batfiles init` writes, run from a checkout as `./install.sh` is, against a release
//! tree of stand-ins. The stub `exec`s whichever batfiles it settles on, and a stand-in prints
//! what it was asked to run.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

use super::support::{LINUX, Machine, dist, stand_in, stderr_of};

/// A machine with a checkout at `dotfiles/`, initialized with a stub whose base is the machine's
/// release tree. The checkout's path is canonical, as the stub's `pwd` reports it when the
/// temporary directory lies behind a symlink, as macOS's `/var` does.
fn checkout() -> (Machine, PathBuf) {
    let machine = Machine::new();
    let dir = machine.path("dotfiles");
    fs::create_dir(&dir).expect("a checkout");
    let dir = dir.canonicalize().expect("a canonical checkout path");
    Command::cargo_bin("batfiles")
        .expect("the batfiles binary")
        .current_dir(&dir)
        .env("BATFILES_BASE", machine.tree.url())
        .args(["--quiet", "init", "--no-git-init"])
        .assert()
        .success();
    (machine, dir)
}

/// The stub, run by its absolute path from the machine's home.
fn stub(machine: &Machine, dir: &Path) -> Command {
    let mut command = machine.shell(Path::new("/bin/sh"));
    command
        .current_dir(machine.path("home"))
        .arg(dir.join("install.sh"));
    command
}

/// What a stand-in prints when the stub runs it on `dir` with `args`.
fn synced(who: &str, version: &str, dir: &Path, args: &str) -> String {
    let args = if args.is_empty() {
        String::new()
    } else {
        format!(" {args}")
    };
    format!(
        "{who} {version} ran: sync --bootstrap --batfiles-dir {}{args}\n",
        dir.display()
    )
}

#[test]
fn an_unpinned_stub_uses_a_batfiles_it_finds_without_the_network() {
    let (machine, dir) = checkout();
    stand_in(&machine.on_path().join("batfiles"), "on-path", "1.0.0");

    stub(&machine, &dir)
        .env("BATFILES_BASE", "file:///nonexistent")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(synced("on-path", "1.0.0", &dir, "--dry-run"));
}

#[test]
fn a_machine_without_batfiles_gets_one_from_the_hosted_installer() {
    let (machine, dir) = checkout();

    stub(&machine, &dir)
        .args(["--disable-group", "gui"])
        .assert()
        .success()
        .stdout(synced(LINUX, "1.2.3", &dir, "--disable-group gui"));
    assert_eq!(machine.installed(), ["batfiles"]);
}

#[test]
fn run_by_a_relative_name_it_still_finds_its_checkout() {
    let (machine, dir) = checkout();
    stand_in(&machine.on_path().join("batfiles"), "on-path", "1.0.0");

    machine
        .shell(Path::new("/bin/sh"))
        .current_dir(&dir)
        .arg("install.sh")
        .assert()
        .success()
        .stdout(synced("on-path", "1.0.0", &dir, ""));
}

#[test]
fn piped_into_sh_it_points_at_the_clone_one_liner() {
    let (machine, dir) = checkout();
    let script = fs::read(dir.join("install.sh")).expect("the stub");

    // Even from inside the checkout: piped, the stub has no file of its own to find it by.
    for cwd in [machine.path("home"), dir.clone()] {
        let assertion = machine.piped(&script).current_dir(&cwd).assert().code(1);
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(&format!(
                "curl -fsSL {}/latest/download/install.sh | sh -s -- clone",
                machine.tree.url()
            )),
            "{stderr}"
        );
        assert!(machine.installed().is_empty());
    }
}

#[test]
fn a_pin_passes_over_an_older_batfiles() {
    let (machine, dir) = checkout();
    stand_in(&machine.on_path().join("batfiles"), "on-path", "1.0.0");

    let assertion = stub(&machine, &dir)
        .env("BATFILES_VERSION", "1.2.3")
        .assert()
        .success()
        .stdout(synced(LINUX, "1.2.3", &dir, ""));
    assert!(stderr_of(&assertion).contains("warning: passing over"));
    assert_eq!(machine.installed(), ["batfiles"]);
}

#[test]
fn a_pin_that_what_is_found_meets_uses_it() {
    let (machine, dir) = checkout();
    stand_in(&machine.on_path().join("batfiles"), "on-path", "1.3.0");

    stub(&machine, &dir)
        .env("BATFILES_VERSION", "v1.2.3")
        .assert()
        .success()
        .stdout(synced("on-path", "1.3.0", &dir, ""));
    assert!(machine.installed().is_empty());
}

#[test]
fn offline_a_pinned_stub_uses_what_it_finds_unchecked() {
    let (machine, dir) = checkout();
    stand_in(&machine.on_path().join("batfiles"), "on-path", "1.0.0");

    let assertion = stub(&machine, &dir)
        .env("BATFILES_BASE", "file:///nonexistent")
        .env("BATFILES_VERSION", "1.2.3")
        .assert()
        .success()
        .stdout(synced("on-path", "1.0.0", &dir, ""));
    assert!(stderr_of(&assertion).contains("using it unchecked"));
}

#[test]
fn offline_with_no_batfiles_it_fails() {
    let (machine, dir) = checkout();

    let assertion = stub(&machine, &dir)
        .env("BATFILES_BASE", "file:///nonexistent")
        .assert()
        .code(1);
    assert!(stderr_of(&assertion).contains("cannot fetch"));
}

#[test]
fn a_chosen_batfiles_is_the_only_candidate() {
    let (machine, dir) = checkout();
    stand_in(&machine.on_path().join("batfiles"), "on-path", "1.0.0");
    let chosen = machine.path("chosen");
    stand_in(&chosen, "chosen", "1.1.0");

    stub(&machine, &dir)
        .env("BATFILES_BIN", &chosen)
        .assert()
        .success()
        .stdout(synced("chosen", "1.1.0", &dir, ""));
}

#[test]
fn the_stub_finds_batfiles_exactly_as_the_installer_does() {
    // The function or line beginning with `start`, through the line that ends it.
    let excerpt = |path: &Path, start: &str, end: &str| -> String {
        let text = fs::read_to_string(path).expect("a script");
        let from = text
            .find(start)
            .unwrap_or_else(|| panic!("{} has no `{start}`", path.display()));
        let length = text[from..].find(end).expect("its end") + end.len();
        text[from..from + length].to_string()
    };
    let installer = dist("install.sh");
    let stub = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/stub.sh");
    for (start, end) in [
        ("version_pattern=", "\n"),
        ("is_version() {", "\n}\n"),
        ("version_of() {", "\n}\n"),
        ("candidates() {", "\n}\n"),
    ] {
        assert_eq!(
            excerpt(&installer, start, end),
            excerpt(&stub, start, end),
            "`{start}` differs between install.sh and the stub"
        );
    }
}

#[test]
fn the_leaf_fixture_carries_the_stub_init_writes() {
    let dir = tempfile::TempDir::new().expect("a scratch directory");
    Command::cargo_bin("batfiles")
        .expect("the batfiles binary")
        .current_dir(dir.path())
        .env_remove("BATFILES_BASE")
        .args(["--quiet", "init", "--no-git-init"])
        .assert()
        .success();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/leaf/install.sh");
    assert_eq!(
        fs::read_to_string(&fixture).expect("the fixture's stub"),
        fs::read_to_string(dir.path().join("install.sh")).expect("the written stub"),
        "regenerate tests/fixtures/leaf/install.sh with `batfiles init`"
    );
}
