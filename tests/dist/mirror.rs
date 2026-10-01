//! `dist/mirror.sh`: a published release, copied and restamped for another base.

use std::fs;

use tempfile::TempDir;

use super::support::{LINUX, Tree, script, stderr_of, utf8};

/// A release tree with 1.2.3 as its latest release, and a directory to mirror it into.
fn upstream() -> (TempDir, Tree) {
    let dir = TempDir::new().expect("a scratch directory");
    let tree = Tree::new(dir.path().join("upstream"));
    tree.release("1.2.3", true);
    (dir, tree)
}

#[test]
fn a_mirror_holds_the_same_release_stamped_for_its_own_base() {
    let (dir, upstream) = upstream();
    let out = dir.path().join("mirror");
    script(
        "mirror.sh",
        &[
            "latest",
            "https://mirror.example.com/batfiles",
            utf8(&out),
            &upstream.url(),
        ],
    )
    .assert()
    .success();

    let release = upstream.path("download/v1.2.3");
    for name in ["VERSION", "SHA256SUMS", &format!("batfiles-{LINUX}")] {
        assert_eq!(
            fs::read(out.join(name)).expect("a mirrored asset"),
            fs::read(release.join(name)).expect("an upstream asset"),
            "{name}"
        );
    }
    let install = fs::read_to_string(out.join("install.sh")).expect("the installer");
    assert!(install.contains("\nbatfiles_stamped_base='https://mirror.example.com/batfiles'\n"));
    let install = fs::read_to_string(out.join("install.ps1")).expect("the installer");
    assert!(install.contains("\n$BatfilesStampedBase = 'https://mirror.example.com/batfiles'\n"));

    // The mirror verifies like any release tree.
    let mirror = Tree::new(dir.path().join("served"));
    fs::create_dir_all(mirror.path("download")).expect("a release tree");
    fs::rename(&out, mirror.path("download/v1.2.3")).expect("a served release");
    script(
        "verify.sh",
        &[
            &mirror.url(),
            "1.2.3",
            "no",
            "https://mirror.example.com/batfiles",
        ],
    )
    .assert()
    .success();
}

#[test]
fn a_corrupt_release_is_not_mirrored() {
    let (dir, upstream) = upstream();
    fs::write(
        upstream.path(&format!("download/v1.2.3/batfiles-{LINUX}")),
        "tampered\n",
    )
    .expect("a corrupt binary");

    let out = dir.path().join("mirror");
    let assertion = script(
        "mirror.sh",
        &[
            "v1.2.3",
            "https://mirror.example.com/batfiles",
            utf8(&out),
            &upstream.url(),
        ],
    )
    .assert()
    .failure();
    assert!(stderr_of(&assertion).contains("does not match SHA256SUMS"));
    assert!(!out.exists());
}
