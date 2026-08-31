//! `apply-action` and `apply-group`: carrying out part of a manifest by name.
//!
//! Both drive the loop `sync` drives, over the same ordered list and the same
//! filter, so what is written here is what naming a target changes: which
//! records are reached, what happens when the name resolves to none, and the
//! one rule that separates these commands from a `sync` — an explicit request
//! waives the exclusions naming what it asked for.
//!
//! Every action here is a `create-dir`, so it runs on every platform and what a
//! run installed is exactly the set of names under the home.

use crate::support::*;

/// Four `create-dir` actions: three in `shell` and one in `gui`, one of the
/// `shell` three carrying no `id` at all.
///
/// The unnamed record is what makes a group more than a shorthand for listing
/// its members: nothing can reach it by name, so an `apply-group` that installs
/// it proves the group is what was resolved.
fn four_actions(tree: &Tree) {
    tree.write_manifest(
        "[[actions]]\n\
         type = \"create-dir\"\n\
         id = \"zshrc\"\n\
         group = \"shell\"\n\
         dest = \"~/zshrc\"\n\
         \n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         id = \"gtkrc\"\n\
         group = \"gui\"\n\
         dest = \"~/gtkrc\"\n\
         \n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         group = \"shell\"\n\
         dest = \"~/zshenv\"\n\
         \n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         id = \"aliases\"\n\
         group = \"shell\"\n\
         dest = \"~/aliases\"\n",
    );
}

/// Run one command over [`four_actions`] and return what it left under the
/// home.
fn applied(tree: &Tree, args: &[&str]) -> Vec<String> {
    tree.batfiles().args(args).assert().success();
    entries(&tree.path("home"))
}

// What a target reaches.

#[test]
fn applying_an_action_carries_out_that_record_and_no_other() {
    let tree = Tree::new();
    four_actions(&tree);
    assert_eq!(
        applied(&tree, &["apply-action", "--id", "zshrc"]),
        ["zshrc"]
    );
}

#[test]
fn applying_a_group_carries_out_every_record_naming_it() {
    // Including the one with no `id`, which nothing else can reach.
    let tree = Tree::new();
    four_actions(&tree);
    assert_eq!(
        applied(&tree, &["apply-group", "--group", "shell"]),
        ["aliases", "zshenv", "zshrc"]
    );
}

#[test]
fn the_two_namespaces_stay_separate() {
    // A group named like an action is reached by neither the other's command.
    let tree = Tree::new();
    tree.write_manifest(
        "[[actions]]\n\
         type = \"create-dir\"\n\
         id = \"shell\"\n\
         group = \"editor\"\n\
         dest = \"~/by-id\"\n\
         \n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         id = \"editor\"\n\
         group = \"shell\"\n\
         dest = \"~/by-group\"\n",
    );
    assert_eq!(
        applied(&tree, &["apply-action", "--id", "shell"]),
        ["by-id"]
    );
}

#[test]
fn a_group_is_applied_in_declaration_order() {
    // The same order `sync` runs, and the records keep the positions the
    // manifest gave them: the heading for the unnamed one says `action 3`,
    // which is where it sits in the whole list rather than within the group.
    let tree = Tree::new();
    four_actions(&tree);
    let assertion = tree
        .batfiles()
        .args(["apply-group", "--group", "shell", "-v"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    let headings: Vec<&str> = stderr
        .lines()
        .filter(|line| line.starts_with("create-dir"))
        .collect();
    assert_eq!(
        headings,
        [
            "create-dir zshrc (group shell)",
            "create-dir action 3 (group shell)",
            "create-dir aliases (group shell)",
        ],
        "unexpected headings in:\n{stderr}"
    );
}

// A name that resolves to nothing.

#[test]
fn applying_an_action_no_record_carries_fails_without_writing() {
    let tree = Tree::new();
    four_actions(&tree);
    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "vimrc"])
        .assert()
        .failure()
        .code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`vimrc`") && stderr.contains("batfiles.toml"),
        "the failure should name the id and the manifest:\n{stderr}"
    );
    assert!(
        entries(&tree.path("home")).is_empty(),
        "nothing should have been installed"
    );
}

