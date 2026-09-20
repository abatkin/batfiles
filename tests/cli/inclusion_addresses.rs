//! Naming what an inclusion contributed.
//!
//! Two rules, read from both sides: an included record answers to its qualified
//! address and to no unqualified one, and the inclusion is itself a record in
//! the list rather than a phase beside it.
//!
//! The leaf is the `inclusion` fixture, whose `corp` inclusion sits in the leaf
//! group `work` and contributes `zshrc` (in the included group `shell`), `p10k`
//! (in `prompt`), and `seeds` (in no group). Which of those an inclusion takes
//! is `inclusion_filters.rs`; this one is about what they are called.

use crate::support::*;

/// The leaf with nothing brought down yet, for the cases about a group whose
/// only member is an inclusion the run cannot read.
fn including_unmaterialized() -> (BareRepo, Tree) {
    let origin = BareRepo::from_fixture("corporate");
    let tree = Tree::fixture("inclusion");
    tree.point_at_origin(&origin);
    (origin, tree)
}

/// The same leaf with the remote already materialized, so that every case below
/// is about naming rather than about fetching.
fn synchronized() -> (BareRepo, Tree) {
    let (origin, tree) = including_unmaterialized();
    tree.batfiles().arg("sync").assert().success();
    (origin, tree)
}

/// The same again with the contributed actions uninstalled, for the cases about
/// what a later run does or does not do.
fn ready() -> (BareRepo, Tree) {
    let (origin, tree) = synchronized();
    for CorporateAction { dest, .. } in CORPORATE_ACTIONS {
        let path = tree.home(dest);
        if path.is_dir() {
            std::fs::remove_dir_all(&path).expect("the installed directory");
        } else {
            std::fs::remove_file(&path).expect("the installed link");
        }
    }
    (origin, tree)
}

/// Whether `zshrc` and `seeds` are installed: the two records every address
/// below either reaches or does not, `zshrc` being the one in a group and
/// `seeds` the one in none.
fn installed(tree: &Tree) -> (bool, bool) {
    let installed = installed_corporate(tree);
    (installed.contains(&"zshrc"), installed.contains(&"seeds"))
}

#[test]
fn a_qualified_address_applies_one_contributed_action() {
    let (_origin, tree) = ready();

    tree.batfiles()
        .args(["apply-action", "--id", "corp.zshrc"])
        .assert()
        .success();

    assert_eq!(
        installed(&tree),
        (true, false),
        "`corp.zshrc` reached the wrong records"
    );
}

#[test]
fn a_qualified_group_applies_what_the_inclusion_declared_under_it() {
    let (_origin, tree) = ready();

    tree.batfiles()
        .args(["apply-group", "--group", "corp.shell"])
        .assert()
        .success();

    assert_eq!(
        installed(&tree),
        (true, false),
        "`corp.shell` reached the wrong records"
    );
}

#[test]
fn asking_for_the_inclusion_asks_for_everything_it_contributed() {
    // The inclusion is in the leaf group `work`, and what it brings in comes
    // with it: naming the record is naming its contents.
    let (_origin, tree) = ready();

    tree.batfiles()
        .args(["apply-group", "--group", "work"])
        .assert()
        .success();

    assert_eq!(
        installed(&tree),
        (true, true),
        "the group holding the inclusion did not reach its contents"
    );
}

#[test]
fn an_unqualified_name_reaches_the_leaf_repository_alone() {
    // The included manifest declares `zshrc` in group `shell`, and so could the
    // leaf. Neither spelling crosses over.
    let (_origin, tree) = ready();

    for (option, name) in [("--id", "zshrc"), ("--group", "shell")] {
        let command = if option == "--id" {
            "apply-action"
        } else {
            "apply-group"
        };
        let assertion = tree
            .batfiles()
            .args([command, option, name])
            .assert()
            .failure()
            .code(1);
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(&format!("`{name}`")),
            "`{name}` was not reported as naming nothing in the leaf:\n{stderr}"
        );
    }
    assert_eq!(
        installed(&tree),
        (false, false),
        "an unqualified name reached what an inclusion contributed"
    );
}

