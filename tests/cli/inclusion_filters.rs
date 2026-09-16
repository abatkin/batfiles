//! Which of a remote's actions an inclusion takes.
//!
//! The four filters are the leaf repository saying what it composes, which is a
//! different question from what this machine leaves out. So a record they leave
//! out is not dropped: it keeps the address that reaches it, the run says why it
//! was passed over, and naming it directly does not bring it back — the waiver
//! `apply-action` carries covers the lists this machine keeps, not the leaf's
//! description of what it took.
//!
//! The remote is the `corporate` fixture, whose three records are shaped for
//! exactly this: `zshrc` in group `shell`, `p10k` in group `prompt`, and `seeds`
//! in no group at all.

use crate::support::*;

/// The three destinations `corporate` installs at, in declaration order.
const DESTS: [&str; 3] = [".zshrc.corporate", ".p10k.zsh", ".config/corporate"];

/// A leaf whose one inclusion carries the filters named, pointed at a bare
/// repository holding `corporate`.
fn filtering(filters: &str) -> (BareRepo, Tree) {
    let origin = BareRepo::from_fixture("corporate");
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[remotes.corporate]\ntype = \"git\"\nurl = \"{{origin}}\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"corp\"\nremote = \"corporate\"\n{filters}"
    ));
    tree.point_at_origin(&origin);
    (origin, tree)
}

/// Which of the three the run installed, by the ID that contributed each.
fn installed(tree: &Tree) -> Vec<&'static str> {
    ["zshrc", "p10k", "seeds"]
        .into_iter()
        .zip(DESTS)
        .filter(|(_, dest)| tree.home(dest).exists())
        .map(|(id, _)| id)
        .collect()
}

#[test]
fn an_inclusion_writing_no_filter_takes_the_whole_manifest() {
    // The baseline every case below is a departure from.
    let (_origin, tree) = filtering("");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(installed(&tree), ["zshrc", "p10k", "seeds"]);
}

#[test]
fn an_allow_list_takes_what_it_names_and_leaves_the_rest() {
    let (_origin, tree) = filtering("install-groups = [\"shell\"]\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(
        installed(&tree),
        ["zshrc"],
        "the inclusion took something its filter did not name:\n{stderr}"
    );
}

#[test]
fn a_deny_list_leaves_out_what_it_names_and_takes_the_rest() {
    let (_origin, tree) = filtering("exclude-actions = [\"p10k\"]\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(installed(&tree), ["zshrc", "seeds"]);
}

#[test]
fn excluded_actions_narrow_what_a_group_filter_selected() {
    // The combination the two halves exist for: a group, less one of its
    // members.
    let (_origin, tree) =
        filtering("exclude-groups = [\"prompt\"]\nexclude-actions = [\"seeds\"]\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(installed(&tree), ["zshrc"]);
}

#[test]
fn an_empty_allow_list_takes_nothing_and_is_not_an_absent_one() {
    let (_origin, tree) = filtering("install-actions = []\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        installed(&tree).is_empty(),
        "an empty allow-list took something:\n{stderr}"
    );
    // The inclusion itself still ran: it read the manifest and contributed
    // records, all of which its own filter then passed over.
    assert!(
        stderr.contains("include-remote corp"),
        "the inclusion did not report at its position:\n{stderr}"
    );
}

#[test]
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
fn an_inclusion_with_no_id_names_the_remote_it_left_the_record_out_of() {
    // The only other thing such a record can be called. What it contributed
    // answers to no address, so the reason is all a reader gets.
    let origin = BareRepo::from_fixture("corporate");
    let tree = Tree::new();
    tree.write_manifest(
        "[remotes.corporate]\ntype = \"git\"\nurl = \"{origin}\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nremote = \"corporate\"\n\
         exclude-actions = [\"p10k\"]\n",
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("skipped: not selected by the include-remote of remote `corporate`"),
        "the reason did not name the remote the record came from:\n{stderr}"
    );
    assert_eq!(installed(&tree), ["zshrc", "seeds"]);
}

#[test]
fn naming_a_record_the_filters_left_out_does_not_bring_it_back() {
    // `apply-action` waives what this machine keeps: its disabled lists, its
    // skips, and the record's own condition. The filters are not that. They are
    // the leaf saying what it took from the remote, so they hold under every
    // command, as a remote's own condition does.
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
fn a_command_that_named_one_record_says_when_it_carried_nothing_out() {
    // At every verbosity, since it is the answer to what was asked rather than
    // detail about a list: the record's own line, with the reason on it, is the
    // `-v` half.
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
fn a_skip_naming_a_record_the_filters_left_out_matched_something() {
    // The reason such a record stays in the list: it still answers to its
    // address, so a name that reaches it is answered rather than reported as
    // reaching nothing.
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
fn a_filter_name_the_remote_does_not_declare_warns_and_the_run_carries_on() {
    // The manifest was read, so batfiles can tell. A warning rather than a
    // failure: a remote at an older revision than the leaf expects is a
    // repository to update, not a run to stop.
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
        installed(&tree),
        ["zshrc"],
        "the rest of the filter stopped working:\n{stderr}"
    );
}

#[test]
fn a_group_filter_is_answered_by_a_group_and_not_by_an_action_of_that_name() {
    // Two namespaces, one spelling. `zshrc` is a record's `id` and no group's
    // name, so a group filter naming it has named nothing.
    let (_origin, tree) = filtering("exclude-groups = [\"zshrc\"]\n");

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("exclude-groups `zshrc` matched no group in remote `corporate`"),
        "an action ID satisfied a group filter:\n{stderr}"
    );
    assert_eq!(installed(&tree), ["zshrc", "p10k", "seeds"]);
}

#[test]
fn an_inclusion_may_not_describe_its_selection_twice() {
    // Refused as the manifest is read, before anything is materialized: two
    // selections over one set of records have no reading that is obviously the
    // one that was meant.
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
    // Unqualified: the qualifier is the inclusion's own `id`, so writing it
    // again would name the record twice over.
    let (_origin, tree) = filtering("install-actions = [\"corp.zshrc\"]\n");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("`corp.zshrc`"),
        "the refused value was not quoted back:\n{stderr}"
    );
}
