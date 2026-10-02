//! `dist/install.ps1`, run by `pwsh` as its one-liners run it, against a release tree of compiled
//! stand-ins.

use std::fs;

use assert_cmd::Command;

use super::support::{
    ASSET, Machine, WINDOWS, put_stand_in, quoted, sha256, stand_in, stderr_of, stdout_of,
};

#[test]
fn a_machine_without_batfiles_gets_the_latest_release() {
    let machine = Machine::new();
    let assertion = machine.installer(&[]).assert().success();

    assert_eq!(machine.installed(), ["batfiles.exe"]);
    assert_eq!(
        fs::read(machine.install_location()).expect("the binary"),
        stand_in(WINDOWS, "1.2.3")
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "install.ps1: installed batfiles 1.2.3 as {}",
            machine.install_location().display()
        )),
        "{stderr}"
    );
    // Its directory is not on PATH, which the installer never changes; it says how.
    assert!(stderr.contains("is not on PATH"), "{stderr}");
    assert!(
        stderr.contains("[Environment]::SetEnvironmentVariable('Path'"),
        "{stderr}"
    );
}

#[test]
fn piped_into_iex_it_installs_and_leaves_the_session_running() {
    let machine = Machine::new();
    let assertion = machine
        .pwsh(&format!(
            "Invoke-RestMethod {} | Invoke-Expression; 'still here'",
            quoted(&format!(
                "{}/latest/download/install.ps1",
                machine.tree.url()
            ))
        ))
        .assert()
        .success();
    assert_eq!(machine.installed(), ["batfiles.exe"]);
    assert_eq!(stdout_of(&assertion), "still here\n");
}

#[test]
fn with_arguments_it_runs_what_it_installed_and_keeps_its_status() {
    let machine = Machine::new();
    let assertion = machine.installer(&["sync", "--exit", "3"]).assert().code(3);
    assert_eq!(
        stdout_of(&assertion),
        format!("{WINDOWS} 1.2.3 ran: sync --exit 3\n")
    );
}

#[test]
fn a_batfiles_on_path_is_used_without_a_download() {
    let machine = Machine::new();
    put_stand_in(&machine.on_path().join("batfiles.exe"), "1.0.0");
    let requests = machine.tree.server.requests();

    let assertion = machine.installer(&["sync"]).assert().success();
    assert_eq!(
        stdout_of(&assertion),
        format!("{WINDOWS} 1.0.0 ran: sync\n")
    );
    // One request: the installer itself.
    assert_eq!(machine.tree.server.requests(), requests + 1);
    assert!(machine.installed().is_empty());

    let assertion = machine.installer(&[]).assert().success();
    assert!(stderr_of(&assertion).contains("to upgrade it, run 'batfiles update'"));
}

#[test]
fn a_requested_version_is_a_floor_for_what_is_found() {
    let machine = Machine::new();
    machine.tree.release("1.3.0-rc.1", false);
    put_stand_in(&machine.on_path().join("batfiles.exe"), "1.2.3");

    let mut installer = machine.installer(&["sync"]);
    installer.env("BATFILES_VERSION", "v1.2.3-rc.1");
    let assertion = installer.assert().success();
    assert_eq!(
        stdout_of(&assertion),
        format!("{WINDOWS} 1.2.3 ran: sync\n")
    );

    let mut installer = machine.installer(&["sync"]);
    installer.env("BATFILES_VERSION", "1.3.0-rc.1");
    let assertion = installer.assert().success();
    assert_eq!(
        stdout_of(&assertion),
        format!("{WINDOWS} 1.3.0-rc.1 ran: sync\n")
    );
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("warning: passing over"), "{stderr}");
    assert!(
        stderr.contains("which is older than 1.3.0-rc.1; it is left as it is"),
        "{stderr}"
    );
    assert_eq!(
        fs::read(machine.on_path().join("batfiles.exe")).expect("the older batfiles"),
        stand_in(WINDOWS, "1.2.3")
    );
}

#[test]
fn versions_compare_by_semantic_version_precedence() {
    // (found, requested, whether what was found satisfies the request)
    let cases = [
        ("1.10.0", "1.9.9", true),
        ("1.9.9", "1.10.0", false),
        ("1.2.3", "1.2.3-rc.1", true),
        ("1.2.3-rc.1", "1.2.3", false),
        ("1.2.3-rc.10", "1.2.3-rc.2", true),
        ("1.2.3-rc.1", "1.2.3-rc.1.1", false),
        ("1.2.3-alpha", "1.2.3-1", true),
        ("1.0.0-a", "1.0.0-B", true),
        ("18446744073709551617.0.0", "18446744073709551616.0.0", true),
        (
            "18446744073709551616.0.0",
            "18446744073709551617.0.0",
            false,
        ),
    ];
    let machine = Machine::new();
    for (found, requested, satisfies) in cases {
        put_stand_in(&machine.on_path().join("batfiles.exe"), found);
        let mut installer = machine.installer(&[]);
        installer
            .env("BATFILES_BASE", "http://127.0.0.1:9")
            .env("BATFILES_VERSION", requested);
        let assertion = installer.assert();
        let stderr = stderr_of(&assertion);
        if satisfies {
            assert!(
                stderr.contains(&format!("using batfiles {found}")),
                "{found} should satisfy {requested}:\n{stderr}"
            );
        } else {
            assert!(
                stderr.contains("warning: passing over"),
                "{found} should not satisfy {requested}:\n{stderr}"
            );
        }
    }
}

