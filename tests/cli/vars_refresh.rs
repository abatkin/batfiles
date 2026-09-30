//! CLI tests for forced refresh of leaf and remote dynamic variables. Command marker files
//! record which declarations ran.

use std::fs;

use crate::support::*;

/// A command for `name` that records its run and prints `value`.
fn counted(name: &str, value: &str) -> String {
    format!(r#"{{ command = "echo ran >> {name}-runs; printf {value}" }}"#)
}

fn runs(tree: &Tree, file: &str) -> usize {
    fs::read_to_string(tree.path(&format!("repo/{file}")))
        .map(|text| text.lines().count())
        .unwrap_or_default()
}

fn cache(tree: &Tree) -> std::path::PathBuf {
    tree.path("cache").join("dynamic-vars.toml")
}

fn cache_document(tree: &Tree) -> String {
    fs::read_to_string(cache(tree)).expect("dynamic-vars.toml should exist")
}

/// A leaf declaring `email` and `shell` by command, and `editor` statically.
fn leaf() -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[vars]\nemail = {}\nshell = {}\neditor = \"nvim\"\n",
        counted("email", "me@example.com"),
        counted("shell", "zsh"),
    ));
    tree
}

fn refresh(tree: &Tree, args: &[&str]) -> String {
    let assertion = tree
        .batfiles()
        .args(["vars", "refresh"])
        .args(args)
        .assert()
        .success()
        .stdout("");
    stderr_of(&assertion)
}

fn refused(tree: &Tree, args: &[&str]) -> String {
    let assertion = tree
        .batfiles()
        .args(["vars", "refresh"])
        .args(args)
        .assert()
        .failure()
        .code(1)
        .stdout("");
    stderr_of(&assertion)
}

#[test]
fn every_leaf_command_runs_whatever_the_cache_holds() {
    let tree = leaf();
    assert_eq!(
        refresh(&tree, &[]),
        "refreshed `email`\nrefreshed `shell`\n"
    );
    refresh(&tree, &[]);
    assert_eq!(
        (runs(&tree, "email-runs"), runs(&tree, "shell-runs")),
        (2, 2)
    );
    let document = cache_document(&tree);
    assert!(
        document.contains("value = \"me@example.com\""),
        "{document}"
    );
    assert!(document.contains("value = \"zsh\""), "{document}");
}

#[test]
fn quiet_leaves_out_what_was_refreshed() {
    assert_eq!(refresh(&leaf(), &["--quiet"]), "");
}

#[test]
fn a_machine_local_value_does_not_spare_the_declaration_it_overrides() {
    let tree = leaf();
    tree.write_machine_vars("email = \"lab@example.com\"\n");
    refresh(&tree, &[]);
    assert_eq!(runs(&tree, "email-runs"), 1);
}

#[test]
fn a_named_key_runs_alone() {
    let tree = leaf();
    assert_eq!(refresh(&tree, &["shell"]), "refreshed `shell`\n");
    assert_eq!(
        (runs(&tree, "email-runs"), runs(&tree, "shell-runs")),
        (0, 1)
    );
    assert!(!cache_document(&tree).contains("[email]"));
}

#[test]
fn every_key_that_cannot_be_refreshed_is_named_and_nothing_runs() {
    let tree = leaf();
    let stderr = refused(&tree, &["shell", "nope", "editor", "ghost.team"]);
    assert_eq!(
        stderr,
        "error: cannot refresh 3 dynamic variables:\n  \
         `editor`: it is a static variable, with no command to run\n  \
         `nope`: batfiles.toml does not declare it\n  \
         `ghost.team`: batfiles.toml declares no remote `ghost`\n"
    );
    assert_eq!(runs(&tree, "shell-runs"), 0);
    assert!(!cache(&tree).exists());
}

