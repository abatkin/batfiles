//! CLI tests for qualified action/group addresses and inclusion selection. Use the `corporate`
//! fixture included as `corp` in leaf group `work`.

use crate::support::*;

/// Create the inclusion fixture with an unmaterialized corporate remote.
fn including_unmaterialized() -> (BareRepo, Tree) {
    let origin = BareRepo::from_fixture("corporate");
    let tree = Tree::fixture("inclusion");
    tree.point_at_origin(&origin);
    (origin, tree)
}

#[cfg(unix)]
/// Create and synchronize the inclusion fixture.
fn synchronized() -> (BareRepo, Tree) {
    let (origin, tree) = including_unmaterialized();
    tree.batfiles().arg("sync").assert().success();
    (origin, tree)
}

#[cfg(unix)]
/// Create the synchronized fixture, then remove its installed corporate content.
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

#[cfg(unix)]
/// Return whether the included `zshrc` and `seeds` actions are installed.
fn installed(tree: &Tree) -> (bool, bool) {
    let installed = installed_corporate(tree);
    (installed.contains(&"zshrc"), installed.contains(&"seeds"))
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
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
// Symlink actions, which are Unix-only.
#[cfg(unix)]
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
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn asking_for_the_inclusion_asks_for_everything_it_contributed() {
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
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn an_unqualified_name_reaches_the_leaf_repository_alone() {
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
// Symlink actions, which are Unix-only.
#[cfg(unix)]
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
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn a_qualified_disable_leaves_out_one_contributed_action() {
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
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn excluding_the_inclusion_leaves_its_manifest_unread() {
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
    assert!(tree.home(".cache/zsh").is_dir(), "the leaf's action ran");
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn a_skip_qualified_by_an_unopened_inclusion_is_not_reported_as_matching_nothing() {
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
    assert!(
        stderr.contains("`nowhere` matched no action"),
        "a name nothing answers was not reported:\n{stderr}"
    );
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn a_skip_qualified_by_an_inclusion_with_nothing_to_read_is_treated_the_same_way() {
    let origin = BareRepo::from_fixture("corporate");

    for (args, tree) in [
        (vec!["sync", "--dry-run"], {
            let tree = Tree::fixture("inclusion");
            tree.point_at_origin(&origin);
            tree
        }),
        (vec!["sync", "--var", "work=false"], {
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
fn a_skip_qualified_by_an_inclusion_that_was_read_and_holds_nothing_is_reported() {
    let origin = BareRepo::new();
    origin.publish("batfiles.toml", "", "a remote with no actions");
    let tree = Tree::fixture("inclusion");
    tree.point_at_origin(&origin);

    let assertion = tree
        .batfiles()
        .args(["sync", "--skip-action", "corp.zshrc"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("`corp.zshrc` matched no action"),
        "a name an opened inclusion could not answer went unreported:\n{stderr}"
    );
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn an_inclusion_opened_to_reach_a_group_is_not_an_applied_action() {
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
// Symlink actions, which are Unix-only.
#[cfg(unix)]
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

#[cfg(unix)]
/// Rewrite the leaf manifest, replacing `from` with `to` once.
fn edit_manifest(tree: &Tree, from: &str, to: &str) {
    let manifest = std::fs::read_to_string(tree.manifest()).expect("the leaf manifest");
    assert!(manifest.contains(from), "the manifest has no `{from}`");
    tree.write_manifest(&manifest.replacen(from, to, 1));
}

#[cfg(unix)]
/// One way to exclude `corp` on this machine.
struct Excluding {
    case: &'static str,
    /// Applies it to a ready tree.
    exclude: fn(&Tree),
    /// The reason a run gives for it.
    reason: &'static str,
}

#[cfg(unix)]
/// Each way to exclude `corp` on this machine.
const EXCLUDING_CORP: [Excluding; 4] = [
    Excluding {
        case: "disabled",
        exclude: |tree| {
            tree.batfiles()
                .args(["disable-action", "corp"])
                .assert()
                .success();
        },
        reason: "action `corp` is disabled",
    },
    Excluding {
        case: "group disabled",
        exclude: |tree| {
            tree.batfiles()
                .args(["disable-group", "work"])
                .assert()
                .success();
        },
        reason: "group `work` is disabled",
    },
    Excluding {
        case: "condition",
        exclude: |tree| {
            edit_manifest(
                tree,
                "remote = \"corporate\"\n",
                "remote = \"corporate\"\nwhen = \"work\"\n",
            );
            tree.batfiles()
                .args(["vars", "set", "work", "false"])
                .assert()
                .success();
        },
        reason: "when \"work\" is false",
    },
    Excluding {
        case: "remote's condition",
        exclude: |tree| {
            edit_manifest(
                tree,
                "[remotes.corporate]\n",
                "[remotes.corporate]\nwhen = \"work\"\n",
            );
            tree.batfiles()
                .args(["vars", "set", "work", "false"])
                .assert()
                .success();
        },
        reason: "remote `corporate` is excluded here: when \"work\" is false",
    },
];

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn naming_an_included_action_does_not_waive_its_inclusions_exclusions() {
    for Excluding {
        case,
        exclude,
        reason,
    } in EXCLUDING_CORP
    {
        let (_origin, tree) = ready();
        exclude(&tree);

        let assertion = tree
            .batfiles()
            .args(["apply-action", "--id", "corp.zshrc"])
            .assert()
            .failure()
            .code(1);
        let stderr = stderr_of(&assertion);

        let expected = format!(
            "action `corp.zshrc` would come from include-remote `corp`, which is excluded: {reason}"
        );
        assert!(stderr.contains(&expected), "{case}:\n{stderr}");
        assert_eq!(installed(&tree), (false, false), "{case}: something ran");
    }
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn naming_an_included_group_does_not_waive_its_inclusions_exclusions() {
    let (_origin, tree) = ready();
    tree.batfiles()
        .args(["disable-group", "work"])
        .assert()
        .success();

    let assertion = tree
        .batfiles()
        .args(["apply-group", "--group", "corp.shell"])
        .assert()
        .failure()
        .code(1);
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains(
            "group `corp.shell` would come from include-remote `corp`, which is excluded: \
             group `work` is disabled"
        ),
        "{stderr}"
    );
    assert_eq!(installed(&tree), (false, false), "something ran");
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn naming_the_inclusions_own_group_still_waives_it() {
    let (_origin, tree) = ready();
    tree.batfiles()
        .args(["disable-group", "work"])
        .assert()
        .success();

    tree.batfiles()
        .args(["apply-group", "--group", "work"])
        .assert()
        .success();

    assert_eq!(installed(&tree), (true, true), "the group's disable held");
}

#[test]
fn naming_an_action_in_an_unmaterialized_inclusion_says_to_synchronize() {
    let (_origin, tree) = including_unmaterialized();

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "corp.zshrc"])
        .assert()
        .failure()
        .code(1);
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains(
            "action `corp.zshrc` would come from include-remote `corp`, which cannot be read: \
             remote `corporate` is not materialized; run `batfiles sync` to bring it down"
        ),
        "{stderr}"
    );
    assert!(!stderr.contains("has the id"), "{stderr}");
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn two_inclusions_of_one_remote_are_two_sets_of_records() {
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
    assert_eq!(
        installed(&tree),
        (true, true),
        "the inclusion that was not skipped did not install"
    );
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn an_inclusion_written_without_an_id_runs_and_answers_to_nothing() {
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
