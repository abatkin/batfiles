//! CLI tests for persisted disabled state and run-only skips. Unmatched run-only skips warn;
//! unmatched persisted entries remain silent.

use crate::support::*;

/// Declare three named directory actions: two in one group and one in another.
fn three_actions(tree: &Tree) {
    tree.write_manifest(
        r#"[[actions]]
type = "create-dir"
id = "zshrc"
group = "shell"
dest = "~/zshrc"

[[actions]]
type = "create-dir"
id = "zshenv"
group = "shell"
dest = "~/zshenv"

[[actions]]
type = "create-dir"
id = "gtkrc"
group = "gui"
dest = "~/gtkrc"
"#,
    );
}

/// Run `sync` with the given arguments over [`three_actions`], and return what
/// it left under the home.
fn installed(tree: &Tree, args: &[&str]) -> Vec<String> {
    tree.batfiles().arg("sync").args(args).assert().success();
    entries(&tree.path("home"))
}

// The two run-only sources.

#[test]
fn skipping_an_action_leaves_the_rest_of_the_run_alone() {
    let tree = Tree::new();
    three_actions(&tree);
    assert_eq!(
        installed(&tree, &["--skip-action", "zshrc"]),
        ["gtkrc", "zshenv"]
    );
}

#[test]
fn skipping_a_group_skips_every_action_in_it() {
    let tree = Tree::new();
    three_actions(&tree);
    assert_eq!(installed(&tree, &["--skip-group", "shell"]), ["gtkrc"]);
}

#[test]
fn a_skip_option_is_repeatable() {
    let tree = Tree::new();
    three_actions(&tree);
    assert_eq!(
        installed(&tree, &["--skip-action", "zshrc", "--skip-action", "gtkrc"]),
        ["zshenv"]
    );
}

#[test]
fn the_environment_skips_the_same_way_the_options_do() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.batfiles()
        .arg("sync")
        .env("BATFILES_SKIP_ACTIONS", "zshrc")
        .env("BATFILES_SKIP_GROUPS", "gui")
        .assert()
        .success();
    assert_eq!(entries(&tree.path("home")), ["zshenv"]);
}

#[test]
fn an_environment_list_is_split_on_commas_and_trimmed() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.batfiles()
        .arg("sync")
        .env("BATFILES_SKIP_ACTIONS", " zshrc , ,gtkrc ")
        .assert()
        .success();
    assert_eq!(entries(&tree.path("home")), ["zshenv"]);
}

#[test]
fn the_option_and_the_variable_union_rather_than_one_replacing_the_other() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.batfiles()
        .args(["sync", "--skip-action", "zshrc"])
        .env("BATFILES_SKIP_ACTIONS", "gtkrc")
        .assert()
        .success();
    assert_eq!(entries(&tree.path("home")), ["zshenv"]);
}

// The persistent lists.

#[test]
fn a_disabled_action_is_not_installed() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.batfiles()
        .args(["disable-action", "zshrc"])
        .assert()
        .success();
    assert_eq!(installed(&tree, &[]), ["gtkrc", "zshenv"]);
}

#[test]
fn a_disabled_group_is_not_installed() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.batfiles()
        .args(["disable-group", "shell"])
        .assert()
        .success();
    assert_eq!(installed(&tree, &[]), ["gtkrc"]);
}

#[test]
fn enabling_an_action_again_puts_it_back_in_the_run() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.batfiles()
        .args(["disable-group", "shell"])
        .assert()
        .success();
    assert_eq!(installed(&tree, &[]), ["gtkrc"]);

    tree.batfiles()
        .args(["enable-group", "shell"])
        .assert()
        .success();
    assert_eq!(installed(&tree, &[]), ["gtkrc", "zshenv", "zshrc"]);
}

#[test]
fn a_hand_written_disabled_document_is_honored() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.write_disabled("actions = [\"gtkrc\"]\ngroups = [\"shell\"]\n");
    assert!(installed(&tree, &[]).is_empty());
}

#[test]
fn a_malformed_disabled_document_fails_the_run() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.write_disabled("actions = [\"my action\"]\n");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains("`my action`"),
        "the run should name the entry it could not read:\n{}",
        stderr_of(&assertion)
    );
    assert!(
        entries(&tree.path("home")).is_empty(),
        "nothing should have been installed"
    );
}

#[test]
fn a_skipped_action_is_reported_at_verbose_only() {
    let tree = Tree::new();
    three_actions(&tree);

    let quiet = tree
        .batfiles()
        .args(["sync", "--skip-action", "zshrc"])
        .assert()
        .success();
    assert!(
        !stderr_of(&quiet).contains("skipped"),
        "an ordinary run should not mention the skip it was asked for:\n{}",
        stderr_of(&quiet)
    );

    let tree = Tree::new();
    three_actions(&tree);
    let verbose = tree
        .batfiles()
        .args(["sync", "-v", "--skip-action", "zshrc"])
        .assert()
        .success();
    assert!(
        stderr_of(&verbose)
            .contains("create-dir zshrc (group shell) - skipped: `zshrc` from --skip-action"),
        "the heading should say why the record did not act:\n{}",
        stderr_of(&verbose)
    );
}

