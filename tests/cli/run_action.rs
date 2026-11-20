//! CLI tests for the `run` action: launching, output, failure, triggers, `runs.toml`, and
//! commands in included manifests. Commands append to marker files in the repository root,
//! where they run, so a test can count how often each ran.

use std::fs;

use jiff::{SignedDuration, Timestamp};

use crate::support::*;

/// A leaf whose manifest is `manifest`.
fn leaf(manifest: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(manifest);
    tree
}

/// How many lines the marker `name` in the repository root holds.
fn count(tree: &Tree, name: &str) -> usize {
    fs::read_to_string(tree.path(&format!("repo/{name}")))
        .map(|text| text.lines().count())
        .unwrap_or_default()
}

fn runs_toml(tree: &Tree) -> std::path::PathBuf {
    tree.path("state/runs.toml")
}

fn runs_document(tree: &Tree) -> String {
    fs::read_to_string(runs_toml(tree)).expect("runs.toml should exist")
}

fn write_runs(tree: &Tree, document: &str) {
    fs::create_dir_all(tree.path("state")).expect("a state directory");
    fs::write(runs_toml(tree), document).expect("runs.toml");
}

/// A `runs.toml` saved in generation 1, in which each of `ids` succeeded at `at`.
fn succeeded(ids: &[&str], at: Timestamp) -> String {
    let mut document = "generation = 1\n".to_owned();
    for id in ids {
        document.push_str(&format!(
            "\n[actions.{id}]\nlast-success = \"{at}\"\nsuccess-generation = 1\n"
        ));
    }
    document
}

/// Run `args` and require success, returning standard error.
fn ok(tree: &Tree, args: &[&str]) -> String {
    let assertion = tree.batfiles().args(args).assert().success();
    stderr_of(&assertion)
}

/// Run `args` and require exit status 1, returning standard error.
fn fails(tree: &Tree, args: &[&str]) -> String {
    let assertion = tree.batfiles().args(args).assert().code(1);
    stderr_of(&assertion)
}

/// One `run` action named `id`, running `command` (written as TOML) under `trigger`.
fn one_run(id: &str, command: &str, trigger: &str) -> String {
    format!(
        "[[actions]]\ntype = \"run\"\nid = \"{id}\"\ncommand = {command}\ntrigger = {trigger}\n"
    )
}

#[test]
fn an_always_command_runs_on_every_sync_in_either_form() {
    let tree = leaf(&format!(
        "{}\n{}",
        one_run("string", r#""echo string >> ran""#, r#""always""#),
        r#"[[actions]]
type = "run"
command = ["sh", "-c", "echo list >> ran"]
"#
    ));
    ok(&tree, &["sync"]);
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 4);
    let document = runs_document(&tree);
    assert!(
        document.contains("[actions.string]\nlast-changed = 2\n") && !document.contains("success"),
        "an always command records its change and no success:\n{document}"
    );
}

#[test]
fn a_command_runs_in_the_repository_root_with_its_input_closed() {
    let tree = leaf(&one_run(
        "where",
        r#""pwd > cwd; cat > input""#,
        r#""always""#,
    ));
    ok(&tree, &["sync"]);
    let cwd = fs::read_to_string(tree.path("repo/cwd")).expect("the command ran");
    assert_eq!(cwd.trim_end(), display(&tree.path("repo")));
    assert_eq!(
        fs::read_to_string(tree.path("repo/input")).expect("input"),
        ""
    );
}

#[test]
fn output_passes_through_and_quiet_keeps_only_standard_error() {
    let tree = leaf(&one_run(
        "talk",
        r#""echo to-stdout; echo to-stderr >&2""#,
        r#""always""#,
    ));
    let assertion = tree.batfiles().arg("sync").assert().success();
    assert_eq!(stdout_of(&assertion), "to-stdout\n");
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("to-stderr"), "{stderr}");
    assert!(
        stderr.contains("run talk: echo to-stdout; echo to-stderr >&2"),
        "the command was not named before it ran:\n{stderr}"
    );

    let assertion = tree.batfiles().args(["sync", "-q"]).assert().success();
    assert_eq!(stdout_of(&assertion), "");
    let stderr = stderr_of(&assertion);
    assert_eq!(stderr, "to-stderr\n");
}

