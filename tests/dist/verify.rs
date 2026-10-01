//! `dist/verify.sh`: checking a published release through its tree.

use std::fs;

use super::support::{LINUX, Scratch, script, stderr_of};

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
