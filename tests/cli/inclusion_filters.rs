//! CLI tests for inclusion allow/deny filters. Filtered records retain addresses for reporting
//! and cannot be restored by explicit application. Use the `corporate` fixture.

use crate::support::*;

/// A leaf whose one inclusion carries the filters named, pointed at a bare
/// repository holding `corporate`.
fn filtering(filters: &str) -> (BareRepo, Tree) {
    let origin = BareRepo::from_fixture("corporate");
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{{origin}}"

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
{filters}"#
    ));
    tree.point_at_origin(&origin);
    (origin, tree)
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn an_inclusion_writing_no_filter_takes_the_whole_manifest() {
    let (_origin, tree) = filtering("");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(installed_corporate(&tree), ["zshrc", "p10k", "seeds"]);
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn an_allow_list_takes_what_it_names_and_leaves_the_rest() {
    let (_origin, tree) = filtering("install-groups = [\"shell\"]\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(
        installed_corporate(&tree),
        ["zshrc"],
        "the inclusion took something its filter did not name:\n{stderr}"
    );
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn a_deny_list_leaves_out_what_it_names_and_takes_the_rest() {
    let (_origin, tree) = filtering("exclude-actions = [\"p10k\"]\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(installed_corporate(&tree), ["zshrc", "seeds"]);
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn excluded_actions_narrow_what_a_group_filter_selected() {
    let (_origin, tree) =
        filtering("exclude-groups = [\"prompt\"]\nexclude-actions = [\"seeds\"]\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(installed_corporate(&tree), ["zshrc"]);
}

#[test]
fn an_empty_allow_list_takes_nothing_and_is_not_an_absent_one() {
    let (_origin, tree) = filtering("install-actions = []\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        installed_corporate(&tree).is_empty(),
        "an empty allow-list took something:\n{stderr}"
    );
    assert!(
        stderr.contains("include-remote corp"),
        "the inclusion did not report at its position:\n{stderr}"
    );
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn a_record_the_filters_left_out_says_so_rather_than_going_unmentioned() {
    let (_origin, tree) = filtering("install-groups = [\"shell\"]\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    for heading in [
        "symlink corp.p10k (group corp.prompt) - skipped: not selected by include-remote `corp`",
        "copy-dir corp.seeds - skipped: not selected by include-remote `corp`",
    ] {
        assert!(
            stderr.contains(heading),
            "`{heading}` was not reported:\n{stderr}"
        );
    }
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn an_inclusion_with_no_id_is_named_by_where_it_was_written() {
    let origin = BareRepo::from_fixture("corporate");
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.corporate]
type = "git"
url = "{origin}"

[[actions]]
type = "include-remote"
remote = "corporate"
exclude-actions = ["p10k"]
"#,
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("skipped: not selected by include-remote action 1 of remote `corporate`"),
        "the reason did not name the inclusion the record came from:\n{stderr}"
    );
    assert_eq!(installed_corporate(&tree), ["zshrc", "seeds"]);
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn naming_a_record_the_filters_left_out_does_not_bring_it_back() {
    let (_origin, tree) = filtering("exclude-actions = [\"p10k\"]\n");
    tree.batfiles().arg("sync").assert().success();

    let assertion = tree
        .batfiles()
        .args(["apply-action", "-v", "--id", "corp.p10k"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        !tree.home(".p10k.zsh").exists(),
        "naming the record waived the filter that left it out:\n{stderr}"
    );
    assert!(
        stderr.contains("not selected by include-remote `corp`"),
        "the run did not say why it installed nothing:\n{stderr}"
    );
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn a_command_that_named_one_record_says_when_it_carried_nothing_out() {
    let (_origin, tree) = filtering("exclude-actions = [\"p10k\"]\n");
    tree.batfiles().arg("sync").assert().success();

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "corp.p10k"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("nothing to apply"),
        "a command that installed nothing said nothing:\n{stderr}"
    );
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn a_skip_naming_a_record_the_filters_left_out_matched_something() {
    let (_origin, tree) = filtering("exclude-actions = [\"p10k\"]\n");

    let assertion = tree
        .batfiles()
        .args(["sync", "--skip-action", "corp.p10k"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        !stderr.contains("matched no action"),
        "an address the inclusion left out was reported as naming nothing:\n{stderr}"
    );
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn a_filter_name_the_remote_does_not_declare_warns_and_the_run_carries_on() {
    let (_origin, tree) = filtering("install-actions = [\"zshrc\", \"nowhere\"]\n");

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains(
            "include-remote `corp`: install-actions `nowhere` matched no action in remote `corporate`"
        ),
        "the unmatched filter name was not reported:\n{stderr}"
    );
    assert_eq!(
        installed_corporate(&tree),
        ["zshrc"],
        "the rest of the filter stopped working:\n{stderr}"
    );
}

#[test]
// Symlink actions, which are Unix-only.
#[cfg(unix)]
fn a_group_filter_is_answered_by_a_group_and_not_by_an_action_of_that_name() {
    let (_origin, tree) = filtering("exclude-groups = [\"zshrc\"]\n");

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("exclude-groups `zshrc` matched no group in remote `corporate`"),
        "an action ID satisfied a group filter:\n{stderr}"
    );
    assert_eq!(installed_corporate(&tree), ["zshrc", "p10k", "seeds"]);
}

#[test]
fn an_inclusion_may_not_describe_its_selection_twice() {
    let (_origin, tree) =
        filtering("install-actions = [\"zshrc\"]\nexclude-groups = [\"prompt\"]\n");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);

    for expected in ["`install-actions`", "`exclude-groups`", "action 1"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert!(
        !tree.path("repo/remotes/corporate").exists(),
        "the run got as far as materializing the remote:\n{stderr}"
    );
}

#[test]
fn a_filter_names_a_record_as_the_remote_declares_it() {
    let (_origin, tree) = filtering("install-actions = [\"corp.zshrc\"]\n");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("`corp.zshrc`"),
        "the refused value was not quoted back:\n{stderr}"
    );
}
