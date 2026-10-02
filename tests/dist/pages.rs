//! `dist/pages.sh`: the Pages site, and `dist/verify.sh` checking a site against its release tree.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::support::{Tree, entries, script, stderr_of, stdout_of, utf8};

/// A release tree whose latest release is 1.2.3, with a later 1.3.0-rc.1 beside it, and a site
/// directory holding a page and a nested asset.
fn upstream() -> (TempDir, Tree) {
    let dir = TempDir::new().expect("a scratch directory");
    let tree = Tree::new(dir.path().join("tree"));
    tree.release("1.2.3", true);
    tree.release("1.3.0-rc.1", false);
    let site = dir.path().join("site");
    fs::create_dir_all(site.join("assets")).expect("a site");
    fs::write(site.join("index.html"), "<p>batfiles</p>\n").expect("a page");
    fs::write(site.join("assets/style.css"), "p {}\n").expect("an asset");
    (dir, tree)
}

fn pages(out: &Path, from: &str, site: &Path) -> assert_cmd::Command {
    script("pages.sh", &[utf8(out), from, utf8(site)])
}

#[test]
fn a_site_holds_its_own_content_and_the_latest_stable_installers() {
    let (dir, tree) = upstream();
    let out = dir.path().join("out");
    let assertion = pages(&out, &tree.url(), &dir.path().join("site"))
        .assert()
        .success();
    assert_eq!(stdout_of(&assertion), "version=1.2.3\n");

    assert_eq!(
        entries(&out),
        ["assets", "index.html", "install.ps1", "install.sh"]
    );
    assert_eq!(
        fs::read_to_string(out.join("assets/style.css")).expect("the asset"),
        "p {}\n"
    );
    for name in ["install.sh", "install.ps1"] {
        assert_eq!(
            fs::read(out.join(name)).expect("a copied installer"),
            fs::read(tree.path(&format!("download/v1.2.3/{name}"))).expect("the installer"),
            "{name}"
        );
    }
}

#[test]
fn the_repository_site_builds() {
    let (dir, tree) = upstream();
    let out = dir.path().join("out");
    script("pages.sh", &[utf8(&out), &tree.url()])
        .assert()
        .success();

    let site = Path::new(env!("CARGO_MANIFEST_DIR")).join("site");
    assert_eq!(
        fs::read(out.join("index.html")).expect("the built page"),
        fs::read(site.join("index.html")).expect("the repository's page")
    );
    assert!(out.join("install.sh").is_file());
}

#[test]
fn a_site_is_not_built_without_a_stable_release() {
    let (dir, tree) = upstream();
    let site = dir.path().join("site");
    let out = dir.path().join("out");

    fs::write(tree.path("latest/download/VERSION"), "1.3.0-rc.1\n").expect("a pre-release");
    let prerelease = pages(&out, &tree.url(), &site).assert().failure();
    assert!(stderr_of(&prerelease).contains("names the pre-release 1.3.0-rc.1"));
    assert!(!out.exists());

    fs::remove_dir_all(tree.path("latest")).expect("no latest release");
    let none = pages(&out, &tree.url(), &site).assert().failure();
    assert!(stderr_of(&none).contains("a site needs a stable release"));
    assert!(!out.exists());
}

#[test]
fn a_site_is_not_built_over_old_content_or_with_an_installer_of_its_own() {
    let (dir, tree) = upstream();
    let site = dir.path().join("site");
    let out = dir.path().join("out");

    fs::create_dir(&out).expect("an output directory");
    fs::write(out.join("stale.html"), "").expect("old content");
    let occupied = pages(&out, &tree.url(), &site).assert().failure();
    assert!(stderr_of(&occupied).contains("is not empty"));
    assert_eq!(entries(&out), ["stale.html"]);
    fs::remove_dir_all(&out).expect("a clear output");

    fs::write(site.join("install.ps1"), "# a page of its own\n").expect("a clashing file");
    let clash = pages(&out, &tree.url(), &site).assert().failure();
    assert!(stderr_of(&clash).contains("would be replaced by the release's installer"));
    assert!(!out.exists());
}

/// A site built from `tree` and served from `dir/served`, returning its URL.
fn served_site(dir: &Path, tree: &Tree) -> String {
    let served = dir.join("served");
    pages(&served, &tree.url(), &dir.join("site"))
        .assert()
        .success();
    format!("file://{}", served.display())
}

#[test]
fn a_deployed_site_verifies_against_its_release_tree() {
    let (dir, tree) = upstream();
    let site = served_site(dir.path(), &tree);
    script("verify.sh", &[&tree.url(), "1.2.3", "yes", "", &site])
        .env("PAGES_WAIT", "0")
        .assert()
        .success();
}

#[test]
fn verification_catches_a_stale_or_missing_site() {
    let (dir, tree) = upstream();
    let site = served_site(dir.path(), &tree);
    let served = dir.path().join("served");

    fs::write(served.join("install.sh"), "# an older installer\n").expect("a stale copy");
    let stale = script("verify.sh", &[&tree.url(), "1.2.3", "yes", "", &site])
        .env("PAGES_WAIT", "0")
        .assert()
        .failure();
    assert!(stderr_of(&stale).contains("install.sh differs from"));

    fs::remove_file(served.join("install.ps1")).expect("a missing copy");
    fs::copy(
        tree.path("latest/download/install.sh"),
        served.join("install.sh"),
    )
    .expect("a current copy");
    let missing = script("verify.sh", &[&tree.url(), "1.2.3", "yes", "", &site])
        .env("PAGES_WAIT", "0")
        .assert()
        .failure();
    assert!(stderr_of(&missing).contains("install.ps1 cannot be fetched"));

    let not_latest = script("verify.sh", &[&tree.url(), "1.2.3", "no", "", &site])
        .assert()
        .failure();
    assert!(stderr_of(&not_latest).contains("serves only the latest release"));
}