#[test]
fn applying_a_group_no_record_names_fails_without_writing() {
    // A group is nothing but the actions naming it, so one nothing names does
    // not exist; there is no separate "empty group" to succeed quietly over.
    let tree = Tree::new();
    four_actions(&tree);
    let assertion = tree
        .batfiles()
        .args(["apply-group", "--group", "fonts"])
        .assert()
        .failure()
        .code(1);
    assert!(
        stderr_of(&assertion).contains("`fonts`"),
        "the failure should name the group:\n{}",
        stderr_of(&assertion)
    );
    assert!(
        entries(&tree.path("home")).is_empty(),
        "nothing should have been installed"
    );
}

#[test]
fn a_name_that_is_not_an_address_fails_before_the_repository_is_opened() {
    // Reported as the malformed name it is rather than as a lookup that missed.
    // The repository is missing altogether, so a run that got as far as opening
    // it would fail with a different message.
    let tree = Tree::roots();
    for args in [
        &["apply-action", "--id", "core..zshrc"][..],
        &["apply-group", "--group", "core..shell"],
    ] {
        let assertion = tree.batfiles().args(args).assert().failure().code(1);
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("is not a valid address"),
            "expected the address rule for `{args:?}`:\n{stderr}"
        );
    }
}

#[test]
fn a_qualified_address_resolves_to_nothing_rather_than_being_refused() {
    // It is well formed, so it reaches the manifest; nothing there answers to
    // it, because only an included remote could contribute an action a dotted
    // name reaches, and none is included. That is the ordinary unresolved
    // failure rather than a complaint about the name.
    let tree = Tree::new();
    four_actions(&tree);
    for (args, expected) in [
        (&["apply-action", "--id", "core.zshrc"][..], "core.zshrc"),
        (&["apply-group", "--group", "core.shell"], "core.shell"),
    ] {
        let assertion = tree.batfiles().args(args).assert().failure().code(1);
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(expected) && stderr.contains("no action in"),
            "expected an unresolved failure for `{args:?}`:\n{stderr}"
        );
    }
    assert!(
        entries(&tree.path("home")).is_empty(),
        "nothing should have been installed"
    );
}

// Waiving what the machine turned off.

#[test]
fn applying_an_action_waives_every_exclusion_naming_it() {
    // Naming one action is as explicit as an invocation gets, so both lists are
    // waived: the action's own name and its group's.
    let tree = Tree::new();
    four_actions(&tree);
    tree.write_disabled("actions = [\"zshrc\"]\ngroups = [\"shell\"]\n");
    assert_eq!(
        applied(&tree, &["apply-action", "--id", "zshrc"]),
        ["zshrc"]
    );
}

#[test]
fn applying_a_group_waives_the_groups_disable_and_not_its_members() {
    // The exclusion naming what was asked for is waived; one naming something
    // more specific than what was asked for still applies.
    let tree = Tree::new();
    four_actions(&tree);
    tree.write_disabled("actions = [\"aliases\"]\ngroups = [\"shell\"]\n");
    assert_eq!(
        applied(&tree, &["apply-group", "--group", "shell"]),
        ["zshenv", "zshrc"]
    );
}

#[test]
fn a_malformed_disabled_document_fails_either_apply() {
    // `apply-action` waives the lists and reads the document anyway, so the two
    // commands fail alike rather than by a rule about which files each opens.
    for args in [
        &["apply-action", "--id", "zshrc"][..],
        &["apply-group", "--group", "shell"],
    ] {
        let tree = Tree::new();
        four_actions(&tree);
        tree.write_disabled("actions = [\"my action\"]\n");

        let assertion = tree.batfiles().args(args).assert().failure().code(1);
        assert!(
            stderr_of(&assertion).contains("`my action`"),
            "`{args:?}` should name the entry it could not read:\n{}",
            stderr_of(&assertion)
        );
        assert!(
            entries(&tree.path("home")).is_empty(),
            "`{args:?}` should have installed nothing"
        );
    }
}

// The run-only skips each command accepts.

