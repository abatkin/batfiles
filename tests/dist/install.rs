//! `dist/install.sh`, the hosted installer, piped into `sh` as the one-liner runs it, against a
//! release tree of stand-in binaries.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

#[cfg(target_os = "linux")]
use super::support::FileServer;
use super::support::{
    LINUX, Machine, dist, executable, is_stand_in, stand_in, stderr_of, stdout_of,
};

#[test]
fn a_machine_without_batfiles_gets_the_latest_release() {
    let machine = Machine::new();
    let assertion = machine.installer().assert().success();

    assert_eq!(machine.installed(), ["batfiles"]);
    assert!(is_stand_in(
        &machine.install_dir().join("batfiles"),
        LINUX,
        "1.2.3"
    ));
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("installed batfiles 1.2.3 as "), "{stderr}");
    assert!(stderr.contains("is not on PATH"), "{stderr}");
    assert_eq!(stdout_of(&assertion), "");
}

#[test]
fn with_arguments_it_runs_what_it_installed() {
    let machine = Machine::new();
    machine
        .installer()
        .args(["clone", "https://example.com/dotfiles"])
        .assert()
        .success()
        .stdout(format!(
            "{LINUX} 1.2.3 ran: clone https://example.com/dotfiles\n"
        ));
}

#[test]
fn each_platform_gets_its_own_binary() {
    let cases = [
        ("Linux", "x86_64", None, "x86_64-unknown-linux-musl"),
        ("Linux", "amd64", None, "x86_64-unknown-linux-musl"),
        ("Linux", "aarch64", None, "aarch64-unknown-linux-musl"),
        ("Linux", "arm64", None, "aarch64-unknown-linux-musl"),
        ("Darwin", "arm64", Some("1"), "aarch64-apple-darwin"),
        ("Darwin", "x86_64", None, "x86_64-apple-darwin"),
        ("Darwin", "x86_64", Some("0"), "x86_64-apple-darwin"),
        // An x86_64 shell under Rosetta.
        ("Darwin", "x86_64", Some("1"), "aarch64-apple-darwin"),
    ];
    for (os, arch, arm64, target) in cases {
        let machine = Machine::new();
        let mut installer = machine.installer();
        installer.env("FAKE_OS", os).env("FAKE_ARCH", arch);
        if let Some(arm64) = arm64 {
            installer.env("FAKE_ARM64", arm64);
        }
        installer.assert().success();
        assert!(
            is_stand_in(&machine.install_dir().join("batfiles"), target, "1.2.3"),
            "{os} {arch} {arm64:?}"
        );
    }
}

#[test]
fn a_platform_without_a_release_is_named() {
    for (os, arch) in [("FreeBSD", "amd64"), ("Linux", "riscv64")] {
        let machine = Machine::new();
        let assertion = machine
            .installer()
            .env("FAKE_OS", os)
            .env("FAKE_ARCH", arch)
            .assert()
            .code(1);
        assert!(stderr_of(&assertion).contains(&format!("no batfiles release for {os} on {arch}")));
        assert!(machine.installed().is_empty());
    }
}

#[test]
fn a_batfiles_on_path_is_used_without_a_download() {
    let machine = Machine::new();
    stand_in(&machine.on_path().join("batfiles"), "on-path", "1.0.0");

    let assertion = machine
        .installer()
        .env("BATFILES_BASE", "file:///nonexistent")
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("using batfiles 1.0.0 at "), "{stderr}");
    assert!(stderr.contains("batfiles update"), "{stderr}");

    machine
        .installer()
        .env("BATFILES_BASE", "file:///nonexistent")
        .arg("sync")
        .assert()
        .success()
        .stdout("on-path 1.0.0 ran: sync\n");
    assert!(machine.installed().is_empty());
}

#[test]
fn the_default_install_location_is_a_candidate_too() {
    let machine = Machine::new();
    fs::create_dir_all(machine.install_dir()).expect("an install directory");
    stand_in(
        &machine.install_dir().join("batfiles"),
        "installed",
        "1.0.0",
    );

    machine
        .installer()
        .env("BATFILES_BASE", "file:///nonexistent")
        .arg("sync")
        .assert()
        .success()
        .stdout("installed 1.0.0 ran: sync\n");
}

