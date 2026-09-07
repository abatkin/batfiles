//! The machine-local variable commands.
//!
//! They read and write `vars.toml` and nothing else, so what these tests pin
//! down is the document, the account of the edit, and which stream a value
//! comes back on. Nothing yet reads these values into a run.

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
fn the_repository_and_the_disabled_lists_are_left_alone() {
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
