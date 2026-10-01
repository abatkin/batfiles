//! CLI tests for bootstrap disabled-state adoption during `clone`, using local bare
//! repositories.

use std::fs;

use assert_cmd::Command;

use crate::support::*;

/// A leaf declaring three actions, one of them in a group, and whatever
/// `[default-disabled]` the case is about.
fn origin_with(candidates: &str) -> BareRepo {
    let origin = BareRepo::new();
    for name in ["p10k", "zshrc", "gvim"] {
        origin.publish(&format!("files/{name}"), name, "a file to install");
    }
    origin.publish(
        "batfiles.toml",
        &format!(
            r#"{candidates}
[[actions]]
type = "copy"
id = "p10k"
source = "files/p10k"
dest = "~/p10k"

[[actions]]
type = "copy"
id = "zshrc"
source = "files/zshrc"
dest = "~/zshrc"

[[actions]]
type = "copy"
id = "gvim"
group = "gui"
source = "files/gvim"
dest = "~/gvim"
"#
        ),
        "a leaf with a bootstrap policy",
    );
    origin
}

/// `clone`, into the repository root the tree selects and leaves vacant.
fn cloning(tree: &Tree, origin: &BareRepo) -> Command {
    let mut command = tree.batfiles();
    command.args(["clone", &display(&origin.origin())]);
    command
}

/// Which of the three the run installed, in manifest order.
fn installed(tree: &Tree) -> Vec<&'static str> {
    ["p10k", "zshrc", "gvim"]
        .into_iter()
        .filter(|name| tree.home(name).is_file())
        .collect()
}