#[test]
fn a_failed_command_stops_the_run() {
    let tree = leaf(&format!(
        "{}\n{}",
        one_run("fail", r#""exit 3""#, r#""always""#),
        one_create_dir("~/after")
    ));
    let stderr = fails(&tree, &["sync"]);
    assert!(
        stderr.contains("error: run fail: the command exited with status 3"),
        "{stderr}"
    );
    assert!(!tree.home("after").exists(), "the run continued");
}

#[test]
fn allow_failure_warns_and_the_run_continues() {
    let tree = leaf(&format!(
        "{}allow-failure = true\n\n{}",
        one_run("fail", r#""exit 3""#, r#""once""#),
        one_create_dir("~/after")
    ));
    let stderr = ok(&tree, &["sync"]);
    assert!(
        stderr.contains("warning: run fail: the command exited with status 3"),
        "{stderr}"
    );
    assert!(tree.home("after").is_dir());
    assert!(!runs_toml(&tree).exists(), "a failure records nothing");
}

#[test]
fn a_command_past_its_timeout_is_killed_and_fails() {
    let tree = leaf(&format!(
        "{}command-timeout = \"200ms\"\n",
        one_run("slow", r#"["sleep", "10"]"#, r#""always""#)
    ));
    let started = std::time::Instant::now();
    let stderr = fails(&tree, &["sync"]);
    assert!(
        started.elapsed().as_secs() < 8,
        "the command was not killed"
    );
    assert!(
        stderr.contains("run slow: the command timed out"),
        "{stderr}"
    );
}

#[test]
fn a_command_that_cannot_start_fails() {
    let tree = leaf(&one_run(
        "missing",
        r#"["batfiles-test-no-such-program"]"#,
        r#""always""#,
    ));
    let stderr = fails(&tree, &["sync"]);
    assert!(
        stderr.contains("run missing: the command could not be started"),
        "{stderr}"
    );
}

#[test]
fn a_bootstrap_command_runs_only_in_a_bootstrap() {
    let tree = leaf(&one_run("boot", r#""echo ran >> ran""#, r#""bootstrap""#));
    let stderr = ok(&tree, &["sync", "-v"]);
    assert_eq!(count(&tree, "ran"), 0);
    assert!(stderr.contains("not run: not a bootstrap run"), "{stderr}");
    ok(&tree, &["sync", "--bootstrap"]);
    assert_eq!(count(&tree, "ran"), 1);

    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        &one_run("boot", r#""echo ran >> ran""#, r#""bootstrap""#),
        "a bootstrap command",
    );
    let fresh = Tree::roots();
    ok(&fresh, &["clone", &display(&origin.origin())]);
    assert_eq!(count(&fresh, "ran"), 1);
}

#[test]
fn a_once_command_runs_until_it_succeeds_and_then_never_again() {
    // Fails the first time, and succeeds from then on.
    let command = r#""test -f tried || { touch tried; exit 1; }; echo ran >> ran""#;
    let tree = leaf(&one_run("setup", command, r#""once""#));
    fails(&tree, &["sync"]);
    assert!(!runs_toml(&tree).exists());
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1);
    assert!(
        runs_document(&tree).contains("[actions.setup]\nlast-success = \""),
        "{}",
        runs_document(&tree)
    );
    let stderr = ok(&tree, &["sync", "-v"]);
    assert_eq!(count(&tree, "ran"), 1);
    assert!(stderr.contains("not run: already ran"), "{stderr}");

    // A changed command resets nothing; a new ID starts fresh.
    tree.write_manifest(&one_run("setup", r#""echo changed >> ran""#, r#""once""#));
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1);
    tree.write_manifest(&one_run(
        "setup-v2",
        r#""echo renamed >> ran""#,
        r#""once""#,
    ));
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 2);
    let document = runs_document(&tree);
    assert!(
        document.contains("[actions.setup]") && document.contains("[actions.setup-v2]"),
        "an entry for an address no longer declared was dropped:\n{document}"
    );
}

#[test]
fn an_every_command_runs_when_its_last_success_is_old_enough() {
    let tree = leaf(&one_run(
        "daily",
        r#""echo ran >> ran""#,
        r#"{ every = "24h" }"#,
    ));
    let seed = |ago: SignedDuration| {
        write_runs(&tree, &succeeded(&["daily"], Timestamp::now() - ago));
    };

    seed(SignedDuration::from_hours(3));
    let stderr = ok(&tree, &["sync", "-v"]);
    assert_eq!(count(&tree, "ran"), 0);
    assert!(stderr.contains("not run: ran 3h ago"), "{stderr}");

    seed(SignedDuration::from_hours(-3));
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 0, "a future success is fresh");

    seed(SignedDuration::from_hours(25));
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1);
}

/// A copy of `rc` to `~/.rc`, and a command that follows it.
fn following_a_copy() -> Tree {
    let tree = leaf(&format!(
        r#"[[actions]]
type = "copy"
id = "rc"
source = "rc"
dest = "~/.rc"

{}"#,
        one_run("reload", r#""echo ran >> ran""#, r#"{ after = "rc" }"#)
    ));
    tree.repo_file("rc", "first\n");
    tree
}

#[test]
fn an_after_command_runs_when_what_it_follows_changes() {
    let tree = following_a_copy();
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1, "it never succeeded, so it runs");

    let stderr = ok(&tree, &["sync", "-v"]);
    assert_eq!(count(&tree, "ran"), 1);
    assert!(
        stderr.contains("not run: nothing it follows changed"),
        "{stderr}"
    );

    tree.repo_file("rc", "second\n");
    ok(&tree, &["sync", "--refresh-content"]);
    assert_eq!(count(&tree, "ran"), 2);
    assert!(runs_document(&tree).contains("[actions.reload]\nlast-success = "));
}

#[test]
fn an_after_command_runs_when_a_file_remote_it_follows_is_fetched_again() {
    let tree = Tree::new();
    let write = |version: &str| {
        let file = tree.path(&format!("tool-{version}"));
        fs::write(&file, version).expect("a release");
        tree.write_manifest(&format!(
            r#"[vars]
version = "{version}"

[remotes.tool]
type = "file"
url = "{}"

{}"#,
            file_url(&tree.path("tool-${vars.version}")),
            one_run("upgrade", r#""echo ran >> ran""#, r#"{ after = "@tool" }"#)
        ));
    };
    write("1");
    ok(&tree, &["sync"]);
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1);
    write("2");
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 2);
}

#[test]
fn recreating_an_empty_directory_wide_destination_runs_its_follower() {
    for kind in ["copy-dir", "symlink-dir"] {
        let tree = leaf(&format!(
            r#"[[actions]]
type = "{kind}"
id = "conf"
source-dir = "empty"
dest-dir = "~/conf"

{}"#,
            one_run("follow", r#""echo ran >> ran""#, r#"{ after = "conf" }"#)
        ));
        fs::create_dir(tree.path("repo/empty")).expect("an empty source");
        ok(&tree, &["sync"]);
        ok(&tree, &["sync"]);
        assert_eq!(count(&tree, "ran"), 1, "{kind}");

        fs::remove_dir(tree.home("conf")).expect("the destination");
        ok(&tree, &["sync"]);
        assert!(tree.home("conf").is_dir(), "{kind}");
        assert_eq!(
            count(&tree, "ran"),
            2,
            "{kind}: creating the directory is a change"
        );
    }
}

/// A directory, and a disabled command following it.
fn following_a_directory() -> Tree {
    let tree = leaf(&format!(
        r#"[[actions]]
type = "create-dir"
id = "dir"
dest = "~/dir"

{}"#,
        one_run("follow", r#""echo ran >> ran""#, r#"{ after = "dir" }"#)
    ));
    ok(&tree, &["disable-action", "follow"]);
    tree
}

#[test]
fn a_disabled_follower_runs_once_it_is_enabled() {
    let tree = following_a_directory();
    write_runs(&tree, &succeeded(&["follow"], Timestamp::now()));
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 0);
    let document = runs_document(&tree);
    assert!(
        document.starts_with("generation = 2\n")
            && document.contains("[actions.dir]\nlast-changed = 2\n"),
        "{document}"
    );

    // Later runs that change nothing leave the change unhandled.
    ok(&tree, &["sync"]);
    ok(&tree, &["enable-action", "follow"]);
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1);
    let stderr = ok(&tree, &["sync", "-v"]);
    assert_eq!(count(&tree, "ran"), 1);
    assert!(
        stderr.contains("not run: nothing it follows changed"),
        "{stderr}"
    );
}

#[test]
fn an_existing_directory_leaves_its_follower_alone() {
    let tree = following_a_directory();
    fs::create_dir(tree.home("dir")).expect("a directory");
    ok(&tree, &["sync"]);
    assert!(!runs_toml(&tree).exists(), "nothing changed or ran");
}

#[test]
fn a_follower_stays_due_through_a_failed_command_and_a_failed_action() {
    let tree = leaf(&format!(
        r#"[[actions]]
type = "create-dir"
id = "dir"
dest = "~/dir"

[[actions]]
type = "copy"
source = "later"
dest = "~/later"

{}"#,
        one_run(
            "follow",
            r#""test -f ok && echo ran >> ran""#,
            r#"{ after = "dir" }"#
        )
    ));
    write_runs(&tree, &succeeded(&["follow"], Timestamp::now()));
    // The copy has no source yet, so the run stops before the command, keeping the change.
    fails(&tree, &["sync"]);
    let changed = runs_document(&tree);
    assert!(
        changed.starts_with("generation = 2\n")
            && changed.contains("[actions.dir]\nlast-changed = 2\n"),
        "{changed}"
    );

    // The command fails without its flag file, and records no success.
    tree.repo_file("later", "now there\n");
    fails(&tree, &["sync"]);
    assert!(
        runs_document(&tree).contains("success-generation = 1\n"),
        "{}",
        runs_document(&tree)
    );

    tree.repo_file("ok", "");
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1);
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1, "one success handles the change");
}

#[test]
fn a_remote_update_before_a_failed_preparation_still_runs_its_follower() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.core]
type = "git"
url = "{{origin}}"