#[test]
fn a_malformed_key_is_a_usage_error_before_any_root_is_resolved() {
    let assertion = batfiles()
        .args(["vars", "refresh", "remote:core.team"])
        .assert()
        .failure()
        .code(2);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`remote:core` is not a valid ID"),
        "{stderr}"
    );
}

#[test]
fn a_failed_command_fails_the_refresh_after_the_rest_are_saved() {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[vars]\nemail = {}\nshell = {{ command = \"exit 3\" }}\n",
        counted("email", "me@example.com"),
    ));
    let stderr = refused(&tree, &[]);
    assert!(stderr.contains("refreshed `email`\n"), "{stderr}");
    assert!(
        stderr.contains(
            "warning: dynamic variable `shell` could not be refreshed: exited with status 3"
        ),
        "{stderr}"
    );
    assert!(
        stderr.ends_with("error: 1 dynamic variable could not be refreshed\n"),
        "{stderr}"
    );
    assert!(cache_document(&tree).contains("value = \"me@example.com\""));
}

#[test]
fn with_nothing_dynamic_there_is_nothing_to_refresh_and_no_cache() {
    let tree = Tree::new();
    tree.write_manifest("[vars]\neditor = \"nvim\"\n");
    assert_eq!(refresh(&tree, &[]), "nothing to refresh\n");
    assert!(
        fs::read_dir(tree.path("cache"))
            .expect("cache root")
            .next()
            .is_none()
    );
}

#[test]
fn verbose_reports_every_root_it_resolved() {
    let tree = leaf();
    let stderr = refresh(&tree, &["-v"]);
    for label in ["repository:", "home:", "config:", "cache:"] {
        assert!(stderr.contains(label), "{label} missing:\n{stderr}");
    }
}

// Remotes

/// A remote whose manifest declares `team` by command and `site` statically.
fn remote() -> BareRepo {
    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        r#"[vars]
team = { command = "echo ran >> ../../remote-runs; printf platform" }
site = "hq"
"#,
        "a remote declaring a dynamic variable",
    );
    origin
}

/// A leaf including that remote twice, with `fields` on its `[remotes]` entry
/// and `leaf_vars` as its `[vars]`, synchronized once so the remote is
/// materialized.
fn including(fields: &str, leaf_vars: &str) -> (BareRepo, Tree) {
    let (origin, tree) = unsynchronized(fields, leaf_vars);
    tree.batfiles().arg("sync").assert().success();
    (origin, tree)
}

fn unsynchronized(fields: &str, leaf_vars: &str) -> (BareRepo, Tree) {
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[vars]
{leaf_vars}

[remotes.corporate]
type = "git"
url = "{{origin}}"
{fields}
[[actions]]
type = "include-remote"
id = "first"
remote = "corporate"

[[actions]]
type = "include-remote"
id = "second"
remote = "corporate"
"#
    ));
    tree.point_at_origin(&origin);
    (origin, tree)
}

const ALLOWED: &str = "allow-dynamic-vars = true\n";

#[test]
fn a_remote_in_play_runs_once_however_often_it_is_included() {
    let (_origin, tree) = including(ALLOWED, "");
    let before = runs(&tree, "remote-runs");
    assert_eq!(refresh(&tree, &[]), "refreshed `corporate.team`\n");
    assert_eq!(runs(&tree, "remote-runs"), before + 1);
}

#[test]
fn a_remote_key_runs_in_that_remotes_materialization() {
    let (_origin, tree) = including(ALLOWED, &format!("email = {}", counted("email", "x")));
    let before = runs(&tree, "email-runs");
    assert_eq!(
        refresh(&tree, &["corporate.team"]),
        "refreshed `corporate.team`\n"
    );
    assert_eq!(runs(&tree, "remote-runs"), 2);
    assert_eq!(runs(&tree, "email-runs"), before);
}

