//! `dist/smoke.sh`: a published release installed here by its own one-liner. The release tree
//! holds stand-ins for every target, so whichever this machine maps to answers.

use tempfile::TempDir;

use super::support::{Tree, script, stderr_of, stdout_of};

/// A tree whose latest release is 1.2.3, with 1.3.0-rc.1 published beside it.
fn published() -> (TempDir, Tree) {
    let dir = TempDir::new().expect("a scratch directory");
    let tree = Tree::new(dir.path().join("tree"));
    tree.release("1.2.3", true);
    tree.release("1.3.0-rc.1", false);
    (dir, tree)
}

#[test]
fn a_published_release_installs_pinned_and_as_the_latest() {
    let (_dir, tree) = published();
    let assertion = script("smoke.sh", &[&tree.url(), "1.2.3", "yes"])
        .assert()
        .success();
    assert!(stdout_of(&assertion).contains("batfiles update --check finds 1.2.3"));
    let assertion = script("smoke.sh", &[&tree.url(), "1.3.0-rc.1", "no"])
        .assert()
        .success();
    assert!(!stdout_of(&assertion).contains("update --check"));
}

#[test]
fn a_release_that_is_not_the_latest_fails_as_the_latest() {
    let (_dir, tree) = published();
    let assertion = script("smoke.sh", &[&tree.url(), "1.3.0-rc.1", "yes"])
        .assert()
        .failure();
    assert!(stderr_of(&assertion).contains("gave 'batfiles 1.2.3', not 'batfiles 1.3.0-rc.1'"));
}

#[test]
fn an_unpublished_release_fails() {
    let (_dir, tree) = published();
    let assertion = script("smoke.sh", &[&tree.url(), "9.9.9", "no"])
        .assert()
        .failure();
    assert!(stderr_of(&assertion).contains("cannot fetch"));
}