[[actions]]
type = "git-clone-list"
source = "plugins.txt"
dest-dir = "~/.plugins"

{}"#,
        one_run("rebuild", r#""echo ran >> ran""#, r#"{ after = "@core" }"#)
    ));
    tree.point_at_origin(&origin);
    tree.repo_file("plugins.txt", "");
    ok(&tree, &["sync"]);
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1, "it succeeded once");

    origin.publish("README.md", "a new release\n", "second");
    tree.repo_file("plugins.txt", "https://e.example/a.git colour=blue\n");
    fails(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1, "preparation stopped the run");

    // The remote no longer changes, but its earlier update is still unhandled.
    tree.repo_file("plugins.txt", "");
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 2);
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 2);
}

#[test]
fn an_earlier_runs_toml_keeps_its_successes_and_its_pending_firings() {
    let tree = leaf(&format!(
        "{}\n{}\n{}",
        one_run(
            "pending",
            r#""echo pending >> ran""#,
            r#"{ every = "24h" }"#
        ),
        one_run("settled", r#""echo settled >> ran""#, r#""once""#),
        one_run(
            "waiting",
            r#""echo waiting >> ran""#,
            r#"{ after = "settled" }"#
        ),
    ));
    let now = Timestamp::now();
    write_runs(
        &tree,
        &format!(
            "[pending]\nlast-success = \"{now}\"\npending = true\n\n\
             [settled]\nlast-success = \"{now}\"\n\n\
             [waiting]\nlast-success = \"{now}\"\npending = true\n"
        ),
    );
    ok(&tree, &["sync"]);
    assert_eq!(
        fs::read_to_string(tree.path("repo/ran")).expect("the marker"),
        "waiting\n",
        "only the pending `after` command ran"
    );
    let document = runs_document(&tree);
    assert!(
        document.starts_with("generation = 1\n")
            && document.contains(&format!(
                "[actions.settled]\nlast-success = \"{now}\"\nsuccess-generation = 0\n"
            ))
            && !document.contains("pending = "),
        "{document}"
    );
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1);
}