#[test]
fn applying_a_group_honors_the_action_skips_in_both_spellings() {
    let tree = Tree::new();
    four_actions(&tree);
    assert_eq!(
        applied(
            &tree,
            &["apply-group", "--group", "shell", "--skip-action", "zshrc"]
        ),
        ["aliases", "zshenv"]
    );

    let tree = Tree::new();
    four_actions(&tree);
    tree.batfiles()
        .args(["apply-group", "--group", "shell"])
        .env("BATFILES_SKIP_ACTIONS", "zshrc,aliases")
        .assert()
        .success();
    assert_eq!(entries(&tree.path("home")), ["zshenv"]);
}

#[test]
fn the_group_skips_reach_neither_apply_command() {
    // Neither accepts `--skip-group`, and the variable that is its other half
    // is ignored for the same reason: each command has already named what it is
    // applying, and a group skip could only contradict that.
    let tree = Tree::new();
    four_actions(&tree);
    tree.batfiles()
        .args(["apply-group", "--group", "shell"])
        .env("BATFILES_SKIP_GROUPS", "shell")
        .assert()
        .success();
    assert_eq!(entries(&tree.path("home")), ["aliases", "zshenv", "zshrc"]);

    let tree = Tree::new();
    four_actions(&tree);
    tree.batfiles()
        .args(["apply-action", "--id", "zshrc"])
        .env("BATFILES_SKIP_ACTIONS", "zshrc")
        .env("BATFILES_SKIP_GROUPS", "shell")
        .assert()
        .success();
    assert_eq!(entries(&tree.path("home")), ["zshrc"]);
}

// What it says when nothing is left to do.

/// Two `create-dir` actions in one group, both named.
///
/// Emptying a group takes a member-by-member exclusion, since the group's own
/// disable is waived and `--skip-group` is not accepted — so every member has
/// to be reachable by name for the group to end up with nothing to do.
/// [`four_actions`]' unnamed record is what makes that impossible there.
fn two_named_actions(tree: &Tree) {
    tree.write_manifest(
        "[[actions]]\n\
         type = \"create-dir\"\n\
         id = \"zshrc\"\n\
         group = \"shell\"\n\
         dest = \"~/zshrc\"\n\
         \n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         id = \"aliases\"\n\
         group = \"shell\"\n\
         dest = \"~/aliases\"\n",
    );
}

#[test]
fn a_group_with_nothing_left_to_apply_says_so_and_succeeds() {
    // Every member being passed over is the command working as asked rather
    // than failing, but at normal verbosity it would otherwise answer a command
    // that named one thing with silence.
    let tree = Tree::new();
    two_named_actions(&tree);
    tree.write_disabled("actions = [\"zshrc\"]\n");

    let assertion = tree
        .batfiles()
        .args([
            "apply-group",
            "--group",
            "shell",
            "--skip-action",
            "aliases",
        ])
        .assert()
        .success();
    assert!(
        stderr_of(&assertion).contains("nothing to apply"),
        "a run that did nothing should say so:\n{}",
        stderr_of(&assertion)
    );
    assert!(
        entries(&tree.path("home")).is_empty(),
        "nothing should have been installed"
    );
}

#[test]
fn the_reason_each_member_was_passed_over_is_verbose_detail() {
    // The line above says that nothing happened; `-v` is still where the
    // account of which record and why lives, exactly as in a `sync`.
    let tree = Tree::new();
    two_named_actions(&tree);
    tree.write_disabled("actions = [\"zshrc\"]\n");

    let assertion = tree
        .batfiles()
        .args([
            "apply-group",
            "--group",
            "shell",
            "--skip-action",
            "aliases",
            "-v",
        ])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    for expected in [
        "create-dir zshrc (group shell) - skipped: action `zshrc` is disabled",
        "create-dir aliases (group shell) - skipped: `aliases` from --skip-action",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn quiet_suppresses_the_line_saying_nothing_happened() {
    // It is ordinary status output: what the command did, not a problem.
    let tree = Tree::new();
    two_named_actions(&tree);
    tree.write_disabled("actions = [\"zshrc\", \"aliases\"]\n");

    tree.batfiles()
        .args(["apply-group", "--group", "shell", "--quiet"])
        .assert()
        .success()
        .stderr("");
}
