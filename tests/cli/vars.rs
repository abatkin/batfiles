//! The machine-local variable commands, and the set a run merges out of the
//! four layers that can declare a variable.
//!
//! What the command tests pin down is the document, the account of the edit,
//! and which stream a value comes back on. What the merge tests pin down is
//! which layer wins, since no condition reads a value yet: `-vv` is where a run
//! says what it resolved.

use crate::support::*;
use std::fs;

/// A tree holding a `vars.toml` written by hand rather than by batfiles. The
/// single quotes are the point: the document is valid and is not what the
/// serializer would produce, so a command that rewrote it would say so.
fn with_document(document: &str) -> Tree {
    let tree = Tree::new();
    tree.write_machine_vars(document);
    tree
}

/// A `vars.toml` under a config directory that does not exist yet.
///
/// The one case that cannot use [`Tree`]'s config root, which is created up
/// front: what is under test is that a command creates neither the document nor
/// its directory when it changed nothing.
fn in_absent_config(tree: &Tree) -> (assert_cmd::Command, std::path::PathBuf) {
    let absent = tree.path("config/nested");
    let mut command = batfiles();
    command.arg("--config-dir").arg(&absent);
    // The other roots still have to resolve; only the config one is missing.
    command
        .env("BATFILES_HOME", tree.path("home"))
        .env("BATFILES_DIR", tree.path("repo"))
        .env("BATFILES_CACHE_DIR", tree.path("cache"));
    (command, absent)
}

// Setting.

#[test]
fn setting_a_variable_creates_the_document() {
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();

    assert_eq!(tree.machine_vars_document(), "editor = \"nvim\"\n");
    assert!(stderr_of(&assertion).contains("set `editor`"));
}

#[test]
fn the_written_document_is_sorted_whatever_order_the_keys_arrived_in() {
    let tree = Tree::new();
    for (key, value) in [("profile", "work"), ("editor", "nvim")] {
        tree.batfiles()
            .args(["vars", "set", key, value])
            .assert()
            .success();
    }

    assert_eq!(
        tree.machine_vars_document(),
        "editor = \"nvim\"\nprofile = \"work\"\n"
    );
}

#[test]
fn no_line_about_a_mutation_echoes_the_value() {
    // A value may be a token or a path that identifies a machine, and an
    // informational line would put it in scrollback and in a caller's logs.
    let tree = Tree::new();
    for arguments in [
        ["vars", "set", "token", "s3cr3t"],
        ["vars", "set", "token", "s3cr3t"],
        ["vars", "set", "token", "s3cr3t-2"],
    ] {
        let assertion = tree.batfiles().args(arguments).assert().success();
        let stderr = stderr_of(&assertion);
        assert!(!stderr.contains("s3cr3t"), "the value leaked:\n{stderr}");
        assert!(stderr.contains("`token`"), "unexpected stderr:\n{stderr}");
    }
}

#[test]
fn replacing_a_value_says_that_it_had_a_different_one() {
    let tree = Tree::new();
    tree.batfiles()
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();

    let assertion = tree
        .batfiles()
        .args(["vars", "set", "editor", "emacs"])
        .assert()
        .success();

    assert!(stderr_of(&assertion).contains("changed `editor` (it had a different value)"));
    assert_eq!(tree.machine_vars_document(), "editor = \"emacs\"\n");
}

#[test]
fn setting_the_value_already_there_does_not_rewrite_the_document() {
    let tree = with_document("editor = 'nvim'\n");
    let assertion = tree
        .batfiles()
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();

    assert!(stderr_of(&assertion).contains("`editor` was already set to that value"));
    assert_eq!(
        tree.machine_vars_document(),
        "editor = 'nvim'\n",
        "an idempotent set should leave the file exactly as it found it"
    );
}

// Reading.

#[test]
fn getting_a_value_prints_it_alone_on_standard_output() {
    let tree = Tree::new();
    tree.batfiles()
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();

    let assertion = tree
        .batfiles()
        .args(["vars", "get", "editor"])
        .assert()
        .success()
        .stdout("nvim\n");
    assert_eq!(stderr_of(&assertion), "");
}