/// A remote holding `manifest`, declared with `fields` and included as `corp`, ahead of the
/// leaf's `actions`.
fn including(manifest: &str, fields: &str, actions: &str) -> (BareRepo, Tree) {
    let origin = BareRepo::new();
    origin.publish("batfiles.toml", manifest, "a remote manifest");
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{{origin}}"
{fields}

[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"

{actions}"#
    ));
    tree.point_at_origin(&origin);
    (origin, tree)
}

#[test]
fn an_after_command_runs_when_an_inclusion_it_follows_changes() {
    let (_origin, tree) = including(
        "[[actions]]\ntype = \"create-dir\"\nid = \"team\"\ndest = \"~/team\"\n",
        "",
        &one_run("follow", r#""echo ran >> ran""#, r#"{ after = "corp" }"#),
    );
    ok(&tree, &["sync"]);
    ok(&tree, &["sync"]);
    assert_eq!(
        count(&tree, "ran"),
        1,
        "nothing the inclusion contributed changed"
    );

    fs::remove_dir(tree.home("team")).expect("the contributed directory");
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 2);
}

#[test]
fn a_remote_and_an_inclusion_of_the_same_name_change_apart() {
    let origin = BareRepo::new();
    origin.publish(
        "batfiles.toml",
        "[[actions]]\ntype = \"create-dir\"\ndest = \"~/team\"\n",
        "a remote manifest",
    );
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.core]
type = "git"
url = "{{origin}}"