#[test]
fn a_qualified_skip_leaves_out_one_contributed_action() {
    let (_origin, tree) = ready();

    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--skip-action", "corp.zshrc"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains(
            "symlink corp.zshrc (group corp.shell) - skipped: `corp.zshrc` from --skip-action"
        ),
        "the skip did not name the record it left out:\n{stderr}"
    );
    assert!(
        !stderr.contains("matched no action"),
        "a skip an inclusion answers was reported as matching nothing:\n{stderr}"
    );
    assert_eq!(
        installed(&tree),
        (false, true),
        "the skip reached the wrong records"
    );
}

#[test]
fn a_qualified_disable_leaves_out_one_contributed_action() {
    // The persistent half of the same rule, and the reason `ItemAddress` keeps
    // an address that resolves to nothing: one written before the inclusion
    // existed starts matching when it does.
    let (_origin, tree) = ready();

    tree.batfiles()
        .args(["disable-action", "corp.seeds"])
        .assert()
        .success();
    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        installed(&tree),
        (true, false),
        "the disabled address reached the wrong records"
    );
}

#[test]
fn excluding_the_inclusion_leaves_its_manifest_unread() {
    // Coarser than any address into it: the record is passed over, so nothing it
    // would have contributed is in the run's list to be named at all.
    let (_origin, tree) = ready();

    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--skip-action", "corp"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("include-remote corp (group work) - skipped: `corp` from --skip-action"),
        "the inclusion was not reported as skipped:\n{stderr}"
    );
    assert!(
        !stderr.contains("included 2 actions"),
        "a skipped inclusion read its manifest anyway:\n{stderr}"
    );
    assert_eq!(
        installed(&tree),
        (false, false),
        "a skipped inclusion contributed actions anyway"
    );
    // The leaf's own record is untouched by the skip.
    assert!(tree.home(".cache/zsh").is_dir(), "the leaf's action ran");
}

#[test]
fn a_skip_qualified_by_an_unopened_inclusion_is_not_reported_as_matching_nothing() {
    // The run never read the list that would have answered it, so it is neither
    // matched nor unmatched, and saying either would be an invention.
    let (_origin, tree) = ready();

    let assertion = tree
        .batfiles()
        .args([
            "sync",
            "--skip-action",
            "corp",
            "--skip-action",
            "corp.zshrc",
            "--skip-action",
            "nowhere",
        ])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        !stderr.contains("`corp.zshrc` matched no action"),
        "a name an unopened inclusion might have answered was reported as matching nothing:\n{stderr}"
    );
    // A name nothing could ever answer is still reported, so the silence above
    // is about the inclusion rather than about the warning being gone.
    assert!(
        stderr.contains("`nowhere` matched no action"),
        "a name nothing answers was not reported:\n{stderr}"
    );
}

#[test]
fn a_skip_qualified_by_an_inclusion_with_nothing_to_read_is_treated_the_same_way() {
    // An inclusion the run opened and could not read is in the same position as
    // one it never opened: the list that would have answered the name was never
    // read either way. Both an absent materialization and a remote this machine
    // excludes leave it there.
    let origin = BareRepo::from_fixture("corporate");

    for (args, tree) in [
        (vec!["sync", "--dry-run"], {
            // Nothing materialized, and a dry run fetches nothing.
            let tree = Tree::fixture("inclusion");
            tree.point_at_origin(&origin);
            tree
        }),
        (vec!["sync", "--var", "work=false"], {
            // Materialized, and then closed by the remote's own condition.
            let (_, tree) = synchronized();
            let manifest = std::fs::read_to_string(tree.manifest()).expect("the fixture manifest");
            tree.write_manifest(&manifest.replace(
                "[remotes.corporate]\ntype = \"git\"",
                "[remotes.corporate]\nwhen = \"work\"\ntype = \"git\"",
            ));
            tree
        }),
    ] {
        let assertion = tree
            .batfiles()
            .args(&args)
            .args(["--skip-action", "corp.zshrc", "--skip-action", "nowhere"])
            .assert()
            .success();
        let stderr = stderr_of(&assertion);

        assert!(
            !stderr.contains("`corp.zshrc` matched no action"),
            "{args:?}: a name the run could not read was reported as matching nothing:\n{stderr}"
        );
        assert!(
            stderr.contains("`nowhere` matched no action"),
            "{args:?}: a name nothing answers was not reported:\n{stderr}"
        );
    }
}

