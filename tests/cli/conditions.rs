//! `when` and `unless`: the record's own say in whether it runs.
//!
//! A condition is the one exclusion the repository declares rather than the
//! machine, which is what separates most of this file from `selection.rs`: the
//! variables a condition is decided against come from four layers, and the
//! record either belongs on this machine or does not.
//!
//! Nothing here needs a symlink, so it runs on every platform, and what a run
//! installed is exactly the set of names under the home.

use crate::support::*;

/// Two `create-dir` actions in one group, the first carrying `condition`
/// written in the given spelling.
fn gated(tree: &Tree, spelling: &str, condition: &str, vars: &str) {
    tree.write_manifest(&format!(
        "{vars}\
         [[actions]]\n\
         type = \"create-dir\"\n\
         id = \"gated\"\n\
         group = \"shell\"\n\
         dest = \"~/gated\"\n\
         {spelling} = \"{condition}\"\n\
         \n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         id = \"plain\"\n\
         group = \"shell\"\n\
         dest = \"~/plain\"\n"
    ));
}

/// What a `sync` over [`gated`] leaves under the home.
fn installed(tree: &Tree, args: &[&str]) -> Vec<String> {
    tree.batfiles().arg("sync").args(args).assert().success();
    entries(&tree.path("home"))
}

// What the two spellings decide.

#[test]
fn a_true_when_runs_the_record_and_a_false_one_does_not() {
    let tree = Tree::new();
    gated(&tree, "when", "work", "[vars]\nwork = \"true\"\n\n");
    assert_eq!(installed(&tree, &[]), ["gated", "plain"]);

    let tree = Tree::new();
    gated(&tree, "when", "work", "[vars]\nwork = \"false\"\n\n");
    assert_eq!(installed(&tree, &[]), ["plain"]);
}

#[test]
fn unless_is_the_other_way_round_rather_than_a_second_when() {
    // Written out as its own case because this is the one a reader gets
    // backwards, and a `when` that happened to work for both would hide it.
    let tree = Tree::new();
    gated(&tree, "unless", "work", "[vars]\nwork = \"true\"\n\n");
    assert_eq!(installed(&tree, &[]), ["plain"]);

    let tree = Tree::new();
    gated(&tree, "unless", "work", "[vars]\nwork = \"false\"\n\n");
    assert_eq!(installed(&tree, &[]), ["gated", "plain"]);
}

#[test]
fn a_record_with_no_condition_is_reached_by_nothing_here() {
    // The reason every case above asserts two names: `plain` runs whatever the
    // variables say, so a condition that closed the whole run would show up.
    let tree = Tree::new();
    gated(&tree, "when", "work", "[vars]\nwork = \"false\"\n\n");
    assert_eq!(installed(&tree, &[]), ["plain"]);
}

// What a condition may read.

#[test]
fn every_variable_layer_reaches_a_condition() {
    // One condition, decided four times over: the layers are one flat scope, so
    // a name resolves the same way whatever declared it.
    let tree = Tree::new();
    gated(&tree, "when", "work", "[vars]\nwork = \"false\"\n\n");
    assert_eq!(installed(&tree, &[]), ["plain"]);

    // The command line, over the manifest's `false`.
    let tree = Tree::new();
    gated(&tree, "when", "work", "[vars]\nwork = \"false\"\n\n");
    assert_eq!(installed(&tree, &["--var", "work=yes"]), ["gated", "plain"]);

    // The environment, over the same.
    let tree = Tree::new();
    gated(&tree, "when", "work", "[vars]\nwork = \"false\"\n\n");
    tree.batfiles()
        .arg("sync")
        .env("BATFILES_VAR_work", "on")
        .assert()
        .success();
    assert_eq!(entries(&tree.path("home")), ["gated", "plain"]);

    // And the machine-local document, which no manifest has to mention.
    let tree = Tree::new();
    gated(&tree, "when", "work", "");
    tree.write_machine_vars("work = \"1\"\n");
    assert_eq!(installed(&tree, &[]), ["gated", "plain"]);
}

#[test]
fn a_condition_reads_the_host_through_facts_and_env() {
    // Asserted against `std::env::consts` rather than a named platform, so this
    // decides the same way on every runner.
    let tree = Tree::new();
    gated(
        &tree,
        "when",
        &format!("facts.os == '{}'", std::env::consts::OS),
        "",
    );
    assert_eq!(installed(&tree, &[]), ["gated", "plain"]);

    // A fact batfiles does not define is empty rather than an error, which is
    // what makes the set extensible and a typo quiet.
    let tree = Tree::new();
    gated(&tree, "when", "facts.osx == 'macos'", "");
    assert_eq!(installed(&tree, &[]), ["plain"]);

    let tree = Tree::new();
    gated(&tree, "when", "env.DESKTOP == 'gnome'", "");
    tree.batfiles()
        .arg("sync")
        .env("DESKTOP", "gnome")
        .assert()
        .success();
    assert_eq!(entries(&tree.path("home")), ["gated", "plain"]);
}

