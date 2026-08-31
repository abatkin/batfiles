//! The persistent enable/disable commands.
//!
//! They read and write `disabled.toml` and nothing else, so what these tests
//! pin down is the document and the account of the edit. What a run then *does*
//! with the lists belongs to the filter, and is in `selection`.

use crate::support::*;

/// A `disabled.toml` under a config directory that does not exist yet.
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

#[test]
fn disabling_an_action_creates_the_document() {
    let tree = Tree::new();
    tree.batfiles()
        .args(["disable-action", "p10k"])
        .assert()
        .success();

    assert_eq!(
        tree.disabled_document(),
        "actions = [\"p10k\"]\ngroups = []\n"
    );
}

#[test]
fn a_group_goes_in_the_other_list() {
    let tree = Tree::new();
    tree.batfiles()
        .args(["disable-group", "work"])
        .assert()
        .success();

    assert_eq!(
        tree.disabled_document(),
        "actions = []\ngroups = [\"work\"]\n"
    );
}

#[test]
fn a_name_matching_nothing_is_recorded_without_complaint() {
    // These commands never open the manifest, so they cannot tell a typo from a
    // name a later branch will introduce. Pre-registering one is the point.
    let tree = Tree::new();
    let assertion = tree
        .batfiles()
        .args(["disable-action", "not-in-any-manifest"])
        .assert()
        .success();

    assert_eq!(
        stderr_of(&assertion),
        "disabled action `not-in-any-manifest`\n"
    );
    assert_eq!(
        tree.disabled_document(),
        "actions = [\"not-in-any-manifest\"]\ngroups = []\n"
    );
}

#[test]
fn an_address_is_recorded_exactly_as_written() {
    // Qualified and deeper-than-resolvable addresses alike: these commands
    // validate syntax and resolve nothing, so `a.b.c.d.e` is simply an address
    // that names nothing yet. The order is the dotted text's, which puts
    // `a.b.c.d.e` ahead of `core.zshrc` and the shorter `core.zshrc` ahead of
    // the name extending it.
    let tree = Tree::new();
    tree.batfiles()
        .args([
            "disable-action",
            "core.zshrc",
            "core.zshrc.plugin",
            "a.b.c.d.e",
        ])
        .assert()
        .success();

    assert_eq!(
        tree.disabled_document(),
        "actions = [\"a.b.c.d.e\", \"core.zshrc\", \"core.zshrc.plugin\"]\ngroups = []\n"
    );

    // And an address comes back out the way any other name does.
    tree.batfiles()
        .args(["enable-action", "core.zshrc"])
        .assert()
        .success();
    assert_eq!(
        tree.disabled_document(),
        "actions = [\"a.b.c.d.e\", \"core.zshrc.plugin\"]\ngroups = []\n"
    );
}

#[test]
fn enabling_removes_a_name_and_keeps_an_empty_document() {
    let tree = Tree::new();
    // Given out of order, so the document that lands is the sorted one.
    tree.batfiles()
        .args(["disable-action", "zshrc", "p10k"])
        .assert()
        .success();
    assert_eq!(
        tree.disabled_document(),
        "actions = [\"p10k\", \"zshrc\"]\ngroups = []\n"
    );

    tree.batfiles()
        .args(["enable-action", "p10k"])
        .assert()
        .success();
    assert_eq!(
        tree.disabled_document(),
        "actions = [\"zshrc\"]\ngroups = []\n"
    );

    // Emptying the last entry keeps a canonical document rather than deleting it.
    tree.batfiles()
        .args(["enable-action", "zshrc"])
        .assert()
        .success();
    assert_eq!(tree.disabled_document(), "actions = []\ngroups = []\n");
}