#[test]
fn a_remote_not_allowed_to_run_commands_refreshes_nothing() {
    let (_origin, tree) = including("", "");
    let stderr = refresh(&tree, &["-v"]);
    assert!(stderr.contains("nothing to refresh"), "{stderr}");
    assert!(
        stderr.contains("remote `corporate` is not allowed to run dynamic variables"),
        "{stderr}"
    );
    assert_eq!(runs(&tree, "remote-runs"), 0);

    let stderr = refused(&tree, &["corporate.team"]);
    assert!(
        stderr.contains(
            "cannot refresh `corporate.team`: remote `corporate` is not allowed to run \
             dynamic variables"
        ),
        "{stderr}"
    );
}

#[test]
fn a_remote_every_disabled_inclusion_names_is_not_in_play() {
    let (_origin, tree) = including(ALLOWED, "");
    tree.write_disabled("actions = [\"first\"]\n");
    refresh(&tree, &[]);
    assert_eq!(
        runs(&tree, "remote-runs"),
        2,
        "one enabled inclusion is enough"
    );

    tree.write_disabled("actions = [\"first\", \"second\"]\n");
    assert_eq!(refresh(&tree, &[]), "nothing to refresh\n");
    assert_eq!(runs(&tree, "remote-runs"), 2);

    let stderr = refused(&tree, &["corporate.team"]);
    assert!(
        stderr.contains(
            "cannot refresh `corporate.team`: remote `corporate` is not in play on this \
             machine: include-remote `first`: action `first` is disabled; include-remote \
             `second`: action `second` is disabled"
        ),
        "{stderr}"
    );
}

#[test]
fn a_run_only_skip_does_not_take_a_remote_out_of_play() {
    let (_origin, tree) = including(ALLOWED, "");
    tree.batfiles()
        .args(["vars", "refresh"])
        .env("BATFILES_SKIP_ACTIONS", "first,second")
        .assert()
        .success();
    assert_eq!(runs(&tree, "remote-runs"), 2);
}

