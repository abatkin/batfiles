//! `include-remote`: the actions another repository declares, taken into this
//! one at the position the inclusion is written.
//!
//! Three questions separate the cases. Which tree the manifest came from is what
//! makes a description knowingly stale; whether there was one at all is what
//! makes a plan partial; and what a contributed record is *called* is what makes
//! it reachable, since an included action answers to its qualified name and to
//! no unqualified one.
//!
//! The leaf is the `inclusion` fixture and the remote is the `corporate` one,
//! committed into a local bare repository as the remotes tests commit it
//! (`guidance.md`, "Test environments").

use std::fs;

use crate::support::*;

/// The leaf that includes `corporate`, pointed at a bare repository holding it.
fn including() -> (BareRepo, Tree) {
    let origin = BareRepo::from_fixture("corporate");
    let tree = Tree::fixture("inclusion");
    tree.point_at_origin(&origin);
    (origin, tree)
}

/// Where the included manifest lands once the remote is materialized.
fn included_manifest(tree: &Tree) -> String {
    display(&tree.path("repo").join("remotes/corporate/batfiles.toml"))
}

/// The three actions the `corporate` fixture declares, as a report names them:
/// under the `corp` inclusion that contributed them, in declaration order.
const INCLUDED_ACTIONS: [&str; 3] = [
    "symlink corp.zshrc (group corp.shell)",
    "symlink corp.p10k (group corp.prompt)",
    "copy-dir corp.seeds",
];

/// The destinations those three install at.
const INCLUDED_DESTS: [&str; 3] = [".zshrc.corporate", ".p10k.zsh", ".config/corporate"];

fn assert_included_ran(tree: &Tree) {
    for dest in INCLUDED_DESTS {
        assert!(
            tree.home(dest).exists(),
            "`{dest}` was not installed by the action the inclusion contributed"
        );
    }
}

fn assert_nothing_included_ran(tree: &Tree) {
    for dest in INCLUDED_DESTS {
        assert!(
            !tree.home(dest).exists(),
            "`{dest}` was installed by an inclusion this run should not have carried out"
        );
    }
}

#[test]
fn a_materialized_inclusion_contributes_its_actions_to_the_run() {
    let (_origin, tree) = including();

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("include-remote corp (group work)"),
        "the inclusion did not report at its position:\n{stderr}"
    );
    for action in INCLUDED_ACTIONS {
        assert!(
            stderr.contains(action),
            "`{action}` did not report under the address that reaches it:\n{stderr}"
        );
    }
    assert_included_ran(&tree);

    // The source of a contributed action is read from the remote's own tree,
    // which is the whole of what the inclusion changes about resolving one.
    assert_eq!(
        fs::read_link(tree.home(".zshrc.corporate")).expect("the link the inclusion installed"),
        tree.path("repo").join("remotes/corporate/files/zshrc"),
        "the included source was not read from the materialization"
    );

    // The leaf's own actions are untouched by any of it.
    assert!(
        tree.home(".cache/zsh").is_dir(),
        "the leaf's own action did not run"
    );
}

#[test]
fn an_inclusion_contributes_at_its_position_in_the_list() {
    // Declaration order is the whole point of the record: what it brings in
    // belongs where it is written, not before or after the list.
    let (_origin, tree) = including();

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    let at = |heading: &str| {
        stderr
            .find(heading)
            .unwrap_or_else(|| panic!("`{heading}` was not reported:\n{stderr}"))
    };
    let leaf = at("create-dir zsh-cache");
    let inclusion = at("include-remote corp");
    assert!(
        leaf < inclusion && inclusion < at(INCLUDED_ACTIONS[0]),
        "the inclusion's contents were not spliced where it is written:\n{stderr}"
    );
    assert!(
        at(INCLUDED_ACTIONS[0]) < at(INCLUDED_ACTIONS[1])
            && at(INCLUDED_ACTIONS[1]) < at(INCLUDED_ACTIONS[2]),
        "the contributed actions lost the order the included manifest declares:\n{stderr}"
    );
}

