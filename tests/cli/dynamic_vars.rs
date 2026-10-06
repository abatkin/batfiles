//! CLI tests for dynamic variables and their cache. Command marker files distinguish execution
//! from cache reuse.

use std::fs;

use jiff::{SignedDuration, Timestamp};

use crate::support::*;

/// A leaf declaring `profile` by `command`, and one directory its condition
/// decides: `~/work` exists exactly when `profile` read `work`.
fn declaring(declaration: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[vars]
profile = {declaration}

[[actions]]
type = "create-dir"
dest = "~/work"
when = "profile == 'work'"
"#
    ));
    tree
}

/// A declaration that records its run and prints `work`.
const COUNTED: &str = r#"{ command = "echo ran >> runs; printf work" }"#;

/// How many times the leaf's commands ran.
fn runs(tree: &Tree) -> usize {
    fs::read_to_string(tree.path("repo/runs"))
        .map(|text| text.lines().count())
        .unwrap_or_default()
}

fn cache(tree: &Tree) -> std::path::PathBuf {
    tree.path("cache").join("dynamic-vars.toml")
}

fn cache_document(tree: &Tree) -> String {
    fs::read_to_string(cache(tree)).expect("dynamic-vars.toml should exist")
}

/// Put one leaf entry in the cache, captured `ago` before now.
fn seed(tree: &Tree, name: &str, value: &str, ago: SignedDuration) {
    let captured = Timestamp::now() - ago;
    fs::write(
        cache(tree),
        format!("[{name}]\nvalue = \"{value}\"\ncaptured-at = \"{captured}\"\n"),
    )
    .expect("a cache document");
}

fn two_days() -> SignedDuration {
    SignedDuration::from_hours(48)
}

fn sync(tree: &Tree, args: &[&str]) -> String {
    let assertion = tree.batfiles().arg("sync").args(args).assert().success();
    stderr_of(&assertion)
}

#[test]
fn a_condition_is_decided_by_what_the_command_printed() {
    let tree = declaring(COUNTED);
    let stderr = sync(&tree, &["-vv"]);
    assert!(tree.home("work").is_dir(), "{stderr}");
    assert!(
        stderr.contains(r#"profile = "work" (batfiles.toml, command)"#),
        "{stderr}"
    );
    let document = cache_document(&tree);
    assert!(document.contains("[profile]"), "{document}");
    assert!(document.contains("value = \"work\""), "{document}");
    assert!(document.contains("captured-at = \""), "{document}");
}

#[test]
fn a_fresh_value_is_reused_and_a_stale_one_is_captured_again() {
    let tree = declaring(COUNTED);
    sync(&tree, &[]);
    let stderr = sync(&tree, &["-vv"]);
    assert_eq!(runs(&tree), 1, "a fresh value ran again");
    assert!(stderr.contains("(batfiles.toml, cached"), "{stderr}");

    seed(&tree, "profile", "old", two_days());
    sync(&tree, &[]);
    assert_eq!(runs(&tree), 2, "a stale value was not captured again");
    assert!(cache_document(&tree).contains("value = \"work\""));
}

#[test]
fn a_cache_duration_decides_what_is_fresh() {
    let tree = declaring(r#"{ command = "echo ran >> runs; printf work", cache = "0s" }"#);
    sync(&tree, &[]);
    sync(&tree, &[]);
    assert_eq!(runs(&tree), 2, "a zero duration is never fresh");
}

#[test]
fn refresh_vars_runs_a_command_whose_value_is_fresh() {
    let tree = declaring(COUNTED);
    sync(&tree, &[]);
    sync(&tree, &["--refresh-vars"]);
    assert_eq!(runs(&tree), 2);

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "missing", "--refresh-vars"])
        .assert()
        .failure()
        .code(1);
    assert_eq!(runs(&tree), 3, "{}", stderr_of(&assertion));
}

