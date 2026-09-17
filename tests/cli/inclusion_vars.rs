//! `vars` on an `include-remote`: the values the leaf hands to what it
//! contributes.
//!
//! The overrides reach one inclusion's records and nothing else. They sit above
//! the leaf's `[vars]` and below everything this machine says, which is the
//! [variable precedence](../../docs/environment.md#variable-precedence) read from
//! both ends: a leaf may tell a remote what it is being included for, and the
//! machine may still disagree with the leaf.
//!
//! The remote is written inline rather than committed as a fixture, because what
//! a case needs from it is a condition to decide rather than content to install:
//! the synthetic repository with overlapping paths and overrides arrives at 7.8.

use crate::support::*;

/// The remote both destinations below come from: two records decided by
/// `profile`, exactly one of which runs whatever it is.
fn remote() -> BareRepo {
    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        "[[actions]]\ntype = \"create-dir\"\nid = \"work-tools\"\n\
         dest = \"~/.cache/work-tools\"\nwhen = \"profile == 'work'\"\n\n\
         [[actions]]\ntype = \"create-dir\"\nid = \"personal-tools\"\n\
         dest = \"~/.cache/personal-tools\"\nunless = \"profile == 'work'\"\n",
        "two records one variable decides",
    );
    origin
}

/// A leaf declaring `profile = "personal"` and including that remote, with
/// whatever the case writes on the inclusion.
fn including(record: &str) -> (BareRepo, Tree) {
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[remotes.corporate]\ntype = \"git\"\nurl = \"{{origin}}\"\n\n\
         [vars]\nprofile = \"personal\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"corp\"\nremote = \"corporate\"\n{record}"
    ));
    tree.point_at_origin(&origin);
    (origin, tree)
}

/// What the contributed records' conditions read, as the destinations say it.
/// Exactly one of the two runs, so disagreement is a broken case rather than an
/// answer.
fn included_profile(tree: &Tree) -> &'static str {
    let ran = |dest: &str| tree.home(dest).exists();
    match (ran(".cache/work-tools"), ran(".cache/personal-tools")) {
        (true, false) => "work",
        (false, true) => "personal",
        (work, personal) => {
            panic!("one record should have run: work-tools {work}, personal-tools {personal}")
        }
    }
}

#[test]
fn an_inclusion_writing_no_overrides_leaves_the_runs_variables_alone() {
    // The baseline every case below is a departure from: the leaf's own `[vars]`
    // already reach what an inclusion contributes.
    let (_origin, tree) = including("");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(included_profile(&tree), "personal");
}

#[test]
fn an_override_decides_a_contributed_records_condition() {
    let (_origin, tree) = including("vars = { profile = \"work\" }\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(
        included_profile(&tree),
        "work",
        "the override did not reach the record it was written for:\n{stderr}"
    );
}

#[test]
fn an_override_does_not_reach_the_leafs_own_records() {
    // The leaf declares what it wants for itself in `[vars]`; what it writes on
    // an inclusion is for the repository it is including.
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        "[remotes.corporate]\ntype = \"git\"\nurl = \"{origin}\"\n\n\
         [vars]\nprofile = \"personal\"\n\n\
         [[actions]]\ntype = \"create-dir\"\nid = \"leaf-work\"\n\
         dest = \"~/.cache/leaf-work\"\nwhen = \"profile == 'work'\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"corp\"\nremote = \"corporate\"\n\
         vars = { profile = \"work\" }\n",
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(included_profile(&tree), "work");
    assert!(
        !tree.home(".cache/leaf-work").exists(),
        "an inclusion's override decided the leaf's own record:\n{stderr}"
    );
}