#[test]
fn a_dry_run_describes_the_materialization_it_finds_however_stale() {
    // The cost of reading a tree rather than fetching one, stated as a
    // difference this test can see: the remote publishes a third action, and the
    // dry run still describes the two the last `sync` left behind.
    let (origin, tree) = including();
    tree.batfiles().arg("sync").assert().success();

    origin.publish(
        "batfiles.toml",
        "[[actions]]\ntype = \"create-dir\"\nid = \"a\"\ndest = \"~/.a\"\n\
         [[actions]]\ntype = \"create-dir\"\nid = \"b\"\ndest = \"~/.b\"\n\
         [[actions]]\ntype = \"create-dir\"\nid = \"c\"\ndest = \"~/.c\"\n",
        "three actions",
    );

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run", "-v"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("symlink corp.zshrc"),
        "the dry run did not read the tree on the machine:\n{stderr}"
    );
    assert!(
        !stderr.contains("create-dir corp.a"),
        "the dry run fetched the remote after all:\n{stderr}"
    );
    // Knowingly stale is still complete: the tree was there and was read, so
    // there is nothing for the run to warn about.
    assert!(
        !stderr.contains("warning"),
        "a plan read from a stale tree is still a whole one:\n{stderr}"
    );
}

#[test]
fn an_inclusion_with_no_materialization_warns_rather_than_failing() {
    // The one case where an absent tree is not a refusal. A leaf action reaching
    // one is refused, because the rest of the plan can do without that action's
    // content; a list of actions is not something the plan can do without. The
    // warning is what marks the plan partial, and it is the whole of the mark.
    let (_origin, tree) = including();

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("warning: remote `corporate` is not materialized"),
        "the reason was not reported:\n{stderr}"
    );
    // The remedy, which is the half of the line a reader acts on.
    assert!(
        stderr.contains("batfiles sync"),
        "the warning did not say how to see the rest:\n{stderr}"
    );
    // A dry run materializes nothing, including for its own benefit.
    assert!(
        !tree.path("repo").join("remotes").exists(),
        "the dry run created a remotes tree"
    );
    // Partial is a caveat on the description, not a failure: the run still
    // reports the rest of the plan, and exits 0 above.
    assert!(
        stderr.contains("would create"),
        "the leaf's own action was not described:\n{stderr}"
    );
}

#[test]
fn an_apply_command_with_no_materialization_says_the_same_thing() {
    // The apply commands materialize nothing either, so they are in the dry
    // run's position without being one, and they report it the same way.
    let (_origin, tree) = including();

    let assertion = tree
        .batfiles()
        .args(["apply-group", "--group", "work"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("warning: remote `corporate` is not materialized"),
        "the reason was not reported:\n{stderr}"
    );
    assert!(
        !tree.path("repo").join("remotes").exists(),
        "an apply command materialized a remote"
    );
}

#[test]
fn an_inclusion_the_run_never_reaches_is_never_read() {
    // A record the command did not ask for is not one the run has an opinion
    // about: applying the leaf's own action leaves the inclusion unread, so an
    // absent materialization goes unmentioned rather than warned about.
    let (_origin, tree) = including();

    let assertion = tree
        .batfiles()
        .args(["apply-action", "-v", "--id", "zsh-cache"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        !stderr.contains("is not materialized"),
        "a record the command did not ask for was read:\n{stderr}"
    );
}

#[test]
fn a_remote_this_machine_excludes_includes_nothing_and_leaves_the_plan_whole() {
    // The manifest declining the inclusion, which is not the same as the run
    // being unable to describe it: nothing was left unanswered.
    let origin = BareRepo::from_fixture("corporate");
    let tree = Tree::fixture("inclusion");
    tree.point_at_origin(&origin);
    let manifest = fs::read_to_string(tree.manifest()).expect("the fixture manifest");
    tree.write_manifest(&manifest.replace(
        "[remotes.corporate]\ntype = \"git\"",
        "[remotes.corporate]\nwhen = \"work\"\ntype = \"git\"",
    ));

    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--var", "work=false"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    // Reported as an exclusion on the record, in the form every other excluded
    // record uses, and saying which of the two conditions closed it.
    assert!(
        stderr.contains(
            "include-remote corp (group work) - skipped: \
             remote `corporate` is excluded here: when \"work\" is false"
        ),
        "the inclusion did not say why it brought nothing in:\n{stderr}"
    );
    // A plan is partial where something warned that it could not be listed, and
    // an exclusion is not that: the manifest was read and answered.
    assert!(
        !stderr.contains("warning"),
        "an ordinary exclusion was reported as an incomplete plan:\n{stderr}"
    );
    assert_nothing_included_ran(&tree);
}

#[test]
fn a_materialization_with_no_manifest_is_refused_by_name() {
    // A remote's manifest is optional, because a remote an action only installs
    // *from* has no use for one. An inclusion is what asks for one, so this is
    // the leaf asking for something that is not there rather than a tree
    // batfiles has yet to fetch.
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[remotes.core]\ntype = \"git\"\nurl = \"{}\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nremote = \"core\"\n",
        display(&origin.origin())
    ));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("remote `core` has no batfiles.toml"),
        "the refusal did not name the remote:\n{stderr}"
    );
    assert!(
        stderr.contains(&display(
            &tree.path("repo").join("remotes/core/batfiles.toml")
        )),
        "the refusal did not say where it looked:\n{stderr}"
    );
}