#[test]
fn each_outcome_line_says_whether_the_state_moved() {
    let tree = Tree::new();
    tree.batfiles()
        .args(["disable-action", "p10k"])
        .assert()
        .success()
        .stderr("disabled action `p10k`\n");

    let assertion = tree
        .batfiles()
        .args(["disable-action", "p10k"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        "action `p10k` was already disabled\n"
    );

    // The one that matters most: re-enabling something that really was off.
    let assertion = tree
        .batfiles()
        .args(["enable-action", "p10k"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        "enabled action `p10k` (was disabled)\n"
    );

    let assertion = tree
        .batfiles()
        .args(["enable-action", "p10k"])
        .assert()
        .success();
    assert_eq!(stderr_of(&assertion), "action `p10k` was already enabled\n");
}

#[test]
fn quiet_suppresses_the_outcome_lines_without_suppressing_the_work() {
    let tree = Tree::new();
    for args in [
        ["--quiet", "disable-action", "p10k"],
        ["--quiet", "enable-action", "p10k"],
        ["--quiet", "enable-action", "p10k"],
    ] {
        tree.batfiles().args(args).assert().success().stderr("");
    }

    assert_eq!(tree.disabled_document(), "actions = []\ngroups = []\n");
}

#[test]
fn a_repeated_name_warns_and_is_applied_once() {
    let tree = Tree::new();
    // Under `--quiet`, so the warning is the only thing that can appear.
    let assertion = tree
        .batfiles()
        .args(["--quiet", "disable-action", "p10k", "p10k"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`p10k` was given more than once"),
        "unexpected stderr:\n{stderr}"
    );
    assert_eq!(
        tree.disabled_document(),
        "actions = [\"p10k\"]\ngroups = []\n"
    );
}

#[test]
fn an_invalid_name_fails_before_anything_is_written() {
    // Unsorted and duplicated on purpose: a save of any kind would canonicalize
    // it, so byte-identical content is the proof nothing was written.
    let tree = Tree::new();
    let original = "actions = [\"p10k\", \"zshrc\", \"p10k\"]\n";
    tree.write_disabled(original);

    // `vim` is perfectly valid and is still not applied: the invocation either
    // applies in full or changes nothing.
    let assertion = tree
        .batfiles()
        .args(["disable-action", "vim", "core..p10k"])
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`core..p10k`"),
        "the error should name the offending value:\n{stderr}"
    );
    assert_eq!(tree.disabled_document(), original);
}

#[test]
fn a_no_op_mutation_does_not_rewrite_the_document() {
    let tree = Tree::new();
    let original = "actions = [\"p10k\", \"zshrc\", \"p10k\"]\ngroups = []\n";
    tree.write_disabled(original);

    tree.batfiles()
        .args(["enable-action", "absent"])
        .assert()
        .success();
    assert_eq!(tree.disabled_document(), original);

    tree.batfiles()
        .args(["disable-action", "p10k"])
        .assert()
        .success();
    assert_eq!(tree.disabled_document(), original);
}

#[test]
fn a_no_op_against_a_missing_file_creates_nothing() {
    // A command that changed nothing does not leave a `disabled.toml` — or a
    // config directory — behind just because it ran.
    let tree = Tree::new();
    let (mut command, absent) = in_absent_config(&tree);

    command.args(["enable-action", "p10k"]).assert().success();

    assert!(
        !absent.exists(),
        "the config directory should not be created"
    );
}

#[test]
fn a_malformed_entry_in_the_file_fails_the_command() {
    // Enabling is deliberately not a repair path for a file that does not parse.
    let tree = Tree::new();
    let original = "actions = [\"my action\"]\n";
    tree.write_disabled(original);

    for command in ["disable-action", "enable-action"] {
        let assertion = tree
            .batfiles()
            .args([command, "p10k"])
            .assert()
            .failure()
            .code(1);

        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("`my action`"),
            "`{command}` should name the offending entry:\n{stderr}"
        );
        assert_eq!(tree.disabled_document(), original);
    }
}

#[test]
fn the_leaf_repository_is_never_loaded() {
    // A `batfiles.toml` that cannot parse, and then no leaf repository at all.
    // A command that never opens the manifest cannot be failed by it.
    let tree = Tree::new();
    tree.write_manifest("actions = \n");

    tree.batfiles()
        .args(["disable-action", "p10k"])
        .assert()
        .success();
    tree.batfiles()
        .arg("--batfiles-dir")
        .arg(tree.path("repo/absent"))
        .args(["enable-action", "p10k"])
        .assert()
        .success();

    assert_eq!(tree.disabled_document(), "actions = []\ngroups = []\n");
}

/// A save that cannot happen must not be reported as though it did.
///
/// Restricting a directory is the only way to get past the load and fail at the
/// write, so this is Unix-only; `src/tomlfile.rs` covers the write path itself
/// on both platforms.
#[cfg(unix)]
mod unwritable {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn a_failed_save_reports_no_outcome_at_all() {
        let tree = Tree::new();
        let config = tree.path("config");
        fs::set_permissions(&config, fs::Permissions::from_mode(0o555)).expect("restrict");

        // A process that can write anyway — root — cannot exercise this, and a
        // test that passes because the restriction did nothing proves nothing.
        let writable = fs::File::create(config.join(".probe")).is_ok();
        if writable {
            fs::remove_file(config.join(".probe")).expect("remove the probe");
            fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).expect("restore");
            return;
        }

        let assertion = tree
            .batfiles()
            .args(["disable-action", "p10k"])
            .assert()
            .failure()
            .code(1);
        let stderr = stderr_of(&assertion);

        fs::set_permissions(&config, fs::Permissions::from_mode(0o755)).expect("restore");

        assert!(
            !stderr.contains("disabled action"),
            "the run reported a change it did not make:\n{stderr}"
        );
        assert!(
            stderr.contains("could not write"),
            "the run should say why it failed:\n{stderr}"
        );
        assert!(
            !tree.disabled().exists(),
            "nothing should have been written"
        );
    }
}