#[test]
fn the_vars_namespace_is_total_where_a_bare_name_is_strict() {
    // The spelling for a variable that is legitimately optional: absent, it is
    // false rather than a failed run.
    let tree = Tree::new();
    gated(&tree, "when", "vars.work", "");
    assert_eq!(installed(&tree, &[]), ["plain"]);

    let tree = Tree::new();
    gated(&tree, "when", "vars.work", "");
    assert_eq!(installed(&tree, &["--var", "work=yes"]), ["gated", "plain"]);
}

// How a closed record is reported.

#[test]
fn a_closed_record_is_reported_at_v_with_the_condition_as_written() {
    let tree = Tree::new();
    gated(
        &tree,
        "when",
        "work && facts.os == 'plan9'",
        "[vars]\nwork = \"true\"\n\n",
    );

    let quiet = tree.batfiles().arg("sync").assert().success();
    assert!(
        !stderr_of(&quiet).contains("skipped"),
        "a record the manifest excludes is not news at normal verbosity:\n{}",
        stderr_of(&quiet)
    );

    let verbose = tree.batfiles().args(["sync", "-v"]).assert().success();
    assert!(
        stderr_of(&verbose).contains(
            "create-dir gated (group shell) - skipped: \
             when \"work && facts.os == 'plan9'\" is false"
        ),
        "the heading should say which condition closed the record:\n{}",
        stderr_of(&verbose)
    );
}