#[test]
fn a_candidate_that_reports_no_version_is_passed_over() {
    let machine = Machine::new();
    executable(&machine.on_path().join("batfiles"), "echo something else\n");

    let assertion = machine.installer().assert().success();
    assert!(stderr_of(&assertion).contains("which does not report a batfiles version"));
    assert_eq!(machine.installed(), ["batfiles"]);
}

#[test]
fn a_requested_version_is_a_floor_for_what_is_found() {
    let machine = Machine::new();
    stand_in(&machine.on_path().join("batfiles"), "on-path", "1.3.0");

    machine
        .installer()
        .env("BATFILES_BASE", "file:///nonexistent")
        .env("BATFILES_VERSION", "1.2.3")
        .arg("sync")
        .assert()
        .success()
        .stdout("on-path 1.3.0 ran: sync\n");
}

#[test]
fn versions_compare_by_semantic_version_precedence() {
    // (found, requested, whether what was found satisfies the request)
    let cases = [
        ("1.2.3", "1.2.3", true),
        ("1.10.0", "1.9.9", true),
        ("1.9.9", "1.10.0", false),
        ("1.2.3", "1.2.3-rc.1", true),
        ("1.2.3-rc.1", "1.2.3", false),
        ("1.2.3-rc.10", "1.2.3-rc.2", true),
        ("1.2.3-rc.2", "1.2.3-rc.10", false),
        ("1.2.3-rc.1.1", "1.2.3-rc.1", true),
        ("1.2.3-rc.1", "1.2.3-rc.1.1", false),
        // Numeric identifiers rank below alphanumeric ones.
        ("1.2.3-alpha", "1.2.3-1", true),
        ("1.2.3-1", "1.2.3-alpha", false),
        // Not numbers, however awk might read them.
        ("1.0.0-1e2", "1.0.0-100", true),
        ("1.0.0-100", "1.0.0-1e2", false),
        ("1.0.0-2e1", "1.0.0-1e2", true),
        ("1.0.0-1e2", "1.0.0-2e1", false),
        // ASCII, whatever the locale: upper case first.
        ("1.0.0-a", "1.0.0-B", true),
        ("1.0.0-B", "1.0.0-a", false),
        // Exactly, beyond what a floating-point number holds.
        (
            "1.0.0-rc.9007199254740993",
            "1.0.0-rc.9007199254740992",
            true,
        ),
        (
            "1.0.0-rc.9007199254740992",
            "1.0.0-rc.9007199254740993",
            false,
        ),
        ("18446744073709551617.0.0", "18446744073709551616.0.0", true),
        (
            "18446744073709551616.0.0",
            "18446744073709551617.0.0",
            false,
        ),
    ];
    let machine = Machine::new();
    for (found, requested, satisfies) in cases {
        stand_in(&machine.on_path().join("batfiles"), "on-path", found);
        let mut installer = machine.installer();
        installer
            .env("BATFILES_BASE", "file:///nonexistent")
            .env("BATFILES_VERSION", requested)
            .env("LC_ALL", "en_US.UTF-8");
        if satisfies {
            let assertion = installer.assert().success();
            assert!(
                stderr_of(&assertion).contains(&format!("using batfiles {found}")),
                "{found} should satisfy {requested}"
            );
        } else {
            let assertion = installer.assert().code(1);
            assert!(
                stderr_of(&assertion).contains("warning: passing over"),
                "{found} should not satisfy {requested}"
            );
        }
    }
}

#[test]
fn only_versions_in_the_release_grammar_are_requested_or_accepted() {
    let malformed = [
        "1.2",
        "01.2.3",
        "1.02.3",
        "1.2.3-",
        "1.2.3-rc..1",
        "1.2.3-rc.",
        "1.2.3-.rc",
        "1.2.3-rc.01",
        "1.2.3-01",
        "1.2.3+build",
        "1.2.3-rc+build",
        "1.2.3-rc_1",
    ];
    let machine = Machine::new();
    for version in malformed {
        let assertion = machine
            .installer()
            .env("BATFILES_VERSION", version)
            .assert()
            .code(1);
        assert!(stderr_of(&assertion).contains("not a version"), "{version}");

        // A batfiles reporting it is passed over as reporting no version.
        stand_in(&machine.on_path().join("batfiles"), "on-path", version);
        let assertion = machine
            .installer()
            .env("BATFILES_BASE", "file:///nonexistent")
            .assert()
            .code(1);
        assert!(
            stderr_of(&assertion).contains("which does not report a batfiles version"),
            "{version}"
        );
    }
}