[[actions]]
type = "include-remote"
id = "core"
remote = "core"

{}
{}"#,
        one_run(
            "installed",
            r#""echo installed >> ran""#,
            r#"{ after = "core" }"#
        ),
        one_run("moved", r#""echo moved >> ran""#, r#"{ after = "@core" }"#),
    ));
    tree.point_at_origin(&origin);
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 2);

    origin.publish("README.md", "a new release\n", "second");
    ok(&tree, &["sync"]);
    assert_eq!(
        fs::read_to_string(tree.path("repo/ran")).expect("the marker"),
        "installed\nmoved\nmoved\n",
        "the checkout moved, and nothing the inclusion installed changed"
    );
}

/// A remote manifest with one `run` action that records its runs in the leaf root.
const CONTRIBUTED_RUN: &str = r#"[[actions]]
type = "run"
id = "setup"
command = "echo ran >> ../../ran"
trigger = "once"
"#;

#[test]
fn a_contributed_command_needs_the_leafs_permission() {
    let (_origin, tree) = including(CONTRIBUTED_RUN, "", "");
    let stderr = ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 0);
    assert!(
        stderr.contains(
            "warning: run corp.setup - skipped: remote `corporate` does not set \
             `allow-commands = true`"
        ),
        "{stderr}"
    );

    let stderr = fails(&tree, &["apply-action", "--id", "corp.setup"]);
    assert!(
        stderr.contains("does not set `allow-commands = true`"),
        "{stderr}"
    );
    assert_eq!(count(&tree, "ran"), 0);
}

#[test]
fn a_permitted_contributed_command_keeps_state_under_its_qualified_address() {
    let (_origin, tree) = including(CONTRIBUTED_RUN, "allow-commands = true", "");
    ok(&tree, &["sync"]);
    ok(&tree, &["sync"]);
    assert_eq!(count(&tree, "ran"), 1);
    assert!(
        runs_document(&tree).contains("[actions.\"corp.setup\"]\nlast-success = "),
        "{}",
        runs_document(&tree)
    );
}

#[test]
fn a_stateful_command_from_an_unnamed_inclusion_is_refused() {
    let (_origin, tree) = including(CONTRIBUTED_RUN, "allow-commands = true", "");
    let manifest = fs::read_to_string(tree.manifest()).expect("the manifest");
    tree.write_manifest(&manifest.replace("id = \"corp\"\n", ""));
    let stderr = fails(&tree, &["sync"]);
    assert!(
        stderr.contains("run setup (from include-remote action 1 of remote `corporate`)")
            && stderr.contains("needs an `id` on include-remote action 1 of remote `corporate`"),
        "{stderr}"
    );
    assert_eq!(count(&tree, "ran"), 0);
}