#[test]
fn a_candidate_is_adopted_and_the_synchronization_passes_it_over() {
    let tree = Tree::roots();
    let origin = origin_with(
        "[[default-disabled.actions]]\nid = \"p10k\"\n\n\
         [[default-disabled.groups]]\ngroup = \"gui\"\n",
    );

    let assertion = cloning(&tree, &origin).assert().success();

    assert_eq!(installed(&tree), ["zshrc"]);
    assert_eq!(
        tree.disabled_document(),
        "actions = [\"p10k\"]\ngroups = [\"gui\"]\n"
    );

    let stderr = stderr_of(&assertion);
    for expected in [
        "default-disabled: disabled action `p10k`",
        "default-disabled: disabled group `gui`",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_machine_that_already_has_a_disabled_document_is_not_offered_them() {
    let tree = Tree::roots();
    tree.write_disabled("actions = [\"zshrc\"]\ngroups = []\n");
    let origin = origin_with("[[default-disabled.actions]]\nid = \"p10k\"\n");

    cloning(&tree, &origin).assert().success();

    assert_eq!(installed(&tree), ["p10k", "gvim"]);
    assert_eq!(
        tree.disabled_document(),
        "actions = [\"zshrc\"]\ngroups = []\n"
    );
}

#[test]
fn an_explicit_decision_applies_to_such_a_machine_all_the_same() {
    let tree = Tree::roots();
    tree.write_disabled("actions = []\ngroups = []\n");
    let origin = origin_with("[[default-disabled.actions]]\nid = \"p10k\"\n");

    let assertion = cloning(&tree, &origin)
        .args(["--disable-action", "zshrc"])
        .assert()
        .success();

    assert_eq!(installed(&tree), ["p10k", "gvim"]);
    assert!(
        stderr_of(&assertion).contains("--disable-action: disabled action `zshrc`"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_bootstrap_that_decides_nothing_writes_no_document() {
    let tree = Tree::roots();

    cloning(&tree, &origin_with("")).assert().success();

    assert_eq!(installed(&tree), ["p10k", "zshrc", "gvim"]);
    assert!(
        !tree.disabled().exists(),
        "a bootstrap with nothing to record wrote a document anyway"
    );
}

#[test]
fn the_command_line_outranks_the_environment_and_enable_outranks_disable() {
    let tree = Tree::roots();
    let origin = origin_with("[[default-disabled.actions]]\nid = \"p10k\"\n");

    cloning(&tree, &origin)
        .env("BATFILES_ENABLE_ACTIONS", "p10k")
        .env("BATFILES_DISABLE_ACTIONS", "zshrc")
        .args(["--disable-action", "p10k"])
        .args(["--enable-action", "zshrc"])
        .assert()
        .success();

    assert_eq!(installed(&tree), ["zshrc", "gvim"]);
}

#[test]
fn an_enable_within_one_source_wins_over_its_own_disable() {
    let tree = Tree::roots();

    cloning(&tree, &origin_with(""))
        .env("BATFILES_DISABLE_GROUPS", "gui")
        .env("BATFILES_ENABLE_GROUPS", "gui")
        .assert()
        .success();

    assert_eq!(installed(&tree), ["p10k", "zshrc", "gvim"]);
}

#[test]
fn a_candidate_is_offered_only_where_its_condition_admits_it() {
    let origin = origin_with(
        "[[default-disabled.actions]]\nid = \"p10k\"\nwhen = \"slow\"\n\n\
         [[default-disabled.actions]]\nid = \"zshrc\"\nunless = \"slow\"\n",
    );

    let slow = Tree::roots();
    cloning(&slow, &origin)
        .args(["--var", "slow=true"])
        .assert()
        .success();
    assert_eq!(installed(&slow), ["zshrc", "gvim"]);

    let quick = Tree::roots();
    cloning(&quick, &origin)
        .args(["--var", "slow=false"])
        .assert()
        .success();
    assert_eq!(installed(&quick), ["p10k", "gvim"]);
}

#[test]
fn a_condition_this_machine_cannot_decide_leaves_the_candidate_alone() {
    let tree = Tree::roots();
    let origin = origin_with("[[default-disabled.actions]]\nid = \"p10k\"\nwhen = \"nowhere\"\n");

    let assertion = cloning(&tree, &origin).assert().success();

    assert_eq!(installed(&tree), ["p10k", "zshrc", "gvim"]);
    assert!(!tree.disabled().exists(), "the candidate was adopted");

    let stderr = stderr_of(&assertion);
    for expected in ["candidate action `p10k`", "it is left enabled", "nowhere"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_candidate_a_condition_closes_is_reported_at_one_level_of_detail() {
    let tree = Tree::roots();
    let origin = origin_with("[[default-disabled.actions]]\nid = \"p10k\"\nwhen = \"slow\"\n");

    let quiet = cloning(&tree, &origin)
        .args(["--var", "slow=false"])
        .assert()
        .success();
    assert!(
        !stderr_of(&quiet).contains("candidate action"),
        "{}",
        stderr_of(&quiet)
    );

    let verbose = Tree::roots();
    let assertion = cloning(&verbose, &origin)
        .args(["--var", "slow=false", "-v"])
        .assert()
        .success();
    assert!(
        stderr_of(&assertion).contains("candidate action `p10k` - skipped: when \"slow\" is false"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn an_unusable_option_value_fails_before_anything_is_cloned() {
    let tree = Tree::roots();
    let origin = origin_with("");

    let assertion = cloning(&tree, &origin)
        .args(["--disable-action", "my action"])
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`my action`"),
        "unexpected stderr:\n{stderr}"
    );
    assert!(
        !tree.path("repo").exists(),
        "the command cloned something before checking what it was asked for"
    );
}

#[test]
fn an_unusable_variable_name_warns_and_the_rest_of_the_list_still_applies() {
    let tree = Tree::roots();

    let assertion = cloning(&tree, &origin_with(""))
        .env("BATFILES_DISABLE_ACTIONS", "p10k,my action,zshrc")
        .assert()
        .success();

    assert_eq!(installed(&tree), ["gvim"]);
    let stderr = stderr_of(&assertion);
    for expected in ["BATFILES_DISABLE_ACTIONS", "`my action`"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn an_included_remotes_own_candidates_are_ignored() {
    let tree = Tree::roots();
    let core = BareRepo::new();
    core.publish("files/corerc", "# core\n", "the file it installs");
    core.publish(
        "batfiles.toml",
        r#"[[default-disabled.actions]]
id = "corerc"

[[actions]]
type = "copy"
id = "corerc"
source = "files/corerc"
dest = "~/.corerc"
"#,
        "a remote with a policy of its own",
    );

    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        &format!(
            r#"[remotes.core]
type = "git"
url = "{}"

[[actions]]
type = "include-remote"
id = "core"
remote = "core"
"#,
            display(&core.origin())
        ),
        "a leaf composing it",
    );

    cloning(&tree, &origin).assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".corerc")).expect("the included action's file"),
        "# core\n"
    );
    assert!(
        !tree.disabled().exists(),
        "the remote's own candidates were adopted"
    );
}

#[test]
fn sync_is_not_a_bootstrap_and_adopts_nothing() {
    let tree = Tree::roots();
    let origin = origin_with("[[default-disabled.actions]]\nid = \"p10k\"\n");

    cloning(&tree, &origin).assert().success();
    fs::remove_file(tree.disabled()).expect("the adopted document");
    fs::remove_file(tree.home("zshrc")).expect("an installed file");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(installed(&tree), ["p10k", "zshrc", "gvim"]);
    assert!(
        !tree.disabled().exists(),
        "`sync` adopted the candidates the bootstrap had already offered"
    );
}

/// A checkout that arrived without `clone`, as `git clone` leaves one: the repository is in
/// place, and this machine has no disabled state and nothing installed.
fn checkout(tree: &Tree, origin: &BareRepo) {
    cloning(tree, origin).assert().success();
    if tree.disabled().exists() {
        fs::remove_file(tree.disabled()).expect("the adopted document");
    }
    for name in installed(tree) {
        fs::remove_file(tree.home(name)).expect("an installed file");
    }
}

#[test]
fn sync_bootstrap_adopts_as_clone_does() {
    let tree = Tree::roots();
    let origin = origin_with("[[default-disabled.actions]]\nid = \"p10k\"\n");
    checkout(&tree, &origin);

    let assertion = tree
        .batfiles()
        .args(["sync", "--bootstrap", "--disable-group", "gui"])
        .assert()
        .success();

    assert_eq!(installed(&tree), ["zshrc"]);
    assert_eq!(
        tree.disabled_document(),
        "actions = [\"p10k\"]\ngroups = [\"gui\"]\n"
    );
    let stderr = stderr_of(&assertion);
    for expected in [
        "default-disabled: disabled action `p10k`",
        "--disable-group: disabled group `gui`",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }

    // Offered once: the machine now has a document, so a second bootstrap adds nothing.
    tree.write_disabled("actions = []\ngroups = []\n");
    tree.batfiles()
        .args(["sync", "--bootstrap"])
        .assert()
        .success();
    assert_eq!(installed(&tree), ["p10k", "zshrc", "gvim"]);
}

#[test]
fn a_dry_run_bootstrap_plans_with_the_policy_and_writes_nothing() {
    let tree = Tree::roots();
    let origin = origin_with("[[default-disabled.actions]]\nid = \"p10k\"\n");
    checkout(&tree, &origin);

    let assertion = tree
        .batfiles()
        .args(["sync", "--bootstrap", "--dry-run"])
        .assert()
        .success();

    assert!(installed(&tree).is_empty());
    assert!(!tree.disabled().exists(), "a dry run wrote disabled.toml");
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("default-disabled: would disable action `p10k`"),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!("would copy {}", display(&tree.home("zshrc")))),
        "{stderr}"
    );
    assert!(
        !stderr.contains(&format!("would copy {}", display(&tree.home("p10k")))),
        "the dry run planned an action the bootstrap disables:\n{stderr}"
    );
}

#[test]
fn bootstrap_options_on_sync_need_bootstrap() {
    let tree = Tree::roots();
    let origin = origin_with("");
    checkout(&tree, &origin);

    let assertion = tree
        .batfiles()
        .args(["sync", "--disable-group", "gui"])
        .assert()
        .code(2);
    assert!(stderr_of(&assertion).contains("--bootstrap"));
    assert!(installed(&tree).is_empty());
}
