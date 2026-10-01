//! `dist/assemble.sh`: one release's asset set from staged binaries.

use std::fs;

use super::support::{LINUX, Scratch, WINDOWS, entries, script, sha256, stderr_of};

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

    let names = entries(&scratch.path("out"));
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
fn assembly_refuses_what_would_not_be_a_release() {
    let base = "https://example.com/batfiles";
    let cases: [([&str; 3], &str); 8] = [
        (["1.2", base, "out"], "is not X.Y.Z"),
        (["01.2.3", base, "out"], "is not X.Y.Z"),
        (["1.2.3-rc..1", base, "out"], "is not X.Y.Z"),
        (["1.2.3-rc.01", base, "out"], "is not X.Y.Z"),
        (["1.2.3+build", base, "out"], "is not X.Y.Z"),
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
