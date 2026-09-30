//! CLI tests for named action and group application, missing targets, and exclusion exemptions.
//! Fixtures use `create-dir` actions.

use crate::support::*;

/// Declare three `shell` actions (one unnamed) and one `gui` action.
fn four_actions(tree: &Tree) {
    tree.write_manifest(
        r#"[[actions]]
type = "create-dir"
id = "zshrc"
group = "shell"
dest = "~/zshrc"

[[actions]]
type = "create-dir"
id = "gtkrc"
group = "gui"
dest = "~/gtkrc"

[[actions]]
type = "create-dir"
group = "shell"
dest = "~/zshenv"

[[actions]]
type = "create-dir"
id = "aliases"
group = "shell"
dest = "~/aliases"
"#,
    );
}

/// Run one command over [`four_actions`] and return what it left under the
/// home.
fn applied(tree: &Tree, args: &[&str]) -> Vec<String> {
    tree.batfiles().args(args).assert().success();
    entries(&tree.path("home"))
}

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
    let tree = Tree::new();
    four_actions(&tree);
    assert_eq!(
        applied(&tree, &["apply-group", "--group", "shell"]),
        ["aliases", "zshenv", "zshrc"]
    );
}

#[test]
fn the_two_namespaces_stay_separate() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "create-dir"
id = "shell"
group = "editor"
dest = "~/by-id"

[[actions]]
type = "create-dir"
id = "editor"
group = "shell"
dest = "~/by-group"
"#,
    );
    assert_eq!(
        applied(&tree, &["apply-action", "--id", "shell"]),
        ["by-id"]
    );
}

#[test]
fn a_group_is_applied_in_declaration_order() {
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
    // Leave the repository absent so a file-read error cannot masquerade as address validation.
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

#[test]
fn applying_an_action_waives_every_exclusion_naming_it() {
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

/// Declare two named `create-dir` actions in one group.
fn two_named_actions(tree: &Tree) {
    tree.write_manifest(
        r#"[[actions]]
type = "create-dir"
id = "zshrc"
group = "shell"
dest = "~/zshrc"

[[actions]]
type = "create-dir"
id = "aliases"
group = "shell"
dest = "~/aliases"
"#,
    );
}

#[test]
fn a_group_with_nothing_left_to_apply_says_so_and_succeeds() {
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
    let tree = Tree::new();
    two_named_actions(&tree);
    tree.write_disabled("actions = [\"zshrc\", \"aliases\"]\n");

    tree.batfiles()
        .args(["apply-group", "--group", "shell", "--quiet"])
        .assert()
        .success()
        .stderr("");
}