#[test]
fn a_malformed_version_request_is_refused() {
    let machine = Machine::new();
    for version in ["1.2", "01.2.3", "1.2.3-rc.01", "1.2.3+build", "1.2.3-"] {
        let mut installer = machine.installer(&[]);
        installer.env("BATFILES_VERSION", version);
        let assertion = installer.assert().code(1);
        assert!(
            stderr_of(&assertion).contains("not a version such as 1.2.3"),
            "{version:?}"
        );
    }
    assert!(machine.installed().is_empty());
}

#[test]
fn a_checksum_mismatch_installs_nothing() {
    let machine = Machine::new();
    fs::write(
        machine.tree.path("download/v1.2.3/SHA256SUMS"),
        format!("{}  {ASSET}\n", sha256(b"something else")),
    )
    .expect("wrong checksums");
    let assertion = machine.installer(&[]).assert().code(1);
    assert!(stderr_of(&assertion).contains("does not match"));
    assert!(machine.installed().is_empty());
}

#[test]
fn a_binary_that_reports_another_release_installs_nothing() {
    let machine = Machine::new();
    let binary = stand_in(WINDOWS, "9.9.9");
    fs::write(
        machine.tree.path(&format!("download/v1.2.3/{ASSET}")),
        &binary,
    )
    .expect("a mislabeled binary");
    fs::write(
        machine.tree.path("download/v1.2.3/SHA256SUMS"),
        format!("{}  {ASSET}\n", sha256(&binary)),
    )
    .expect("its checksums");
    let assertion = machine.installer(&[]).assert().code(1);
    assert!(stderr_of(&assertion).contains("reports version 9.9.9, not 1.2.3"));
    assert!(machine.installed().is_empty());
}

#[test]
fn a_base_with_no_stable_release_suggests_a_pre_release() {
    let machine = Machine::new();
    fs::remove_dir_all(machine.tree.path("latest")).expect("no stable release");
    let mut installer = machine.installer_from(
        &format!("{}/download/v1.2.3/install.ps1", machine.tree.url()),
        &[],
    );
    installer.env("BATFILES_BASE", machine.tree.url());
    let assertion = installer.assert().code(1);
    assert!(stderr_of(&assertion).contains("set BATFILES_VERSION to a pre-release"));
}

#[test]
fn the_unstamped_source_needs_a_base() {
    let machine = Machine::new();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("dist/install.ps1");
    let assertion = machine
        .pwsh(&format!(
            "& ([scriptblock]::Create((Get-Content -Raw {})))",
            quoted(&source.display().to_string())
        ))
        .assert()
        .code(1);
    assert!(stderr_of(&assertion).contains("never stamped with a release base"));
}

#[test]
fn each_architecture_gets_the_x86_64_binary_and_others_are_named() {
    for (arch, wow) in [("AMD64", None), ("ARM64", None), ("x86", Some("AMD64"))] {
        let machine = Machine::new();
        let mut installer = machine.installer(&[]);
        installer.env("PROCESSOR_ARCHITECTURE", arch);
        if let Some(wow) = wow {
            installer.env("PROCESSOR_ARCHITEW6432", wow);
        }
        installer.assert().success();
        assert_eq!(machine.installed(), ["batfiles.exe"], "{arch}");
    }

    let machine = Machine::new();
    let mut installer = machine.installer(&[]);
    installer.env("PROCESSOR_ARCHITECTURE", "IA64");
    let assertion = installer.assert().code(1);
    assert!(stderr_of(&assertion).contains("there is no batfiles release for Windows on IA64"));
}

#[test]
fn a_chosen_batfiles_is_the_only_candidate_and_where_a_download_goes() {
    let machine = Machine::new();
    put_stand_in(&machine.on_path().join("batfiles.exe"), "1.0.0");
    let mut installer = machine.installer(&["sync"]);
    installer.env("BATFILES_BIN", r"chosen\batfiles.exe");
    let assertion = installer.assert().success();
    assert_eq!(
        stdout_of(&assertion),
        format!("{WINDOWS} 1.2.3 ran: sync\n")
    );
    assert_eq!(
        fs::read(machine.path(r"work\chosen\batfiles.exe")).expect("the chosen batfiles"),
        stand_in(WINDOWS, "1.2.3")
    );
    assert!(machine.installed().is_empty());
}

#[test]
fn something_other_than_batfiles_is_never_replaced() {
    let machine = Machine::new();
    fs::create_dir_all(machine.install_dir()).expect("an install directory");
    fs::write(machine.install_location(), "not a program").expect("something else");
    let assertion = machine.installer(&[]).assert().code(1);
    assert!(stderr_of(&assertion).contains("is not a batfiles, so it is left as it is"));
    assert_eq!(
        fs::read_to_string(machine.install_location()).expect("what was there"),
        "not a program"
    );
    assert_eq!(machine.installed(), ["batfiles.exe"]);
}

#[test]
fn windows_powershell_is_told_to_use_powershell_7() {
    let machine = Machine::new();
    let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let assertion = Command::new(format!(
        r"{root}\System32\WindowsPowerShell\v1.0\powershell.exe"
    ))
    .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"])
    .arg(format!(
        "Invoke-RestMethod {} | Invoke-Expression",
        quoted(&format!(
            "{}/latest/download/install.ps1",
            machine.tree.url()
        ))
    ))
    .env("LOCALAPPDATA", machine.path("local"))
    .assert()
    .code(1);
    assert!(stderr_of(&assertion).contains("batfiles installs with PowerShell 7 or later"));
    assert!(machine.installed().is_empty());
}