#[test]
fn an_unless_says_it_is_true_rather_than_that_it_is_false() {
    let tree = Tree::new();
    gated(&tree, "unless", "work", "[vars]\nwork = \"yes\"\n\n");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    assert!(
        stderr_of(&assertion)
            .contains("create-dir gated (group shell) - skipped: unless \"work\" is true"),
        "the spelling the record used should be the one reported:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_disable_is_reported_ahead_of_the_condition_that_would_also_have_closed_it() {
    // Precedence among the reasons: the disable is the one still in force
    // tomorrow, and the condition is never even evaluated.
    let tree = Tree::new();
    gated(&tree, "when", "work", "[vars]\nwork = \"false\"\n\n");
    tree.batfiles()
        .args(["disable-action", "gated"])
        .assert()
        .success();

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    assert!(
        stderr_of(&assertion)
            .contains("create-dir gated (group shell) - skipped: action `gated` is disabled"),
        "the disable should be the reason reported:\n{}",
        stderr_of(&assertion)
    );
}

// What the two commands that name a record make of one.

#[test]
fn apply_action_carries_out_the_record_it_names_whatever_its_condition_says() {
    // The waiver is the action tier's, and a condition sits in it: nothing is
    // finer-grained than the one record `apply-action` was given.
    let tree = Tree::new();
    gated(&tree, "when", "work", "[vars]\nwork = \"false\"\n\n");

    tree.batfiles()
        .args(["apply-action", "--id", "gated"])
        .assert()
        .success();

    assert_eq!(entries(&tree.path("home")), ["gated"]);
}

#[test]
fn apply_group_honors_the_conditions_of_the_records_in_it() {
    // A group is coarser than one record, so naming it waives the group-level
    // lists and not what each member says about itself.
    let tree = Tree::new();
    gated(&tree, "when", "work", "[vars]\nwork = \"false\"\n\n");

    tree.batfiles()
        .args(["apply-group", "--group", "shell"])
        .assert()
        .success();

    assert_eq!(entries(&tree.path("home")), ["plain"]);
}

#[test]
fn a_group_every_condition_closes_says_there_was_nothing_to_apply() {
    let tree = Tree::new();
    tree.write_manifest(
        "[vars]\n\
         work = \"false\"\n\
         \n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         id = \"gated\"\n\
         group = \"shell\"\n\
         dest = \"~/gated\"\n\
         when = \"work\"\n",
    );

    let assertion = tree
        .batfiles()
        .args(["apply-group", "--group", "shell"])
        .assert()
        .success();

    assert!(
        stderr_of(&assertion).contains("nothing to apply"),
        "a group whose every member is closed should say so:\n{}",
        stderr_of(&assertion)
    );
}

// What a manifest may write.

#[test]
fn a_record_writes_one_condition_or_none() {
    let tree = Tree::new();
    tree.write_manifest(
        "[[actions]]\n\
         type = \"create-dir\"\n\
         dest = \"~/x\"\n\
         when = \"work\"\n\
         unless = \"work\"\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains("action 1: writes both `when` and `unless`"),
        "the record was accepted:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_malformed_condition_is_a_load_error_rather_than_a_surprise_partway_through() {
    let tree = Tree::new();
    tree.write_manifest(
        "[[actions]]\n\
         type = \"create-dir\"\n\
         id = \"first\"\n\
         dest = \"~/first\"\n\
         \n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         dest = \"~/second\"\n\
         when = \"work &&\"\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    for expected in [
        display(&tree.manifest()),
        "is not a valid condition".to_owned(),
        "at character".to_owned(),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert!(
        !tree.home("first").exists(),
        "the action before the malformed condition ran anyway"
    );
}

// What a condition batfiles cannot decide costs.

#[test]
fn an_undeclared_name_closes_the_gate_and_warns_rather_than_stopping() {
    // No `-v`: a warning is the one line about a passed-over record that a run
    // prints whether or not detail was asked for, because nothing about it was
    // asked for.
    let tree = Tree::new();
    gated(&tree, "when", "work", "");

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    for expected in [
        "create-dir gated (group shell)",
        "when \"work\" cannot be evaluated, so it is not installed",
        "`work` is not declared",
        "batfiles vars set work",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    // The gate closed, and the record after it was carried out all the same:
    // one bad identifier costs its own record and nothing else.
    assert_eq!(entries(&tree.path("home")), ["plain"]);
}

#[test]
fn an_unless_that_cannot_be_decided_closes_rather_than_installing() {
    // The asymmetry the two spellings hide. A false `unless` opens a gate, so
    // treating a failure as false would install the very record the line was
    // written to suppress.
    let tree = Tree::new();
    gated(&tree, "unless", "no_gui_", "");

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("unless \"no_gui_\" cannot be evaluated, so it is not installed"),
        "the warning should name the spelling that decided it:\n{stderr}"
    );
    assert_eq!(entries(&tree.path("home")), ["plain"]);
}

#[test]
fn a_value_outside_the_truthiness_table_closes_without_repeating_the_value() {
    // A condition is the one place a value reaches a diagnostic without having
    // been asked for, and a manifest batfiles evaluates is not always the
    // user's own.
    let tree = Tree::new();
    gated(
        &tree,
        "when",
        "token",
        "[vars]\ntoken = \"s3cret-value\"\n\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("is not a boolean"),
        "the value should have been refused:\n{stderr}"
    );
    assert!(
        !stderr.contains("s3cret-value"),
        "the value was echoed:\n{stderr}"
    );
    assert_eq!(entries(&tree.path("home")), ["plain"]);
}

#[test]
fn a_condition_on_a_record_something_else_excludes_is_never_evaluated() {
    // Which is what keeps one undecidable condition from costing a run that was
    // never going to carry the record out.
    let tree = Tree::new();
    gated(&tree, "when", "nothing_declares_this", "");

    tree.batfiles()
        .args(["sync", "--skip-action", "gated"])
        .assert()
        .success();

    assert_eq!(entries(&tree.path("home")), ["plain"]);
}

#[test]
fn apply_action_reaches_a_record_whose_condition_cannot_be_decided() {
    // The waiver, which is what a reader does about the warning: naming one
    // record reaches it even where a `sync` over the same manifest passes it
    // over, and the condition is not evaluated at all.
    let tree = Tree::new();
    gated(&tree, "when", "nothing_declares_this", "");

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(entries(&tree.path("home")), ["plain"]);

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "gated"])
        .assert()
        .success();
    assert!(
        !stderr_of(&assertion).contains("cannot be evaluated"),
        "a waived condition should not be evaluated at all"
    );
    assert_eq!(entries(&tree.path("home")), ["gated", "plain"]);
}

// A dry run decides conditions the way an ordinary one does.

#[test]
fn a_dry_run_reports_the_record_its_condition_closes() {
    let tree = Tree::new();
    gated(&tree, "when", "work", "[vars]\nwork = \"false\"\n\n");

    let assertion = tree
        .batfiles()
        .args(["sync", "-v", "--dry-run"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("create-dir gated (group shell) - skipped: when \"work\" is false"),
        "a dry run should report the same decision:\n{stderr}"
    );
    assert!(
        stderr.contains("would create"),
        "the record nothing closed should still report its work:\n{stderr}"
    );
    assert!(
        entries(&tree.path("home")).is_empty(),
        "a dry run installed something"
    );
}