#[test]
fn an_override_reaches_only_the_inclusion_that_wrote_it() {
    // Two inclusions of one remote are two scopes. Both contribute the same
    // records, so which of them ran is read from the report rather than from the
    // destinations they share.
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        "[remotes.corporate]\ntype = \"git\"\nurl = \"{origin}\"\n\n\
         [vars]\nprofile = \"personal\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"corp\"\nremote = \"corporate\"\n\
         vars = { profile = \"work\" }\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"lab\"\nremote = \"corporate\"\n",
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    for expected in [
        "create-dir corp.personal-tools - skipped:",
        "create-dir lab.work-tools - skipped:",
    ] {
        assert!(
            stderr.contains(expected),
            "`{expected}` was not reported:\n{stderr}"
        );
    }
    for unexpected in [
        "create-dir corp.work-tools - skipped:",
        "create-dir lab.personal-tools - skipped:",
    ] {
        assert!(
            !stderr.contains(unexpected),
            "one inclusion's override decided the other's records:\n{stderr}"
        );
    }
}

#[test]
fn the_machine_the_environment_and_the_command_line_all_beat_an_override() {
    // The three layers above it, each checked against a leaf that wrote the
    // override and a remote whose records read the answer.
    for (what, machine, env, args) in [
        ("vars.toml", "profile = \"personal\"\n", None, Vec::new()),
        ("the environment", "", Some("personal"), Vec::new()),
        (
            "the command line",
            "",
            None,
            vec!["--var", "profile=personal"],
        ),
    ] {
        let (_origin, tree) = including("vars = { profile = \"work\" }\n");
        if !machine.is_empty() {
            tree.write_machine_vars(machine);
        }
        let mut command = tree.batfiles();
        command.arg("sync").args(&args);
        if let Some(value) = env {
            command.env("BATFILES_VAR_profile", value);
        }
        command.assert().success();

        assert_eq!(
            included_profile(&tree),
            "personal",
            "an override beat {what}"
        );
    }
}

#[test]
fn an_override_beats_the_leafs_own_declaration_of_the_same_name() {
    // The layer directly beneath it, and the reason the override is written at
    // all: the leaf uses one value itself and hands another to what it includes.
    let (_origin, tree) = including("vars = { profile = \"work\" }\n");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(included_profile(&tree), "work");
    assert!(
        stderr.contains("profile = \"personal\" (batfiles.toml)"),
        "the run's own set stopped reading the leaf's value:\n{stderr}"
    );
}

#[test]
fn an_inclusions_own_condition_is_decided_without_its_overrides() {
    // What an inclusion hands to the records it contributes cannot decide
    // whether it contributes them: the record's own condition is the leaf's,
    // read against the leaf's variables.
    let (_origin, tree) =
        including("when = \"profile == 'work'\"\nvars = { profile = \"work\" }\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("include-remote corp - skipped: when \"profile == 'work'\" is false"),
        "the inclusion's own condition read its overrides:\n{stderr}"
    );
    for dest in [".cache/work-tools", ".cache/personal-tools"] {
        assert!(
            !tree.home(dest).exists(),
            "a closed inclusion contributed `{dest}`:\n{stderr}"
        );
    }
}

#[test]
fn a_remotes_own_condition_is_decided_without_an_inclusions_overrides() {
    // A remote is materialized once for the run, however many inclusions name
    // it, so nothing one inclusion writes can decide it.
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        "[remotes.corporate]\ntype = \"git\"\nurl = \"{origin}\"\n\
         when = \"profile == 'work'\"\n\n\
         [vars]\nprofile = \"personal\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"corp\"\nremote = \"corporate\"\n\
         vars = { profile = \"work\" }\n",
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("remote `corporate` is excluded here"),
        "the remote's condition read the inclusion's overrides:\n{stderr}"
    );
    assert!(
        !tree.path("repo/remotes/corporate").exists(),
        "an excluded remote was materialized anyway:\n{stderr}"
    );
}