#[test]
fn allow_commands_alone_requires_the_checkout_to_be_the_declared_repository() {
    let (_first, tree) = including(CONTRIBUTED_RUN, "allow-commands = true", "");
    ok(&tree, &["sync"]);
    fs::remove_file(runs_toml(&tree)).expect("the record");
    let second = BareRepo::new();
    second.publish("batfiles.toml", CONTRIBUTED_RUN, "another repository");
    let manifest = fs::read_to_string(tree.manifest()).expect("the manifest");
    let first_url = manifest
        .lines()
        .find_map(|line| line.strip_prefix("url = "))
        .expect("a url")
        .to_owned();
    tree.write_manifest(
        &manifest.replace(&first_url, &format!("\"{}\"", display(&second.origin()))),
    );
    let stderr = fails(&tree, &["sync"]);
    assert!(
        stderr.contains("its origin is a different repository")
            && stderr.contains("its allow-commands cannot apply"),
        "{stderr}"
    );
    assert_eq!(
        count(&tree, "ran"),
        1,
        "the command ran from an unverified checkout"
    );
}

#[test]
fn load_errors() {
    for (manifest, expected) in [
        (
            "[[actions]]\ntype = \"run\"\ncommand = \"true\"\ntrigger = \"once\"\n".to_owned(),
            "a `once` trigger records when the command last succeeded",
        ),
        (
            one_run("a", r#""true""#, r#"{ after = "nothing" }"#),
            "`after` names `nothing`, which no action in this manifest declares",
        ),
        (
            format!(
                "{}\n{}",
                one_run("a", r#""true""#, r#"{ after = "b" }"#),
                one_run("b", r#""true""#, r#""always""#)
            ),
            "`after` names `b`, which is action 2",
        ),
        (
            one_run("a", r#""true""#, r#"{ after = "a" }"#),
            "`after` names `a`, the action itself",
        ),
        (
            one_run("a", r#""true""#, r#"{ after = "@nowhere" }"#),
            "`after` names `@nowhere`, which this manifest does not declare",
        ),
        (
            one_run("a", r#""true""#, r#"{ after = [] }"#),
            "an empty `after` list",
        ),
        (
            one_run("a", r#""true""#, r#"{ every = "often" }"#),
            "`often` is not a valid duration",
        ),
        (
            format!(
                "{}command-timeout = \"0s\"\n",
                one_run("a", r#""true""#, r#""always""#)
            ),
            "`command-timeout` must be greater than zero",
        ),
        (
            format!(
                "{}shell = \"bash\"\n",
                one_run("a", r#""true""#, r#""always""#)
            ),
            "unknown field `shell`",
        ),
        (one_run("a", r#""echo ${""#, r#""always""#), "command"),
        (
            format!("{}cwd = \"\"\n", one_run("a", r#""true""#, r#""always""#)),
            "cwd",
        ),
        (
            format!(
                "{}env = {{ \"A=B\" = \"x\" }}\n",
                one_run("a", r#""true""#, r#""always""#)
            ),
            "is not an environment variable name",
        ),
        (
            format!(
                "{}env = {{ A = \"${{\" }}\n",
                one_run("a", r#""true""#, r#""always""#)
            ),
            "env",
        ),
    ] {
        let stderr = rejected(&manifest);
        assert!(stderr.contains(expected), "{manifest}\n{stderr}");
    }
}

#[test]
fn apply_action_waives_the_trigger_and_apply_group_honors_it() {
    let tree = leaf(&format!(
        "{}group = \"tools\"\n",
        one_run("setup", r#""echo ran >> ran""#, r#""once""#)
    ));
    ok(&tree, &["sync"]);
    let first = runs_document(&tree);

    ok(&tree, &["apply-group", "--group", "tools"]);
    assert_eq!(count(&tree, "ran"), 1);

    std::thread::sleep(std::time::Duration::from_millis(1100));
    ok(&tree, &["apply-action", "--id", "setup"]);
    assert_eq!(count(&tree, "ran"), 2);
    assert_ne!(runs_document(&tree), first, "the success was not recorded");
}

#[test]
fn a_dry_run_launches_nothing_and_records_nothing() {
    let tree = leaf(&format!(
        r#"[[actions]]
type = "create-dir"
id = "dir"
dest = "~/dir"

{}
{}
{}
{}"#,
        one_run("always", r#""echo ran >> ran""#, r#""always""#),
        one_run("boot", r#""echo ran >> ran""#, r#""bootstrap""#),
        one_run("once", r#""echo ran >> ran""#, r#""once""#),
        one_run("follow", r#""echo ran >> ran""#, r#"{ after = "dir" }"#),
    ));
    let stderr = ok(&tree, &["sync", "--dry-run", "-v"]);
    assert_eq!(count(&tree, "ran"), 0);
    assert!(!runs_toml(&tree).exists());
    for expected in [
        "would run always: echo ran >> ran",
        "not run: not a bootstrap run",
        "would run once: echo ran >> ran",
        "would run follow: echo ran >> ran",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }

    write_runs(&tree, &succeeded(&["once", "follow"], Timestamp::now()));
    let before = runs_document(&tree);
    let stderr = ok(&tree, &["sync", "--dry-run", "-v"]);
    assert_eq!(count(&tree, "ran"), 0);
    assert_eq!(runs_document(&tree), before);
    assert!(stderr.contains("not run: already ran"), "{stderr}");
    assert!(
        stderr.contains("run follow: would run if `dir` changes"),
        "{stderr}"
    );
}

#[test]
fn a_dry_run_checks_the_templates_of_a_due_after_command_only() {
    let tree = leaf(&format!(
        r#"[[actions]]
type = "create-dir"
id = "dir"
dest = "~/dir"

{}"#,
        one_run("follow", r#""echo ${missing}""#, r#"{ after = "dir" }"#)
    ));
    let stderr = fails(&tree, &["sync", "--dry-run"]);
    assert!(stderr.contains("run follow: command"), "{stderr}");

    write_runs(&tree, &succeeded(&["follow"], Timestamp::now()));
    let stderr = ok(&tree, &["sync", "--dry-run"]);
    assert!(stderr.contains("would run if `dir` changes"), "{stderr}");
}

#[test]
fn excluded_commands_do_not_run() {
    let tree = leaf(&format!(
        "{}\n{}\n{}when = \"false\"\n",
        one_run("disabled", r#""echo disabled >> ran""#, r#""always""#),
        one_run("skipped", r#""echo skipped >> ran""#, r#""always""#),
        one_run("unwanted", r#""echo unwanted >> ran""#, r#""always""#),
    ));
    ok(&tree, &["disable-action", "disabled"]);
    ok(&tree, &["sync", "--skip-action", "skipped"]);
    assert_eq!(count(&tree, "ran"), 0);
}

#[test]
fn templates_resolve_only_when_the_command_runs() {
    let tree = leaf(&format!(
        "[vars]\nword = \"hello\"\n\n{}\n{}",
        one_run(
            "greet",
            r#"["sh", "-c", "echo ${vars.word} >> ran"]"#,
            r#""always""#
        ),
        one_run("done", r#""echo ${missing}""#, r#""once""#),
    ));
    write_runs(
        &tree,
        &format!("[done]\nlast-success = \"{}\"\n", Timestamp::now()),
    );
    ok(&tree, &["sync"]);
    assert_eq!(
        fs::read_to_string(tree.path("repo/ran")).expect("ran"),
        "hello\n"
    );
}

#[test]
fn a_program_under_the_selected_home_is_reached_with_a_tilde() {
    use std::os::unix::fs::PermissionsExt;

    let tree = leaf(&one_run(
        "tool",
        r#"["~/bin/tool", "argument"]"#,
        r#""always""#,
    ));
    let program = tree.home("bin/tool");
    fs::create_dir_all(tree.home("bin")).expect("a bin directory");
    fs::write(&program, "#!/bin/sh\necho \"$1\" >> ran\n").expect("a program");
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).expect("executable");
    ok(&tree, &["sync"]);
    assert_eq!(
        fs::read_to_string(tree.path("repo/ran")).expect("ran"),
        "argument\n"
    );
}

#[test]
fn a_command_runs_where_cwd_says() {
    let tree = leaf(&format!(
        "{}cwd = \"tools\"\n\n{}cwd = \"~/src\"\n",
        one_run("relative", r#""pwd > ../relative""#, r#""always""#),
        one_run("home", r#""pwd > ../../repo/home""#, r#""always""#),
    ));
    fs::create_dir(tree.path("repo/tools")).expect("a repository directory");
    fs::create_dir(tree.home("src")).expect("a home directory");
    ok(&tree, &["sync"]);
    let read = |name: &str| {
        let text = fs::read_to_string(tree.path(&format!("repo/{name}"))).expect(name);
        text.trim_end().to_owned()
    };
    assert_eq!(read("relative"), display(&tree.path("repo/tools")));
    assert_eq!(read("home"), display(&tree.home("src")));
}

#[test]
fn a_missing_working_directory_is_a_failure_to_start() {
    let tree = leaf(&format!(
        "{}cwd = \"~/absent\"\n",
        one_run("nowhere", r#""echo ran >> ran""#, r#""always""#)
    ));
    let stderr = fails(&tree, &["sync"]);
    assert!(
        stderr.contains(&format!(
            "run nowhere: the command could not be started: its working directory {} is not a \
             directory",
            display(&tree.home("absent"))
        )),
        "{stderr}"
    );
    // A dry run does not know what earlier actions would create, so it does not look.
    ok(&tree, &["sync", "--dry-run"]);

    let manifest = fs::read_to_string(tree.manifest()).expect("the manifest");
    tree.write_manifest(&format!("{manifest}allow-failure = true\n"));
    let stderr = ok(&tree, &["sync"]);
    assert!(stderr.contains("warning: run nowhere:"), "{stderr}");
}

#[test]
fn a_dangling_working_directory_is_a_failure_to_start_that_allow_failure_excuses() {
    let tree = leaf(&format!(
        "{}cwd = \"~/dangling\"\nallow-failure = true\n\n{}",
        one_run("nowhere", r#""echo ran >> ran""#, r#""always""#),
        one_create_dir("~/after")
    ));
    std::os::unix::fs::symlink(tree.home("absent"), tree.home("dangling")).expect("a symlink");
    let stderr = ok(&tree, &["sync"]);
    assert!(
        stderr.contains(&format!(
            "warning: run nowhere: the command could not be started: its working directory {} \
             is not a directory",
            display(&tree.home("dangling"))
        )),
        "{stderr}"
    );
    assert!(tree.home("after").is_dir(), "the run continued");
    assert_eq!(count(&tree, "ran"), 0);
}

#[test]
fn env_adds_resolved_variables_to_the_inherited_environment() {
    let tree = leaf(&format!(
        "[vars]\nflavor = \"mint\"\n\n{}env = {{ BATFILES_TEST_FLAVOR = \"${{vars.flavor}}\", BATFILES_TEST_PATH = \"extra:${{env.PATH}}\" }}\n",
        one_run(
            "show",
            r#""echo $BATFILES_TEST_FLAVOR > flavor; echo $BATFILES_TEST_PATH > path; echo $PATH > inherited""#,
            r#""always""#
        ),
    ));
    ok(&tree, &["sync"]);
    let read = |name: &str| fs::read_to_string(tree.path(&format!("repo/{name}"))).expect(name);
    assert_eq!(read("flavor"), "mint\n");
    let inherited = read("inherited");
    assert_eq!(read("path"), format!("extra:{inherited}"));
    assert!(!inherited.trim().is_empty());
}

#[test]
fn a_malformed_record_fails_before_any_action() {
    let tree = leaf(&format!(
        "{}\n{}",
        one_create_dir("~/first"),
        one_run("setup", r#""echo ran >> ran""#, r#""once""#)
    ));
    write_runs(&tree, "[setup]\nruns = 3\n");
    let stderr = fails(&tree, &["sync"]);
    assert!(stderr.contains("runs.toml"), "{stderr}");
    assert!(!tree.home("first").exists());
    assert_eq!(runs_document(&tree), "[setup]\nruns = 3\n");
}