#[test]
fn an_unreadable_inclusion_fails_before_the_first_action_runs() {
    // What expanding the list ahead of the run buys: a leaf whose inclusion
    // cannot be read installs nothing at all, rather than half a home and then a
    // failure. The same rule a malformed clone list follows.
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[remotes.core]\ntype = \"git\"\nurl = \"{}\"\n\n\
         [[actions]]\ntype = \"create-dir\"\nid = \"first\"\ndest = \"~/.first\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"core\"\nremote = \"core\"\n",
        display(&origin.origin())
    ));

    tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        !tree.home(".first").exists(),
        "the leaf's own action ran before the inclusion was found to be unreadable"
    );
}

#[test]
fn an_included_clone_list_is_read_from_the_materialization() {
    // The one source a handler does not resolve: a list is read while the run's
    // lists are prepared, so an included one is where the tree a record came
    // from has to reach preparation rather than execution.
    let origin = BareRepo::from_fixture("corporate");
    let plugin = origin.another("zsh-z");
    origin.publish(
        "plugins.txt",
        &format!("{}\n", display(&plugin)),
        "a plugin list",
    );
    origin.publish(
        "batfiles.toml",
        "[[actions]]\ntype = \"git-clone-list\"\nid = \"plugins\"\n\
         source = \"plugins.txt\"\ndest-dir = \"~/.plugins\"\n",
        "clone what the list names",
    );
    let tree = Tree::fixture("inclusion");
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        tree.home(".plugins/zsh-z").is_dir(),
        "the list the remote holds was not read and cloned from:\n{stderr}"
    );
    // The list is named as the manifest that declared it wrote it; which tree
    // that is, the heading above it says.
    assert!(
        stderr.contains("git-clone-list corp.plugins"),
        "the contributed list did not report under its address:\n{stderr}"
    );
}

#[test]
fn an_included_action_may_not_source_from_a_remote() {
    // The rule that keeps inclusion one level deep, enforced as the included
    // manifest is read: remote references belong to the leaf repository.
    let origin = BareRepo::from_fixture("corporate");
    origin.publish(
        "batfiles.toml",
        "[remotes.shared]\ntype = \"git\"\nurl = \"https://e.example/shared.git\"\n\n\
         [[actions]]\ntype = \"symlink\"\nsource = \"@shared/vimrc\"\ndest = \"~/.vimrc\"\n",
        "reach a second repository",
    );
    let tree = Tree::fixture("inclusion");
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);

    // Refused for naming a remote at all, rather than for naming one the
    // included manifest happens not to declare -- it declares this one.
    assert!(
        stderr.contains("names a remote"),
        "the rule that was broken was not stated:\n{stderr}"
    );
    assert!(
        stderr.contains("leaf repository"),
        "the diagnostic did not say whose remotes those are:\n{stderr}"
    );
    assert!(
        stderr.contains(&included_manifest(&tree)),
        "the diagnostic did not name the document:\n{stderr}"
    );
}