#[test]
fn an_included_clone_lists_entries_are_decided_in_the_inclusions_scope() {
    // A list is read while the run's lists are prepared rather than while the
    // record is selected, so this is the scope reaching the second of the two
    // places a contributed record's conditions are decided.
    let origin = BareRepo::new();
    let plugin = origin.another("zsh-z");
    origin.publish(
        "plugins.txt",
        &format!("{} when=\"profile == 'work'\"\n", display(&plugin)),
        "a list one variable decides",
    );
    origin.publish(
        "batfiles.toml",
        "[[actions]]\ntype = \"git-clone-list\"\nid = \"plugins\"\n\
         source = \"plugins.txt\"\ndest-dir = \"~/.plugins\"\n",
        "clone what the list names",
    );

    for (record, cloned) in [("vars = { profile = \"work\" }\n", true), ("", false)] {
        let tree = Tree::new();
        tree.write_manifest(&format!(
            "[remotes.corporate]\ntype = \"git\"\nurl = \"{{origin}}\"\n\n\
             [vars]\nprofile = \"personal\"\n\n\
             [[actions]]\ntype = \"include-remote\"\nid = \"corp\"\n\
             remote = \"corporate\"\n{record}"
        ));
        tree.point_at_origin(&origin);

        let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
        let stderr = stderr_of(&assertion);

        assert_eq!(
            tree.home(".plugins/zsh-z").is_dir(),
            cloned,
            "the entry's condition did not read the inclusion's scope:\n{stderr}"
        );
    }
}

#[test]
fn an_inclusions_overrides_are_reported_at_the_second_verbose_level() {
    // Under a heading naming the inclusion, and listing what it declared rather
    // than the whole set: what a contributed record's condition read is worth
    // seeing beside the run's own variables.
    let (_origin, tree) = including("vars = { profile = \"work\" }\n");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("include-remote `corp` variables:"),
        "the block was not reported:\n{stderr}"
    );
    assert!(
        stderr.contains("profile = \"work\" (include-remote `corp`; over batfiles.toml)"),
        "the line did not name the inclusion and what it overrode:\n{stderr}"
    );
}

#[test]
fn an_override_the_machine_beat_is_reported_with_the_layer_that_won() {
    // The case the block exists for: the leaf asked for one value, and this
    // machine has another.
    let (_origin, tree) = including("vars = { profile = \"work\" }\n");
    tree.write_machine_vars("profile = \"lab\"\n");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("profile = \"lab\" (vars.toml; over include-remote `corp`, batfiles.toml)"),
        "the block did not say the override lost:\n{stderr}"
    );
}

#[test]
fn an_inclusion_with_no_id_names_the_remote_in_its_block() {
    // The only other thing such a record can be called, and the same label its
    // skipped records are named by.
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        "[remotes.corporate]\ntype = \"git\"\nurl = \"{origin}\"\n\n\
         [vars]\nprofile = \"personal\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nremote = \"corporate\"\n\
         vars = { profile = \"work\" }\n",
    );
    tree.point_at_origin(&origin);

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("the include-remote of remote `corporate` variables:"),
        "the block did not name the inclusion the only way it can be named:\n{stderr}"
    );
}

#[test]
fn an_inclusion_writing_no_overrides_reports_no_block_of_its_own() {
    let (_origin, tree) = including("");

    let assertion = tree.batfiles().args(["sync", "-vv"]).assert().success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("variables:"),
        "the run's own variables were not reported:\n{stderr}"
    );
    assert!(
        !stderr.contains("include-remote `corp` variables:"),
        "an inclusion that overrode nothing reported a block:\n{stderr}"
    );
}

#[test]
fn an_unopened_inclusion_reports_nothing_about_its_overrides() {
    // Nothing was handed to anything: this command named a record of the leaf's
    // own, so it never read the manifest the overrides were written for.
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        "[remotes.corporate]\ntype = \"git\"\nurl = \"{origin}\"\n\n\
         [vars]\nprofile = \"personal\"\n\n\
         [[actions]]\ntype = \"create-dir\"\nid = \"cache\"\ndest = \"~/.cache/leaf\"\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"corp\"\nremote = \"corporate\"\n\
         vars = { profile = \"work\" }\n",
    );
    tree.point_at_origin(&origin);
    tree.batfiles().arg("sync").assert().success();

    let assertion = tree
        .batfiles()
        .args(["apply-action", "-vv", "--id", "cache"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        !stderr.contains("include-remote `corp` variables:"),
        "an inclusion this command never opened reported its overrides:\n{stderr}"
    );
}