#[test]
fn requested_data_survives_quiet_and_color() {
    // `--quiet` suppresses what a command did, not what it was asked for, and
    // nothing on standard output is ever labeled or colored.
    let tree = Tree::new();
    tree.batfiles()
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();

    for global in [&["--quiet"][..], &["--color", "always"], &["--verbose"]] {
        let assertion = tree
            .batfiles()
            .args(global)
            .args(["vars", "get", "editor"])
            .assert()
            .success();
        assert_eq!(stdout_of(&assertion), "nvim\n", "with {global:?}");
    }
}

#[test]
fn a_key_with_no_value_fails_and_prints_nothing() {
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .args(["vars", "get", "editor"])
        .assert()
        .failure()
        .code(1);

    assert_eq!(stdout_of(&assertion), "");
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`editor` has no machine-local value"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn an_empty_value_is_stored_and_stays_distinct_from_no_value() {
    // The reason `vars get` fails on an absent key rather than printing an
    // empty line: these two cases have to stay apart.
    let tree = Tree::new();
    tree.batfiles()
        .args(["vars", "set", "editor", ""])
        .assert()
        .success();
    assert_eq!(tree.machine_vars_document(), "editor = \"\"\n");

    tree.batfiles()
        .args(["vars", "get", "editor"])
        .assert()
        .success()
        .stdout("\n");

    tree.batfiles()
        .args(["vars", "unset", "editor"])
        .assert()
        .success();
    tree.batfiles()
        .args(["vars", "get", "editor"])
        .assert()
        .failure()
        .code(1);
}

// Removing.

#[test]
fn unsetting_a_key_removes_it_and_leaves_the_rest() {
    let tree = with_document("editor = 'nvim'\nprofile = 'work'\n");
    let assertion = tree
        .batfiles()
        .args(["vars", "unset", "editor"])
        .assert()
        .success();

    assert!(stderr_of(&assertion).contains("unset `editor`"));
    assert_eq!(tree.machine_vars_document(), "profile = \"work\"\n");
}

#[test]
fn unsetting_the_last_key_leaves_an_empty_document_behind() {
    let tree = Tree::new();
    tree.batfiles()
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();
    tree.batfiles()
        .args(["vars", "unset", "editor"])
        .assert()
        .success();

    assert_eq!(
        tree.machine_vars_document(),
        "",
        "the file should survive its last key"
    );
}

#[test]
fn unsetting_an_absent_key_writes_nothing_at_all() {
    let tree = Tree::new();
    let (mut command, absent) = in_absent_config(&tree);
    let assertion = command.args(["vars", "unset", "editor"]).assert().success();

    assert!(stderr_of(&assertion).contains("`editor` was not set"));
    assert!(
        !absent.exists(),
        "a no-op should create neither the document nor its directory"
    );
}

#[test]
fn unsetting_an_absent_key_does_not_rewrite_an_existing_document() {
    let tree = with_document("profile = 'work'\n");
    tree.batfiles()
        .args(["vars", "unset", "editor"])
        .assert()
        .success();

    assert_eq!(tree.machine_vars_document(), "profile = 'work'\n");
}

// Names and documents that are not valid.

#[test]
fn an_invalid_name_is_refused_before_the_document_is_touched() {
    // A malformed document would be fatal if it were read, so a run that fails
    // on the name alone is proof that nothing opened the file.
    let tree = with_document("editor = \n");
    for (key, expected) in [
        ("1up", "must start with a letter or underscore"),
        ("has-dash", "must start with a letter or underscore"),
        ("env", "reserved"),
    ] {
        let assertion = tree
            .batfiles()
            .args(["vars", "set", key, "x"])
            .assert()
            .failure()
            .code(1);
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(&format!("invalid variable name `{key}`")),
            "unexpected stderr:\n{stderr}"
        );
        assert!(stderr.contains(expected), "unexpected stderr:\n{stderr}");
    }

    assert_eq!(tree.machine_vars_document(), "editor = \n");
}