#[test]
fn a_gate_reads_a_leaf_value_refreshed_by_the_same_command() {
    let (_origin, tree) = unsynchronized(
        "allow-dynamic-vars = true\nwhen = \"work == 'yes'\"\n",
        "work = { command = \"cat work.txt\" }",
    );
    tree.repo_file("work.txt", "yes");
    tree.batfiles().arg("sync").assert().success();
    assert_eq!(runs(&tree, "remote-runs"), 1);

    tree.repo_file("work.txt", "no");
    let stderr = refused(&tree, &["work", "corporate.team"]);
    assert!(
        stderr.contains(
            "cannot refresh `corporate.team`: remote `corporate` is not in play on this \
             machine"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains("when \"work == 'yes'\" is false"),
        "{stderr}"
    );
    assert_eq!(runs(&tree, "remote-runs"), 1);
    assert!(cache_document(&tree).contains("value = \"no\""));
}

#[test]
fn a_remote_in_play_that_is_not_materialized_warns_unless_it_was_named() {
    let (_origin, tree) = unsynchronized(ALLOWED, "");
    let stderr = refresh(&tree, &[]);
    assert!(
        stderr.contains("warning: remote `corporate` is not materialized at"),
        "{stderr}"
    );
    assert!(stderr.contains("nothing to refresh"), "{stderr}");

    let stderr = refused(&tree, &["corporate.team"]);
    assert!(
        stderr.contains("cannot refresh `corporate.team`: remote `corporate` is not materialized"),
        "{stderr}"
    );
}

#[test]
fn a_remote_key_must_name_a_dynamic_declaration_of_that_remote() {
    let (_origin, tree) = including(ALLOWED, "");
    let stderr = refused(&tree, &["corporate.nope", "corporate.site"]);
    assert_eq!(
        stderr,
        "error: cannot refresh 2 dynamic variables:\n  \
         `corporate.nope`: batfiles.toml of remote `corporate` does not declare it\n  \
         `corporate.site`: it is a static variable, with no command to run\n"
    );
    assert_eq!(runs(&tree, "remote-runs"), 1);
}

#[test]
fn a_remote_no_inclusion_names_is_not_in_play() {
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(
        "[remotes.corporate]\ntype = \"git\"\nurl = \"{origin}\"\nallow-dynamic-vars = true\n",
    );
    tree.point_at_origin(&origin);
    tree.batfiles().arg("sync").assert().success();
    assert_eq!(refresh(&tree, &[]), "nothing to refresh\n");
    let stderr = refused(&tree, &["corporate.team"]);
    assert!(
        stderr.contains("no include-remote in batfiles.toml includes remote `corporate`"),
        "{stderr}"
    );
}

#[test]
fn a_remote_in_play_is_refreshed_without_reading_what_it_contributes() {
    let (origin, tree) = unsynchronized(ALLOWED, "");
    origin.publish(
        "batfiles.toml",
        r#"[vars]
team = { command = "echo ran >> ../../remote-runs; printf platform" }

[[actions]]
type = "git-clone-list"
id = "plugins"
source = "missing.txt"
dest-dir = "~/.plugins"
"#,
        "a list the remote does not hold",
    );
    // Skip both inclusions so sync does not try to read the missing clone list.
    tree.batfiles()
        .args(["sync", "--skip-action", "first", "--skip-action", "second"])
        .assert()
        .success();

    assert_eq!(refresh(&tree, &[]), "refreshed `corporate.team`\n");
    assert!(!tree.home(".plugins").exists());
}

/// Create and synchronize a leaf including two command-enabled remotes: `alpha` from [`remote`]
/// and `zeta` from `zeta_manifest`.
fn two_remotes(zeta_manifest: &str) -> Tree {
    let alpha = remote();
    let zeta = BareRepo::new();
    zeta.publish("batfiles.toml", zeta_manifest, "the second remote");
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.alpha]
type = "git"
url = "{alpha}"
allow-dynamic-vars = true

[remotes.zeta]
type = "git"
url = "{zeta}"
allow-dynamic-vars = true

[[actions]]
type = "include-remote"
id = "a"
remote = "alpha"

[[actions]]
type = "include-remote"
id = "z"
remote = "zeta"
"#,
    );
    tree.point_remote_at("alpha", &alpha);
    tree.point_remote_at("zeta", &zeta);
    // Even if assembly fails, sync must materialize both remotes first.
    let _ = tree.batfiles().arg("sync").assert();
    for remote in ["alpha", "zeta"] {
        assert!(
            tree.path(&format!("repo/remotes/{remote}")).is_dir(),
            "`{remote}` was not materialized"
        );
    }
    tree
}

/// A remote manifest declaring `team` by command, with `actions` after it.
fn declaring_team(actions: &str) -> String {
    format!(
        "[vars]\nteam = {{ command = \"echo ran >> ../../remote-runs; printf platform\" }}\n\n{actions}"
    )
}

#[test]
fn a_named_remote_is_the_only_one_opened() {
    let tree = two_remotes("[vars\n");

    assert_eq!(refresh(&tree, &["alpha.team"]), "refreshed `alpha.team`\n");

    let stderr = refused(&tree, &[]);
    assert!(stderr.contains("remotes/zeta/batfiles.toml"), "{stderr}");
}

#[test]
fn an_included_manifest_is_validated_whole_though_refresh_prepares_nothing() {
    let tree = two_remotes(&declaring_team(
        "[[actions]]\ntype = \"create-dir\"\nid = \"nowhere\"\n",
    ));
    let before = runs(&tree, "remote-runs");

    let stderr = refused(&tree, &["zeta.team"]);

    assert!(stderr.contains("remotes/zeta/batfiles.toml"), "{stderr}");
    assert!(stderr.contains("dest"), "{stderr}");
    assert_eq!(runs(&tree, "remote-runs"), before);
}