#[test]
fn the_latest_release_is_read_once() {
    let machine = Machine::new();
    // As if 1.3.0 were published while the installer ran: `latest` now holds a binary and
    // checksums that belong to no one release the installer saw.
    fs::write(
        machine
            .tree
            .path(&format!("latest/download/batfiles-{LINUX}")),
        "#!/bin/sh\necho newer\n",
    )
    .expect("a newer binary");
    fs::write(machine.tree.path("latest/download/SHA256SUMS"), "").expect("newer checksums");

    let assertion = machine
        .installer()
        .arg("sync")
        .assert()
        .success()
        .stdout(format!("{LINUX} 1.2.3 ran: sync\n"));
    assert!(stderr_of(&assertion).contains("/download/v1.2.3"));

    fs::write(
        machine.tree.path("latest/download/VERSION"),
        "not a version\n",
    )
    .expect("a broken VERSION");
    fs::remove_file(machine.install_dir().join("batfiles")).expect("a fresh machine");
    let assertion = machine.installer().assert().code(1);
    assert!(stderr_of(&assertion).contains("VERSION holds 'not a version'"));
}

#[test]
fn the_installer_and_the_release_scripts_share_one_grammar() {
    let pattern = |path: PathBuf| {
        fs::read_to_string(&path)
            .expect("a script")
            .lines()
            .find(|line| line.starts_with("version_pattern="))
            .unwrap_or_else(|| panic!("{} has no version_pattern", path.display()))
            .to_string()
    };
    assert_eq!(pattern(dist("install.sh")), pattern(dist("version.sh")));
}

#[test]
fn a_relative_path_entry_finds_a_batfiles() {
    let machine = Machine::new();
    fs::create_dir(machine.path("home/rel")).expect("a relative PATH entry");
    stand_in(&machine.path("home/rel/batfiles"), "relative", "1.0.0");
    let installer = fs::read(machine.tree.path("latest/download/install.sh")).expect("installer");

    // Shells differ: bash run as `sh` reports an absolute path for a relative PATH entry, while
    // dash and bash run as itself report a relative one.
    let path = std::env::var_os("PATH").expect("a PATH");
    let shells: Vec<_> = ["dash", "bash"]
        .iter()
        .filter_map(|shell| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(shell))
                .find(|candidate| candidate.exists())
        })
        .collect();
    assert!(!shells.is_empty(), "neither dash nor bash is on PATH");

    for shell in shells {
        machine
            .piped_into(&shell, &installer)
            .current_dir(machine.path("home"))
            .env("BATFILES_BASE", "file:///nonexistent")
            .env(
                "PATH",
                format!("{}:rel:/usr/bin:/bin", machine.path("fakes").display()),
            )
            .arg("sync")
            .assert()
            .success()
            .stdout("relative 1.0.0 ran: sync\n");
    }
}

#[test]
fn an_older_batfiles_is_passed_over_and_left_alone() {
    let machine = Machine::new();
    // A release candidate ranks below its release.
    let older = machine.on_path().join("batfiles");
    stand_in(&older, "on-path", "1.2.3-rc.2");
    let before = fs::read(&older).expect("the older batfiles");

    let assertion = machine
        .installer()
        .env("BATFILES_VERSION", "v1.2.3")
        .arg("sync")
        .assert()
        .success()
        .stdout(format!("{LINUX} 1.2.3 ran: sync\n"));
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("warning: passing over ") && stderr.contains("older than 1.2.3"),
        "{stderr}"
    );
    assert_eq!(fs::read(&older).expect("the older batfiles"), before);
    assert_eq!(machine.installed(), ["batfiles"]);
}

#[test]
fn a_requested_version_is_exactly_what_is_downloaded() {
    let machine = Machine::new();
    machine.tree.release("1.3.0-rc.1", false);

    machine
        .installer()
        .env("BATFILES_VERSION", "1.3.0-rc.1")
        .arg("sync")
        .assert()
        .success()
        .stdout(format!("{LINUX} 1.3.0-rc.1 ran: sync\n"));
}