#[test]
fn every_command_refuses_an_invalid_name() {
    let tree = Tree::new();
    for arguments in [
        &["vars", "set", "1up", "x"][..],
        &["vars", "get", "1up"],
        &["vars", "unset", "1up"],
    ] {
        let assertion = tree.batfiles().args(arguments).assert().failure().code(1);
        assert!(
            stderr_of(&assertion).contains("invalid variable name `1up`"),
            "{arguments:?}"
        );
    }
}

#[test]
fn a_malformed_document_is_fatal_and_left_alone() {
    let tree = with_document("editor = \n");
    let assertion = tree
        .batfiles()
        .args(["vars", "get", "editor"])
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("vars.toml"), "unexpected stderr:\n{stderr}");
    assert_eq!(
        tree.machine_vars_document(),
        "editor = \n",
        "a failed load must not touch the file"
    );
}

#[test]
fn a_hand_written_key_that_is_not_a_variable_name_fails_the_document() {
    let tree = with_document("has-dash = 'x'\n");
    let assertion = tree
        .batfiles()
        .args(["vars", "get", "editor"])
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("a variable name must"),
        "unexpected stderr:\n{stderr}"
    );
}

// What these commands do not touch.

#[test]
fn the_repository_and_the_disabled_lists_are_left_alone_by_the_commands() {
    // These commands resolve roots and open one file. A malformed manifest is
    // the check that costs nothing to make and would catch a stray read.
    let tree = Tree::new();
    fs::write(tree.manifest(), "[[actions]\n").expect("a malformed manifest");

    tree.batfiles()
        .args(["vars", "set", "editor", "nvim"])
        .assert()
        .success();

    assert!(!tree.disabled().exists());
}

// The set a run merges.

/// A tree whose manifest declares `[vars]` and whose `vars.toml` overrides part
/// of it: the two documents, ready for the environment and the command line to
/// be layered on top.
fn with_two_documents() -> Tree {
    let tree = Tree::new();
    tree.write_manifest(
        "[vars]\n\
         editor = \"vi\"\n\
         profile = \"personal\"\n\
         rank = \"3\"\n\
         \n\
         # One action, so the commands that apply part of a manifest have\n\
         # something to apply while they resolve the same set.\n\
         [[actions]]\n\
         type = \"create-dir\"\n\
         id = \"zsh-cache\"\n\
         group = \"shell\"\n\
         dest = \"~/.cache/zsh\"\n",
    );
    tree.write_machine_vars("editor = 'nvim'\nprofile = 'work'\n");
    tree
}