#[test]
fn no_remote_command_runs_until_every_remote_key_is_checked() {
    let tree = two_remotes(&declaring_team(""));
    let before = runs(&tree, "remote-runs");

    let stderr = refused(&tree, &["alpha.team", "zeta.nope"]);

    assert!(
        stderr.contains(
            "cannot refresh `zeta.nope`: batfiles.toml of remote `zeta` does not declare it"
        ),
        "{stderr}"
    );
    assert_eq!(runs(&tree, "remote-runs"), before);
}

#[test]
fn leaf_keys_need_neither_the_inclusions_nor_the_disabled_lists() {
    let tree = leaf();
    let manifest = std::fs::read_to_string(tree.manifest()).expect("the leaf manifest");
    tree.write_manifest(&format!(
        "{manifest}\n[remotes.corporate]\ntype = \"git\"\nurl = \"/nowhere\"\n\
         allow-dynamic-vars = true\n\n[[actions]]\ntype = \"include-remote\"\n\
         id = \"corp\"\nremote = \"corporate\"\n"
    ));
    tree.write_disabled("actions = [\n");

    assert_eq!(refresh(&tree, &["email"]), "refreshed `email`\n");
    assert_eq!(runs(&tree, "email-runs"), 1);
}

#[test]
fn an_inclusion_whose_filters_take_nothing_keeps_its_remote_in_play() {
    let (_origin, tree) = including(ALLOWED, "");
    let manifest = std::fs::read_to_string(tree.manifest()).expect("the leaf manifest");
    tree.write_manifest(&manifest.replace(
        "remote = \"corporate\"\n",
        "remote = \"corporate\"\ninstall-actions = []\n",
    ));

    assert_eq!(refresh(&tree, &[]), "refreshed `corporate.team`\n");
}

#[test]
fn a_refresh_reports_nothing_composing_an_inclusion_would() {
    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        &declaring_team(
            "[remotes.elsewhere]\ntype = \"git\"\nurl = \"/nowhere\"\n\n\
             [[actions]]\ntype = \"include-remote\"\nid = \"deeper\"\nremote = \"elsewhere\"\n",
        ),
        "a remote reaching further",
    );
    let tree = Tree::new();
    tree.write_manifest(
        "[remotes.corporate]\ntype = \"git\"\nurl = \"{origin}\"\nallow-dynamic-vars = true\n\n\
         [[actions]]\ntype = \"include-remote\"\nid = \"corp\"\nremote = \"corporate\"\n\
         install-actions = [\"missing\"]\n",
    );
    tree.point_at_origin(&origin);
    let composing = [
        "ignoring the remotes",
        "not included:",
        "`missing` matched no action",
    ];

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    for warning in composing {
        assert!(stderr.contains(warning), "sync: `{warning}`:\n{stderr}");
    }

    let stderr = refresh(&tree, &["-v"]);
    assert!(stderr.contains("refreshed `corporate.team`"), "{stderr}");
    for warning in composing {
        assert!(!stderr.contains(warning), "refresh: `{warning}`:\n{stderr}");
    }
}

#[test]
fn an_excluded_inclusion_is_not_opened_whatever_its_manifest_holds() {
    let (origin, tree) = including(ALLOWED, "");
    origin.publish("batfiles.toml", "[vars\n", "a manifest that does not parse");
    tree.write_disabled("actions = [\"first\", \"second\"]\n");

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(refresh(&tree, &[]), "nothing to refresh\n");
}

#[test]
fn a_relative_repository_root_finds_the_remote_it_materialized() {
    let (_origin, tree) = including(ALLOWED, "");
    let before = runs(&tree, "remote-runs");

    tree.batfiles()
        .current_dir(tree.path("cache"))
        .env("BATFILES_DIR", "../repo")
        .args(["vars", "refresh", "corporate.team"])
        .assert()
        .success()
        .stderr("refreshed `corporate.team`\n");

    assert_eq!(runs(&tree, "remote-runs"), before + 1);
}