#[test]
fn a_skip_line_names_the_source_that_supplied_the_name() {
    let tree = Tree::new();
    three_actions(&tree);
    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--skip-group", "gui"])
        .env("BATFILES_SKIP_ACTIONS", "zshrc")
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    for expected in [
        "create-dir zshrc (group shell) - skipped: `zshrc` from BATFILES_SKIP_ACTIONS",
        "create-dir gtkrc (group gui) - skipped: `gui` from --skip-group",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_disable_is_reported_ahead_of_a_skip_that_names_the_same_action() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.batfiles()
        .args(["disable-group", "shell"])
        .assert()
        .success();

    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--skip-action", "zshrc"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("create-dir zshrc (group shell) - skipped: group `shell` is disabled"),
        "no disabled reason in:\n{stderr}"
    );
    assert!(
        !stderr.contains("from --skip-action"),
        "only one reason should be reported:\n{stderr}"
    );
}

#[test]
fn a_skip_matching_nothing_warns_and_the_run_continues() {
    let tree = Tree::new();
    three_actions(&tree);

    let assertion = tree
        .batfiles()
        .args(["sync", "--skip-action", "vimrc", "--skip-group", "editor"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    for expected in [
        "--skip-action `vimrc` matched no action",
        "--skip-group `editor` matched no group",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(entries(&tree.path("home")), ["gtkrc", "zshenv", "zshrc"]);
}

#[test]
fn a_skip_naming_the_other_namespace_matches_nothing() {
    let tree = Tree::new();
    three_actions(&tree);
    let assertion = tree
        .batfiles()
        .args(["sync", "--skip-group", "zshrc"])
        .assert()
        .success();
    assert!(
        stderr_of(&assertion).contains("--skip-group `zshrc` matched no group"),
        "{}",
        stderr_of(&assertion)
    );
    assert_eq!(entries(&tree.path("home")), ["gtkrc", "zshenv", "zshrc"]);
}

#[test]
fn a_skip_that_is_not_an_address_warns_and_the_run_continues() {
    let tree = Tree::new();
    three_actions(&tree);

    let assertion = tree
        .batfiles()
        .args(["sync", "--skip-action", "core..zshrc"])
        .env("BATFILES_SKIP_GROUPS", "my group")
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    for expected in [
        "--skip-action: `core..zshrc` is not a valid address",
        "BATFILES_SKIP_GROUPS: `my group` is not a valid address",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(entries(&tree.path("home")), ["gtkrc", "zshenv", "zshrc"]);
}

#[test]
fn a_qualified_skip_is_a_name_that_matched_nothing() {
    let tree = Tree::new();
    three_actions(&tree);

    let assertion = tree
        .batfiles()
        .args(["sync", "--skip-action", "core.zshrc"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("--skip-action `core.zshrc` matched no action"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("valid address"),
        "a well-formed address should not be reported as malformed:\n{stderr}"
    );
    assert_eq!(entries(&tree.path("home")), ["gtkrc", "zshenv", "zshrc"]);
}

#[test]
fn a_disabled_entry_matching_nothing_is_silent() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.batfiles()
        .args(["disable-action", "vimrc"])
        .assert()
        .success();
    tree.batfiles()
        .args(["disable-group", "editor"])
        .assert()
        .success();

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);
    assert!(
        !stderr.contains("matched no"),
        "a pre-registered name should say nothing:\n{stderr}"
    );
    assert_eq!(entries(&tree.path("home")), ["gtkrc", "zshenv", "zshrc"]);
}

#[test]
fn an_unmatched_skip_is_reported_before_the_run_rather_than_after_it() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "copy"
id = "absent"
source = "nowhere"
dest = "~/absent"
"#,
    );
    let assertion = tree
        .batfiles()
        .args(["sync", "--skip-action", "vimrc"])
        .assert()
        .failure()
        .code(1);
    assert!(
        stderr_of(&assertion).contains("--skip-action `vimrc` matched no action"),
        "{}",
        stderr_of(&assertion)
    );
}

// Dry-run.

#[test]
fn a_dry_run_reports_the_same_skips_and_installs_nothing() {
    let tree = Tree::new();
    three_actions(&tree);
    tree.batfiles()
        .args(["disable-group", "shell"])
        .assert()
        .success();

    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--dry-run", "--skip-action", "gtkrc"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    for expected in [
        "create-dir zshrc (group shell) - skipped: group `shell` is disabled",
        "create-dir gtkrc (group gui) - skipped: `gtkrc` from --skip-action",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert!(
        entries(&tree.path("home")).is_empty(),
        "a dry run should have installed nothing"
    );
}