#[test]
fn a_malformed_version_request_is_refused() {
    let machine = Machine::new();
    for version in ["1.2", "latest-ish", "v1.2.3-"] {
        let assertion = machine
            .installer()
            .env("BATFILES_VERSION", version)
            .assert()
            .code(1);
        assert!(stderr_of(&assertion).contains("not a version"), "{version}");
    }
    // `latest`, like an empty request, is the latest release.
    machine
        .installer()
        .env("BATFILES_VERSION", "latest")
        .assert()
        .success();
}

#[test]
fn a_checksum_mismatch_installs_nothing() {
    let machine = Machine::new();
    fs::write(
        machine
            .tree
            .path(&format!("download/v1.2.3/batfiles-{LINUX}")),
        "#!/bin/sh\necho tampered\n",
    )
    .expect("a tampered binary");

    let assertion = machine.installer().arg("sync").assert().code(1);
    assert!(stderr_of(&assertion).contains("does not match"));
    assert!(machine.installed().is_empty(), "{:?}", machine.installed());
}

#[test]
fn a_missing_release_installs_nothing() {
    let machine = Machine::new();
    let assertion = machine
        .installer()
        .env("BATFILES_VERSION", "9.9.9")
        .assert()
        .code(1);
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("cannot download"), "{stderr}");
    // A requested release has no reason to suggest requesting one.
    assert!(!stderr.contains("set BATFILES_VERSION"), "{stderr}");
    assert!(machine.installed().is_empty());
}

#[test]
fn a_base_with_no_stable_release_suggests_a_pre_release() {
    let machine = Machine::new();
    machine.tree.release("1.3.0-rc.1", false);
    let installer = fs::read(machine.tree.path("latest/download/install.sh")).expect("installer");
    fs::remove_dir_all(machine.tree.path("latest")).expect("no stable release");

    let assertion = machine.piped(&installer).assert().code(1);
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("cannot download"), "{stderr}");
    assert!(
        stderr.contains("set BATFILES_VERSION to a pre-release"),
        "{stderr}"
    );

    machine
        .piped(&installer)
        .env("BATFILES_VERSION", "1.3.0-rc.1")
        .assert()
        .success();
}

#[test]
fn a_truncated_download_runs_nothing() {
    let machine = Machine::new();
    let whole =
        fs::read_to_string(machine.tree.path("latest/download/install.sh")).expect("the installer");
    let truncated = whole
        .strip_suffix("main \"$@\"\n")
        .expect("the installer ends by calling main");

    machine
        .piped(truncated.as_bytes())
        .arg("sync")
        .assert()
        .success()
        .stdout("")
        .stderr("");
    assert!(machine.installed().is_empty());
}

#[test]
fn the_unstamped_source_needs_a_base() {
    let machine = Machine::new();
    let source = fs::read(dist("install.sh")).expect("the source");

    let assertion = machine.piped(&source).assert().code(1);
    assert!(stderr_of(&assertion).contains("never stamped"));

    machine
        .piped(&source)
        .env("BATFILES_BASE", machine.tree.url())
        .assert()
        .success();
    assert_eq!(machine.installed(), ["batfiles"]);
}

#[test]
fn a_chosen_batfiles_is_the_only_candidate_and_where_a_download_goes() {
    let machine = Machine::new();
    stand_in(&machine.on_path().join("batfiles"), "on-path", "9.9.9");
    let chosen = machine.path("chosen/bin/batfiles");

    let assertion = machine
        .installer()
        .env("BATFILES_BIN", &chosen)
        .arg("sync")
        .assert()
        .success()
        .stdout(format!("{LINUX} 1.2.3 ran: sync\n"));
    assert!(stderr_of(&assertion).contains("chosen/bin is not on PATH"));
    assert!(is_stand_in(&chosen, LINUX, "1.2.3"));
    assert!(machine.installed().is_empty());

    // Installed, it is used without a download, still ahead of PATH.
    machine
        .installer()
        .env("BATFILES_BIN", &chosen)
        .env("BATFILES_BASE", "file:///nonexistent")
        .arg("sync")
        .assert()
        .success()
        .stdout(format!("{LINUX} 1.2.3 ran: sync\n"));
}