#[test]
fn an_included_inclusion_is_dropped_rather_than_followed() {
    // The other half of the one-level rule, and the half that is about what an
    // included manifest may hold rather than what one of its actions may write.
    // Dropped rather than refused: the manifest breaking it belongs to someone
    // else, and the rest of what it declares is still good.
    let origin = BareRepo::from_fixture("corporate");
    origin.publish(
        "batfiles.toml",
        "[remotes.shared]\ntype = \"git\"\nurl = \"https://e.example/shared.git\"\n\n\
         [[actions]]\ntype = \"create-dir\"\nid = \"cache\"\ndest = \"~/.cache/corp\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"shared\"\nremote = \"shared\"\n",
        "include a second repository",
    );
    let tree = Tree::fixture("inclusion");
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("not included: include-remote corp.shared"),
        "the nested inclusion was not reported as left out:\n{stderr}"
    );
    assert!(
        stderr.contains("does not reach further repositories"),
        "the rule behind it was not stated:\n{stderr}"
    );
    // Only the nested record is dropped; the one beside it is contributed.
    assert!(
        tree.home(".cache/corp").is_dir(),
        "the included action beside the nested inclusion did not run"
    );
    // Nothing went looking for the remote the nested record names.
    assert!(
        !tree.path("repo").join("remotes/shared").exists(),
        "a nested inclusion's remote was materialized"
    );
}

#[test]
fn dropping_a_nested_inclusion_does_not_renumber_what_follows_it() {
    // A record with no `id` is named by the position it was written at, so the
    // gap a dropped record leaves has to stay a gap. Both records below would
    // otherwise be `action 1`, and a reader told to go and look at action 1
    // would find the wrong one.
    let origin = BareRepo::from_fixture("corporate");
    origin.publish(
        "batfiles.toml",
        "[remotes.shared]\ntype = \"git\"\nurl = \"https://e.example/shared.git\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"shared\"\nremote = \"shared\"\n\n\
         [[actions]]\ntype = \"create-dir\"\ndest = \"~/.cache/corp\"\n",
        "a nested inclusion ahead of an unnamed action",
    );
    let tree = Tree::fixture("inclusion");
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("not included: include-remote corp.shared"),
        "the nested inclusion was not reported as left out:\n{stderr}"
    );
    assert!(
        stderr.contains("create-dir action 2"),
        "the record after the dropped one was renumbered:\n{stderr}"
    );
    assert!(
        !stderr.contains("create-dir action 1"),
        "a contributed record was named by where it landed rather than where it was written:\n{stderr}"
    );
}

#[test]
fn an_inclusion_cannot_be_applied_on_its_own() {
    // The address reaches a position in the list rather than something to carry
    // out, so it named the wrong thing rather than nothing.
    let (_origin, tree) = including();
    tree.batfiles().arg("sync").assert().success();

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "corp"])
        .assert()
        .failure()
        .code(1);
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("cannot be applied on its own"),
        "the refusal did not say why:\n{stderr}"
    );
    assert!(
        !stderr.contains("no action in"),
        "an inclusion was reported as an unknown action:\n{stderr}"
    );
}

#[test]
fn an_inclusion_naming_an_undeclared_remote_is_refused_as_the_manifest_is_read() {
    let stderr = rejected("[[actions]]\ntype = \"include-remote\"\nremote = \"core\"\n");

    assert!(stderr.contains("action 1"), "{stderr}");
    assert!(stderr.contains("`core` is not declared"), "{stderr}");
    assert!(stderr.contains("[remotes.core]"), "{stderr}");
}
