//! Composing two remotes in one leaf, where both declare the same things.
//!
//! The `alpha` and `beta` fixtures are written to collide: one set of source
//! paths, one set of action IDs, one shared destination, and one variable name
//! each answers for itself. Everything below turns on which inclusion
//! contributed the record doing the work, which is the question a leaf composing
//! two repositories asks of every rule slice 7 built.
//!
//! The leaf is the `composed` fixture, which includes `alpha` first.

use std::fs;

use crate::support::*;

/// The leaf and the two bare repositories it names, each holding one fixture.
fn composed() -> (BareRepo, BareRepo, Tree) {
    let alpha = BareRepo::from_fixture("alpha");
    let beta = BareRepo::from_fixture("beta");
    let tree = Tree::fixture("composed");
    tree.point_remote_at("alpha", &alpha);
    tree.point_remote_at("beta", &beta);
    (alpha, beta, tree)
}

/// Where a remote's materialization lands in the leaf repository.
fn materialization(tree: &Tree, remote: &str) -> std::path::PathBuf {
    tree.path("repo").join("remotes").join(remote)
}

#[test]
fn a_record_is_read_from_the_materialization_of_the_remote_that_declared_it() {
    // Two `symlink` records, identical but for their destinations: both name
    // `files/rc`, and the path each resolves to is settled by the inclusion that
    // contributed it rather than by anything the record wrote.
    let (_alpha, _beta, tree) = composed();

    tree.batfiles().arg("sync").assert().success();

    for (dest, remote) in [(".alpharc", "alpha"), (".betarc", "beta")] {
        assert_eq!(
            fs::read_link(tree.home(dest)).expect("the link the inclusion installed"),
            materialization(&tree, remote).join("files/rc"),
            "`{dest}` was not read from the tree of the remote that declared it"
        );
    }
    // And what is behind the two links is the content of two different
    // repositories, which is the half a path comparison alone would not catch.
    let read = |dest: &str| fs::read_to_string(tree.home(dest)).expect("the link resolves");
    assert!(
        read(".alpharc").contains("ALPHA"),
        "alpha installed beta's rc"
    );
    assert!(
        read(".betarc").contains("BETA"),
        "beta installed alpha's rc"
    );
}

#[test]
fn two_inclusions_seeding_one_destination_are_settled_by_declaration_order() {
    // Both remotes seed `~/.config/tool.conf`. A seed does not replace, so the
    // one that runs first is the one that lands -- and the inclusions are two
    // positions in one list, so the order across two manifests reads exactly as
    // the order within one does.
    let (_alpha, _beta, tree) = composed();

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        fs::read_to_string(tree.home(".config/tool.conf"))
            .expect("the seed")
            .contains("profile = alpha"),
        "the second inclusion's seed replaced the first one's:\n{stderr}"
    );
    // The second says so rather than going unmentioned: it found the
    // destination taken and kept what was there.
    assert!(
        stderr.contains(&format!(
            "kept {}",
            display(&tree.home(".config/tool.conf"))
        )),
        "the inclusion that seeded nothing did not say why:\n{stderr}"
    );
}

#[test]
fn an_override_decides_one_inclusion_and_the_remotes_own_vars_decide_the_other() {
    // One name, `profile`, declared by both remotes for themselves and
    // overridden by the leaf on one of the two inclusions. Neither scope can see
    // the other's declaration, and neither can see the override the other was
    // given.
    let (_alpha, _beta, tree) = composed();

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        tree.home(".cache/alpha").is_dir(),
        "the inclusion's override did not decide the record it contributed:\n{stderr}"
    );
    assert!(
        !tree.home(".cache/beta").exists(),
        "an override reached an inclusion that did not write it:\n{stderr}"
    );
    assert!(
        stderr.contains("create-dir second.cache (group second.shell) - skipped:"),
        "the record the remote's own value closed did not say so:\n{stderr}"
    );
}

#[test]
fn one_id_declared_by_both_remotes_is_two_addresses() {
    // `rc` is an ID in each remote's own namespace, and the inclusion that
    // contributed it is what makes an address out of it. So a skip reaches one
    // of the two and leaves the other alone, though both records call
    // themselves `rc`.
    let (_alpha, _beta, tree) = composed();

    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--skip-action", "first.rc"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        !tree.home(".alpharc").exists(),
        "the skip did not reach the record it named:\n{stderr}"
    );
    assert!(
        tree.home(".betarc").is_symlink(),
        "a skip naming one inclusion's record reached the other's:\n{stderr}"
    );
}