#[test]
fn an_inclusion_opened_to_reach_a_group_is_not_an_applied_action() {
    // `apply-group` says so when it applies nothing, and an inclusion installs
    // nothing: the run opened `corp` only to reach the group named inside it, so
    // it cannot stand in for the one action that group holds.
    let (_origin, tree) = ready();

    let assertion = tree
        .batfiles()
        .args([
            "apply-group",
            "--group",
            "corp.shell",
            "--skip-action",
            "corp.zshrc",
        ])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("nothing to apply"),
        "a group whose only member was skipped reported nothing:\n{stderr}"
    );
    assert_eq!(installed(&tree), (false, false), "something ran after all");
}

#[test]
fn a_group_whose_inclusion_brought_nothing_in_says_so_without_blaming_a_skip() {
    // The other side of the same count, and why the line names one more reason
    // than it used to: the group holds one record, the inclusion was carried
    // out, and nothing in it was disabled, skipped, or excluded.
    let (_origin, tree) = including_unmaterialized();

    let assertion = tree
        .batfiles()
        .args(["apply-group", "--group", "work"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("warning: remote `corporate` is not materialized"),
        "the reason the group came to nothing was not reported:\n{stderr}"
    );
    assert!(
        stderr.contains("nothing to apply"),
        "the group applied nothing and did not say so:\n{stderr}"
    );
    assert!(
        stderr.contains("or was not contributed"),
        "the line did not admit the reason that actually applied:\n{stderr}"
    );
}

#[test]
fn a_qualified_address_that_names_nothing_is_a_failure_like_any_other() {
    let (_origin, tree) = ready();

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "corp.nowhere"])
        .assert()
        .failure()
        .code(1);
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("`corp.nowhere`"),
        "the address that resolved to nothing was not named:\n{stderr}"
    );
    assert_eq!(
        installed(&tree),
        (false, false),
        "a target that resolved to nothing installed something"
    );
}

#[test]
fn two_inclusions_of_one_remote_are_two_sets_of_records() {
    // One remote, one materialization, and two inclusions of it. Each gets its
    // own copy of what the manifest declares, under its own `id`, so a name that
    // reaches one reaches neither the other nor both.
    let origin = BareRepo::from_fixture("corporate");
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{}"

[[actions]]
type = "include-remote"
id = "first"
remote = "corporate"

[[actions]]
type = "include-remote"
id = "second"
remote = "corporate"
"#,
        display(&origin.origin())
    ));

    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--skip-action", "first.zshrc"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    // Both read the one materialization its ID owns, and both contribute: the
    // remote declares `seeds`, and each inclusion has a copy of it.
    assert!(
        stderr.contains("copy-dir first.seeds") && stderr.contains("copy-dir second.seeds"),
        "the two inclusions did not each contribute what the remote declares:\n{stderr}"
    );
    assert!(
        stderr.contains("symlink first.zshrc (group first.shell) - skipped"),
        "the skip did not reach the inclusion it named:\n{stderr}"
    );
    assert!(
        stderr.contains("symlink second.zshrc (group second.shell)")
            && !stderr.contains("symlink second.zshrc (group second.shell) - skipped"),
        "a name qualified by one inclusion reached the other:\n{stderr}"
    );
    // The second inclusion still installed it, so skipping one copy of a record
    // is not skipping the other.
    assert_eq!(
        installed(&tree),
        (true, true),
        "the inclusion that was not skipped did not install"
    );
}

#[test]
fn an_inclusion_written_without_an_id_runs_and_answers_to_nothing() {
    // Contributed actions still install, because running is not the same as
    // being addressable. Nothing can name them: not the qualified spelling,
    // which has no first segment to match, and not the unqualified one, which
    // means the leaf.
    let origin = BareRepo::from_fixture("corporate");
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{}"

[[actions]]
type = "include-remote"
remote = "corporate"
"#,
        display(&origin.origin())
    ));

    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--skip-action", "zshrc"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert_eq!(
        installed(&tree),
        (true, true),
        "an inclusion with no `id` did not contribute its actions"
    );
    // Named as the manifest that declared it names it, since no address reaches
    // it, with the inclusion it came from said in words: that is the whole of
    // what tells it from the leaf's own `zshrc` and from a second inclusion of
    // the same remote.
    assert!(
        stderr.contains(
            "symlink zshrc (group shell, from include-remote action 1 of remote `corporate`)"
        ),
        "the contributed record was not named as its own manifest names it:\n{stderr}"
    );
    assert!(
        stderr.contains("`zshrc` matched no action"),
        "an unqualified skip was answered by something an inclusion contributed:\n{stderr}"
    );
}