#[test]
fn a_relative_chosen_path_is_taken_from_the_working_directory() {
    let machine = Machine::new();
    machine
        .installer()
        .current_dir(machine.path("home"))
        .env("BATFILES_BIN", "tools/batfiles")
        .arg("sync")
        .assert()
        .success()
        .stdout(format!("{LINUX} 1.2.3 ran: sync\n"));
    assert!(is_stand_in(
        &machine.path("home/tools/batfiles"),
        LINUX,
        "1.2.3"
    ));
}

#[test]
fn something_other_than_batfiles_is_never_replaced() {
    let machine = Machine::new();
    fs::create_dir_all(machine.install_dir()).expect("an install directory");
    let squatter = machine.install_dir().join("batfiles");

    // Not a batfiles, whether it runs or not.
    for executable_mode in [true, false] {
        fs::write(&squatter, "#!/bin/sh\necho something else\n").expect("a squatter");
        let mode = if executable_mode { 0o755 } else { 0o644 };
        fs::set_permissions(&squatter, fs::Permissions::from_mode(mode)).expect("its mode");

        let assertion = machine.installer().assert().code(1);
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("is not a batfiles, so it is left as it is"),
            "{stderr}"
        );
        // Refused before anything was downloaded.
        assert!(!stderr.contains("downloading"), "{stderr}");
        assert_eq!(
            fs::read_to_string(&squatter).expect("the squatter"),
            "#!/bin/sh\necho something else\n"
        );
        assert_eq!(machine.installed(), ["batfiles"]);
    }

    // A chosen path holding something else is refused the same way, and a directory too.
    let chosen = machine.path("chosen");
    fs::write(&chosen, "not batfiles\n").expect("a chosen file");
    let assertion = machine
        .installer()
        .env("BATFILES_BIN", &chosen)
        .assert()
        .code(1);
    assert!(stderr_of(&assertion).contains("is not a batfiles"));
    assert_eq!(fs::read_to_string(&chosen).expect("it"), "not batfiles\n");

    let assertion = machine
        .installer()
        .env("BATFILES_BIN", machine.path("home"))
        .assert()
        .code(1);
    assert!(stderr_of(&assertion).contains("is a directory"));
}

#[test]
fn an_older_batfiles_at_the_install_location_is_upgraded_by_a_request() {
    let machine = Machine::new();
    fs::create_dir_all(machine.install_dir()).expect("an install directory");
    stand_in(
        &machine.install_dir().join("batfiles"),
        "installed",
        "1.0.0",
    );

    machine
        .installer()
        .env("BATFILES_VERSION", "1.2.3")
        .arg("sync")
        .assert()
        .success()
        .stdout(format!("{LINUX} 1.2.3 ran: sync\n"));
    assert_eq!(machine.installed(), ["batfiles"]);
}

// Linux only: macOS has no wget unless one is installed.
#[cfg(target_os = "linux")]
#[test]
fn wget_downloads_when_curl_is_absent() {
    let machine = Machine::new();
    let server = FileServer::new(machine.path("tree"));

    // A PATH holding only what the installer runs, with wget and no curl.
    let tools = machine.path("tools");
    fs::create_dir(&tools).expect("a tools directory");
    for tool in [
        "awk",
        "grep",
        "mkdir",
        "mktemp",
        "chmod",
        "mv",
        "rm",
        "sha256sum",
        "wget",
    ] {
        let path = std::env::var_os("PATH").expect("a PATH");
        let found = std::env::split_paths(&path)
            .map(|dir| dir.join(tool))
            .find(|path| path.exists())
            .unwrap_or_else(|| panic!("{tool} is on PATH"));
        std::os::unix::fs::symlink(found, tools.join(tool)).expect("a tool link");
    }

    machine
        .installer()
        .env("BATFILES_BASE", server.address())
        .env(
            "PATH",
            format!("{}:{}", machine.path("fakes").display(), tools.display()),
        )
        .arg("sync")
        .assert()
        .success()
        .stdout(format!("{LINUX} 1.2.3 ran: sync\n"));
}