#[test]
fn every_layer_overrides_the_one_below_it() {
    let tree = with_two_documents();
    let assertion = tree
        .batfiles()
        .env("BATFILES_VAR_editor", "code")
        .args(["-vv", "sync", "--var", "editor=emacs"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    for line in [
        "  editor  = \"emacs\" (--var; over BATFILES_VAR_*, vars.toml, batfiles.toml)",
        "  profile = \"work\" (vars.toml; over batfiles.toml)",
        "  rank    = \"3\" (batfiles.toml)",
    ] {
        assert!(stderr.contains(line), "no `{line}` in:\n{stderr}");
    }
}

#[test]
fn a_repeated_var_takes_the_last_value_written() {
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .args(["-vv", "sync", "--var", "p=first", "--var", "p=last"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("  p = \"last\" (--var)"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn an_empty_value_still_overrides_the_layers_below_it() {
    let tree = with_two_documents();
    let assertion = tree
        .batfiles()
        .args(["-vv", "sync", "--var", "profile="])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("  profile = \"\" (--var; over vars.toml, batfiles.toml)"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn every_command_that_executes_actions_resolves_the_same_set() {
    // Including a dry run, which resolves variables like any other: the set
    // describes the run rather than changing the home directory.
    let tree = with_two_documents();
    for args in [
        &["-vv", "sync"][..],
        &["-vv", "sync", "--dry-run"],
        &["-vv", "apply-group", "--group", "shell"],
    ] {
        let assertion = tree.batfiles().args(args).assert().success();
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("  editor  = \"nvim\" (vars.toml; over batfiles.toml)"),
            "no merged set for `{args:?}` in:\n{stderr}"
        );
    }
}

#[test]
fn the_set_is_shown_at_the_second_verbose_level_and_not_before() {
    let tree = with_two_documents();
    for args in [&["sync"][..], &["-v", "sync"], &["--quiet", "sync"]] {
        let assertion = tree.batfiles().args(args).assert().success();
        let stderr = stderr_of(&assertion);
        assert!(
            !stderr.contains("variables:"),
            "`{args:?}` showed the set:\n{stderr}"
        );
    }
}

#[test]
fn a_run_with_no_variables_anywhere_shows_nothing() {
    // The heading is not printed over an empty list, so `-vv` on a repository
    // that declares no variables says nothing about them at all.
    let tree = Tree::new();
    let assertion = tree.batfiles().args(["-vv", "sync"]).assert().success();
    assert!(
        !stderr_of(&assertion).contains("variables:"),
        "unexpected stderr"
    );
}

#[test]
fn an_unusable_environment_name_is_warned_about_and_the_run_goes_on() {
    // The environment is ambient and may predate any interest in batfiles, so
    // one bad name in it drops that variable rather than stopping the run.
    let tree = with_two_documents();
    let assertion = tree
        .batfiles()
        .env("BATFILES_VAR_1up", "x")
        .env("BATFILES_VAR_env", "x")
        .env("BATFILES_VAR_rank", "9")
        .args(["-vv", "sync"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    for expected in [
        "ignoring \"BATFILES_VAR_1up\"",
        "ignoring \"BATFILES_VAR_env\"",
        "reserved",
        // The usable one in the same environment still lands.
        "  rank    = \"9\" (BATFILES_VAR_*; over batfiles.toml)",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_warning_about_an_environment_name_never_echoes_its_value() {
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .env("BATFILES_VAR_1up", "s3cr3t")
        .arg("sync")
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(!stderr.contains("s3cr3t"), "the value leaked:\n{stderr}");
    assert!(
        stderr.contains("ignoring \"BATFILES_VAR_1up\""),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn a_value_cannot_forge_a_line_of_batfiles_own() {
    // A value is whatever a repository, a hand-edited state file, or the
    // environment put there. Printed raw, one holding a newline would end the
    // line it sits on and start one that reads like a batfiles diagnostic.
    let tree = Tree::new();
    tree.write_manifest("[vars]\nmischief = \"ok\\nerror: forged\"\n");

    let assertion = tree
        .batfiles()
        .env("BATFILES_VAR_terminal", "\u{1b}[31mred")
        .args(["-vv", "sync"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    for line in [
        "  mischief = \"ok\\nerror: forged\" (batfiles.toml)",
        "  terminal = \"\\u{1b}[31mred\" (BATFILES_VAR_*)",
    ] {
        assert!(stderr.contains(line), "no `{line}` in:\n{stderr}");
    }
    assert!(
        !stderr.lines().any(|line| line.starts_with("error:")),
        "a value forged a diagnostic:\n{stderr}"
    );
    assert!(
        !stderr.contains('\u{1b}'),
        "an escape sequence reached the terminal:\n{stderr}"
    );
}

#[test]
fn a_rejected_environment_name_cannot_forge_one_either() {
    // The same rule for the other half: a warning names a suffix that failed
    // the name check, so it is arbitrary text off the environment.
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .env("BATFILES_VAR_1up\nerror: forged", "x")
        .arg("sync")
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("ignoring \"BATFILES_VAR_1up\\nerror: forged\""),
        "unexpected stderr:\n{stderr}"
    );
    assert!(
        !stderr.lines().any(|line| line.starts_with("error:")),
        "a variable name forged a diagnostic:\n{stderr}"
    );
}

#[test]
fn a_malformed_machine_document_fails_a_run_that_merges_it() {
    // `vars.toml` is read by every command that executes actions now, so a
    // document that cannot be parsed stops one the way a bad manifest does.
    let tree = with_document("editor = \n");
    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("vars.toml"), "unexpected stderr:\n{stderr}");
}