#[test]
fn a_failed_refresh_keeps_the_cached_value_and_says_so() {
    let tree = declaring(r#"{ command = "exit 3" }"#);
    seed(&tree, "profile", "work", two_days());
    let stderr = sync(&tree, &["-vv"]);
    assert!(tree.home("work").is_dir(), "{stderr}");
    assert!(
        stderr.contains(
            "warning: dynamic variable `profile` could not be refreshed: exited with status 3; \
             using the value cached 2d ago"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains(r#"profile = "work" (batfiles.toml, command failed, cached 2d ago)"#),
        "{stderr}"
    );
}

#[test]
fn a_failure_with_nothing_cached_reads_as_empty() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[vars]
profile = { command = "exit 3" }

[[actions]]
type = "create-dir"
dest = "~/empty"
when = "profile == ''"
"#,
    );
    let stderr = sync(&tree, &["-vv"]);
    assert!(tree.home("empty").is_dir(), "{stderr}");
    assert!(
        stderr.contains("it has no value, and reads as empty"),
        "{stderr}"
    );
    assert!(
        stderr.contains("profile = no value (batfiles.toml, command failed)"),
        "{stderr}"
    );
    assert!(!cache(&tree).exists(), "a failure was cached");
}

#[test]
fn a_status_capture_is_true_or_false() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[vars]
yes = { command = ["sh", "-c", "exit 0"], capture = "status" }
no = { command = ["sh", "-c", "exit 1"], capture = "status" }

[[actions]]
type = "create-dir"
dest = "~/decided"
when = "yes && !no"
"#,
    );
    let stderr = sync(&tree, &[]);
    assert!(tree.home("decided").is_dir(), "{stderr}");
    let document = cache_document(&tree);
    assert!(document.contains("value = \"true\""), "{document}");
    assert!(document.contains("value = \"false\""), "{document}");
}

#[test]
fn a_status_command_that_cannot_start_is_false_for_this_run_only() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[vars]
has_tool = { command = ["batfiles-no-such-program"], capture = "status" }

[[actions]]
type = "create-dir"
dest = "~/without"
unless = "has_tool"
"#,
    );
    let stderr = sync(&tree, &["-vv"]);
    assert!(tree.home("without").is_dir(), "{stderr}");
    assert!(
        stderr.contains("dynamic variable `has_tool` could not be started"),
        "{stderr}"
    );
    assert!(stderr.contains("assuming `false` for this run"), "{stderr}");
    assert!(
        stderr.contains(r#"has_tool = "false" (batfiles.toml, command could not start)"#),
        "{stderr}"
    );
    assert!(!cache(&tree).exists(), "an assumed value was cached");
}

