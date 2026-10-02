//! The `install.ps1` stub `batfiles init` writes, run from a checkout as `pwsh -File` runs it,
//! against a release tree of compiled stand-ins. A stand-in prints what it was asked to run.

use std::fs;
use std::path::PathBuf;

use super::support::{Machine, WINDOWS, put_stand_in, quoted, stderr_of, stdout_of};

/// A machine with a checkout at `dotfiles\`, initialized with stubs whose base is the machine's
/// release tree.
fn checkout() -> (Machine, PathBuf) {
    let machine = Machine::new();
    let dir = machine.path("dotfiles");
    fs::create_dir(&dir).expect("a checkout");
    assert_cmd::Command::cargo_bin("batfiles")
        .expect("the batfiles binary")
        .current_dir(&dir)
        .env("BATFILES_BASE", machine.tree.url())
        .args(["--quiet", "init", "--no-git-init"])
        .assert()
        .success();
    (machine, dir)
}

/// What a stand-in of `version` prints when the stub runs it on `dir` with `args`.
fn synced(version: &str, dir: &std::path::Path, args: &str) -> String {
    let args = if args.is_empty() {
        String::new()
    } else {
        format!(" {args}")
    };
    format!(
        "{WINDOWS} {version} ran: sync --bootstrap --batfiles-dir {}{args}\n",
        dir.display()
    )
}

#[test]
fn an_unpinned_stub_uses_a_batfiles_it_finds_without_the_network() {
    let (machine, dir) = checkout();
    put_stand_in(&machine.on_path().join("batfiles.exe"), "1.0.0");
    let requests = machine.tree.server.requests();

    let assertion = machine
        .pwsh_file(&dir.join("install.ps1"), &["--dry-run", "--exit", "4"])
        .assert()
        .code(4);
    assert_eq!(
        stdout_of(&assertion),
        synced("1.0.0", &dir, "--dry-run --exit 4")
    );
    assert_eq!(machine.tree.server.requests(), requests);
}

#[test]
fn a_stub_on_a_machine_without_batfiles_installs_one_and_syncs() {
    let (machine, dir) = checkout();
    let assertion = machine
        .pwsh_file(&dir.join("install.ps1"), &[])
        .assert()
        .success();
    assert_eq!(stdout_of(&assertion), synced("1.2.3", &dir, ""));
    assert_eq!(machine.installed(), ["batfiles.exe"]);
}

#[test]
fn a_pinned_stub_asks_its_release_installer_for_at_least_that_release() {
    let (machine, dir) = checkout();
    machine.tree.release("1.3.0", false);
    put_stand_in(&machine.on_path().join("batfiles.exe"), "1.2.3");

    let mut stub = machine.pwsh_file(&dir.join("install.ps1"), &[]);
    stub.env("BATFILES_VERSION", "1.3.0");
    let assertion = stub.assert().success();
    assert_eq!(stdout_of(&assertion), synced("1.3.0", &dir, ""));
    assert!(stderr_of(&assertion).contains("warning: passing over"));
}

#[test]
fn a_pinned_stub_that_cannot_reach_the_installer_uses_what_it_found() {
    let (machine, dir) = checkout();
    put_stand_in(&machine.on_path().join("batfiles.exe"), "1.0.0");

    let mut stub = machine.pwsh_file(&dir.join("install.ps1"), &[]);
    stub.env("BATFILES_VERSION", "9.9.9");
    let assertion = stub.assert().success();
    assert_eq!(stdout_of(&assertion), synced("1.0.0", &dir, ""));
    assert!(stderr_of(&assertion).contains("using it unchecked"));

    fs::remove_file(machine.on_path().join("batfiles.exe")).expect("no batfiles");
    let mut stub = machine.pwsh_file(&dir.join("install.ps1"), &[]);
    stub.env("BATFILES_VERSION", "9.9.9");
    let assertion = stub.assert().code(1);
    assert!(stderr_of(&assertion).contains("this machine has no batfiles to use instead"));
}

#[test]
fn a_stub_that_is_not_run_from_a_checkout_names_the_clone_one_liner() {
    let (machine, dir) = checkout();
    let assertion = machine
        .pwsh(&format!(
            "Get-Content -Raw {} | Invoke-Expression",
            quoted(&dir.join("install.ps1").display().to_string())
        ))
        .assert()
        .code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("this runs from a checkout of a batfiles repository"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "irm {}/latest/download/install.ps1))) clone <repository-url>",
            machine.tree.url()
        )),
        "{stderr}"
    );

    fs::remove_file(dir.join("batfiles.toml")).expect("no manifest");
    machine
        .pwsh_file(&dir.join("install.ps1"), &[])
        .assert()
        .code(1);
}