#[test]
fn a_command_past_its_timeout_is_killed_and_fails() {
    let tree = declaring(r#"{ command = ["sleep", "10"], command-timeout = "200ms" }"#);
    let started = std::time::Instant::now();
    let stderr = sync(&tree, &[]);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(stderr.contains("timed out after 200ms"), "{stderr}");
    assert!(!tree.home("work").exists());
}

#[test]
fn a_command_runs_in_the_repository_that_declared_it() {
    let tree = declaring(r#"{ command = "cat profile.txt" }"#);
    tree.repo_file("profile.txt", "work\n");
    let stderr = sync(&tree, &[]);
    assert!(tree.home("work").is_dir(), "{stderr}");
}

#[test]
fn quiet_disconnects_a_commands_standard_error() {
    let tree = declaring(r#"{ command = "echo from-the-command >&2; printf work", cache = "0s" }"#);
    assert!(sync(&tree, &[]).contains("from-the-command"));
    assert!(!sync(&tree, &["--quiet"]).contains("from-the-command"));
}

#[test]
fn a_dry_run_runs_commands_and_writes_the_cache() {
    let tree = declaring(COUNTED);
    let stderr = sync(&tree, &["--dry-run"]);
    assert!(stderr.contains("would create"), "{stderr}");
    assert!(!tree.home("work").exists());
    assert_eq!(runs(&tree), 1);
    assert!(cache_document(&tree).contains("value = \"work\""));
}

#[test]
fn a_run_with_nothing_dynamic_leaves_the_cache_alone() {
    let tree = declaring("\"work\"");
    sync(&tree, &[]);
    assert!(tree.home("work").is_dir());
    assert!(!cache(&tree).exists());
    assert_eq!(tree.cache_entries(), ["run.lock"]);
}

#[test]
fn a_malformed_cache_fails_the_run_and_is_left_alone() {
    let tree = declaring(COUNTED);
    fs::write(cache(&tree), "[profile]\nvalue = 3\n").expect("a cache document");
    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains(&display(&cache(&tree))), "{stderr}");
    assert_eq!(runs(&tree), 0);
    assert_eq!(cache_document(&tree), "[profile]\nvalue = 3\n");
}

#[test]
fn the_cache_follows_the_selected_cache_root() {
    let tree = declaring(COUNTED);
    let elsewhere = tree.path("elsewhere");
    tree.batfiles()
        .arg("sync")
        .arg("--cache-dir")
        .arg(&elsewhere)
        .assert()
        .success();
    assert!(elsewhere.join("dynamic-vars.toml").is_file());
    assert!(!cache(&tree).exists());
}

// `vars list`

fn list(tree: &Tree, args: &[&str]) -> String {
    let assertion = tree
        .batfiles()
        .args(["vars", "list"])
        .args(args)
        .assert()
        .success();
    stdout_of(&assertion)
}

#[test]
fn a_listing_shows_how_each_dynamic_value_arrived() {
    let tree = declaring(COUNTED);
    assert_eq!(
        list(&tree, &[]),
        "profile = \"work\" (batfiles.toml, command)\n"
    );
    assert_eq!(
        list(&tree, &[]),
        "profile = \"work\" (batfiles.toml, cached just now)\n"
    );
    assert_eq!(runs(&tree), 1);
}

#[test]
fn a_listing_leaves_unrun_what_a_machine_local_value_shadows() {
    let tree = declaring(COUNTED);
    tree.write_machine_vars("profile = \"lab\"\n");
    assert_eq!(
        list(&tree, &[]),
        "profile = \"lab\" (vars.toml; over batfiles.toml)\n"
    );
    assert_eq!(runs(&tree), 0);
    assert!(!cache(&tree).exists());

    sync(&tree, &[]);
    assert_eq!(runs(&tree), 1);
}

#[test]
fn no_refresh_runs_nothing_and_reports_what_the_cache_holds() {
    let tree = declaring(COUNTED);
    assert_eq!(
        list(&tree, &["--no-refresh"]),
        "profile = no value (batfiles.toml, not cached)\n"
    );
    assert!(!cache(&tree).exists());

    seed(&tree, "profile", "old", two_days());
    let before = cache_document(&tree);
    assert_eq!(
        list(&tree, &["--no-refresh"]),
        "profile = \"old\" (batfiles.toml, stale, cached 2d ago)\n"
    );
    assert_eq!(runs(&tree), 0);
    assert_eq!(cache_document(&tree), before);
}

// Remotes

/// A remote whose manifest declares `team` by command, deciding one directory.
fn remote() -> BareRepo {
    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        r#"[vars]
team = { command = "echo ran >> ../../runs; printf platform" }

[[actions]]
type = "create-dir"
id = "team-tools"
dest = "~/team"
when = "team == 'platform'"
"#,
        "a remote declaring a dynamic variable",
    );
    origin
}

/// A leaf including that remote twice, with `fields` on its `[remotes]` entry.
fn including(fields: &str) -> (BareRepo, Tree) {
    let origin = remote();
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
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

#[test]
fn a_remote_runs_no_command_unless_the_leaf_allows_it() {
    let (_origin, tree) = including("");
    let stderr = sync(&tree, &["-v"]);
    assert_eq!(runs(&tree), 0);
    assert!(!tree.home("team").exists(), "{stderr}");
    assert!(
        stderr.contains(
            "remote `corporate` is not allowed to run dynamic variables, so it does not \
             declare `team`"
        ),
        "{stderr}"
    );
    assert!(!cache(&tree).exists());
}

#[test]
fn an_allowed_remote_runs_its_command_once_for_every_inclusion() {
    let (_origin, tree) = including("allow-dynamic-vars = true\n");
    let stderr = sync(&tree, &["-vv"]);
    assert!(tree.home("team").is_dir(), "{stderr}");
    assert_eq!(runs(&tree), 1, "{stderr}");
    assert!(
        stderr.contains(r#"team = "platform" (batfiles.toml of include-remote `second`, command)"#),
        "{stderr}"
    );
    let document = cache_document(&tree);
    assert!(
        document.contains("[\"remote:corporate.team\"]"),
        "{document}"
    );
}

#[test]
fn a_remote_its_condition_excludes_runs_nothing() {
    let (_origin, tree) = including("allow-dynamic-vars = true\nwhen = \"false\"\n");
    sync(&tree, &[]);
    assert_eq!(runs(&tree), 0);
    assert!(!cache(&tree).exists());
}

#[test]
fn what_an_inclusion_captured_is_kept_when_a_later_one_fails() {
    let good = remote();
    let broken = BareRepo::new();
    broken.publish("batfiles.toml", "[[actions]]\n", "an invalid manifest");
    let tree = Tree::new();
    tree.write_manifest(
        r#"[remotes.corporate]
type = "git"
url = "{corporate}"
allow-dynamic-vars = true

[remotes.broken]
type = "git"
url = "{broken}"

[[actions]]
type = "include-remote"
remote = "corporate"

[[actions]]
type = "include-remote"
remote = "broken"
"#,
    );
    tree.point_remote_at("corporate", &good);
    tree.point_remote_at("broken", &broken);

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert_eq!(runs(&tree), 1, "{}", stderr_of(&assertion));
    let document = cache_document(&tree);
    assert!(
        document.contains("[\"remote:corporate.team\"]"),
        "{document}"
    );
}
