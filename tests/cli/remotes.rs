//! CLI tests for Git remote validation, materialization, and dry runs using local bare
//! repositories. File and archive remotes are covered in `fetched_remotes`.

use std::fs;

use crate::support::*;

/// A manifest declaring one Git remote at `core`, plus whatever else the case
/// needs after it.
fn declaring(origin: &BareRepo, rest: &str) -> String {
    format!(
        r#"[remotes.core]
type = "git"
url = "{}"

{rest}"#,
        display(&origin.origin())
    )
}

/// What the materialization of `core` holds at `name`.
fn materialized(tree: &Tree, name: &str) -> String {
    let path = tree.path("repo").join("remotes/core").join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", display(&path)))
}

#[test]
fn a_declared_remote_is_cloned_into_the_repository() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&declaring(&origin, &one_create_dir("~/.cache/zsh")));

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(materialized(&tree, "README.md"), "a plugin\n");
    assert!(
        tree.path("repo").join("remotes/core/.git").is_dir(),
        "the materialization is not a clone"
    );
    assert!(
        stderr_of(&assertion).contains(&format!(
            "cloned {} from {}",
            display(&tree.path("repo").join("remotes/core")),
            display(&origin.origin())
        )),
        "the clone was not reported:\n{}",
        stderr_of(&assertion)
    );
    assert!(tree.home(".cache/zsh").is_dir(), "the action did not run");
}

#[test]
fn a_later_sync_updates_the_materialization() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&declaring(&origin, ""));
    tree.batfiles().arg("sync").assert().success();

    origin.publish("PLUGINS.md", "one more\n", "second");
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(materialized(&tree, "PLUGINS.md"), "one more\n");
    assert!(
        stderr_of(&assertion).contains(&format!(
            "updated {}",
            display(&tree.path("repo").join("remotes/core"))
        )),
        "the update was not reported:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_declared_ref_is_what_the_materialization_follows() {
    let origin = BareRepo::new();
    origin.publish_on("topic", "TOPIC.md", "on the branch\n", "topic work");
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.core]
type = "git"
url = "{}"
ref = "topic"
"#,
        display(&origin.origin())
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(materialized(&tree, "TOPIC.md"), "on the branch\n");
}

#[test]
fn a_materialization_is_named_and_reported_before_the_first_action() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&declaring(&origin, &one_create_dir("~/.cache/zsh")));

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);

    let heading = stderr.find("remote core").expect("the remote heading");
    let cloned = stderr.find("cloned ").expect("the clone line");
    let action = stderr
        .find("create-dir action 1")
        .expect("the action heading");
    assert!(
        heading < cloned,
        "the heading came after its line:\n{stderr}"
    );
    assert!(
        cloned < action,
        "the remote was materialized after the actions:\n{stderr}"
    );
}

#[test]
fn a_remote_that_cannot_be_materialized_stops_the_run() {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[remotes.core]
type = "git"
url = "{}"

{}"#,
        written(&tree.path("nowhere.git")),
        one_create_dir("~/.cache/zsh")
    ));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("git clone failed"),
        "{}",
        stderr_of(&assertion)
    );
    assert!(
        !tree.home(".cache/zsh").exists(),
        "the run continued past the failed remote"
    );
}

#[test]
fn an_apply_command_materializes_nothing() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        r#"[[actions]]
type = "create-dir"
id = "cache"
dest = "~/.cache/zsh"
"#,
    ));

    tree.batfiles()
        .args(["apply-action", "--id", "cache"])
        .assert()
        .success();

    assert!(tree.home(".cache/zsh").is_dir(), "the action did not run");
    assert_eq!(
        entries(&tree.path("repo")),
        ["batfiles.toml"],
        "an apply command materialized a remote"
    );
}

#[test]
fn a_url_is_whatever_git_accepts_but_never_nothing() {
    let stderr = rejected(
        r#"[remotes.core]
type = "git"
url = ""
"#,
    );
    assert!(
        stderr.contains("remote `core`"),
        "the remote was not named:\n{stderr}"
    );
    assert!(stderr.contains("url is empty"), "{stderr}");
}

#[test]
fn a_ref_written_with_nothing_in_it_is_refused() {
    let stderr = rejected(
        r#"[remotes.core]
type = "git"
url = "https://e.example/a.git"
ref = ""
"#,
    );
    assert!(stderr.contains("remote `core`"), "{stderr}");
    assert!(stderr.contains("ref is empty"), "{stderr}");
}

#[test]
fn a_remote_writes_one_condition_or_none() {
    let stderr = rejected(
        r#"[remotes.core]
type = "git"
url = "https://e.example/a.git"
when = "work"
unless = "gui"
"#,
    );
    assert!(stderr.contains("remote `core`"), "{stderr}");
    assert!(
        stderr.contains("writes both `when` and `unless`"),
        "{stderr}"
    );
}

#[test]
fn a_remotes_condition_is_parsed_where_the_manifest_is_read() {
    let stderr = rejected(
        r#"[remotes.core]
type = "git"
url = "https://e.example/a.git"
when = "work &&"
"#,
    );
    assert!(stderr.contains("not a valid condition"), "{stderr}");
}

// Remote conditions control both materialization and access to existing content.

/// A manifest declaring `core` behind one condition, with `vars` declaring what
/// the condition reads and `rest` holding whatever actions the case needs.
fn conditioned(
    origin: &BareRepo,
    spelling: &str,
    condition: &str,
    vars: &str,
    rest: &str,
) -> String {
    format!(
        r#"{vars}[remotes.core]
type = "git"
url = "{}"
{spelling} = "{condition}"

{rest}"#,
        display(&origin.origin())
    )
}

#[test]
fn a_remote_whose_condition_closes_is_not_materialized() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&conditioned(
        &origin,
        "when",
        "work",
        "[vars]\nwork = \"false\"\n\n",
        &one_create_dir("~/.cache/zsh"),
    ));

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();

    assert_eq!(
        entries(&tree.path("repo")),
        ["batfiles.toml"],
        "an excluded remote was brought down anyway"
    );
    assert!(tree.home(".cache/zsh").is_dir(), "the run stopped");
    assert!(
        stderr_of(&assertion).contains("remote core - skipped: when \"work\" is false"),
        "the exclusion was not reported as one:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn an_excluded_remote_says_nothing_without_detail() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&conditioned(
        &origin,
        "when",
        "work",
        "[vars]\nwork = \"false\"\n\n",
        "",
    ));

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(stderr_of(&assertion), "", "the run was not quiet");
}

#[test]
fn unless_decides_a_remote_the_other_way_round() {
    let origin = BareRepo::new();

    let closed = Tree::new();
    closed.write_manifest(&conditioned(
        &origin,
        "unless",
        "work",
        "[vars]\nwork = \"true\"\n\n",
        "",
    ));
    closed.batfiles().arg("sync").assert().success();
    assert_eq!(entries(&closed.path("repo")), ["batfiles.toml"]);

    let open = Tree::new();
    open.write_manifest(&conditioned(
        &origin,
        "unless",
        "work",
        "[vars]\nwork = \"false\"\n\n",
        "",
    ));
    open.batfiles().arg("sync").assert().success();
    assert_eq!(materialized(&open, "README.md"), "a plugin\n");
}

#[test]
fn a_remotes_condition_that_cannot_be_decided_closes_it_and_warns() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&conditioned(
        &origin,
        "when",
        "nowhere",
        "",
        &one_create_dir("~/.cache/zsh"),
    ));

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);

    assert_eq!(
        entries(&tree.path("repo")),
        ["batfiles.toml"],
        "a remote batfiles could not decide about was cloned"
    );
    assert!(
        stderr.contains(
            "remote core: when \"nowhere\" cannot be evaluated, so it is not materialized"
        ),
        "the failure was not reported under the record:\n{stderr}"
    );
    assert!(stderr.contains("`nowhere` is not declared"), "{stderr}");
    assert!(tree.home(".cache/zsh").is_dir(), "the run stopped");
}

#[test]
fn a_condition_flipped_on_the_command_line_decides_the_remote() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&conditioned(
        &origin,
        "when",
        "work",
        "[vars]\nwork = \"false\"\n\n",
        "",
    ));

    tree.batfiles()
        .args(["sync", "--var", "work=true"])
        .assert()
        .success();

    assert_eq!(materialized(&tree, "README.md"), "a plugin\n");
}

#[test]
fn a_git_remote_is_closed_over_the_fields_it_accepts() {
    for (document, unknown) in [
        ("branch = \"main\"\n", "branch"),
        // A field belonging to a remote type that is not this one.
        ("archive-root = \"*\"\n", "archive-root"),
    ] {
        let stderr = rejected(&format!(
            r#"[remotes.core]
type = "git"
url = "https://e.example/a.git"
{document}"#
        ));
        assert!(
            stderr.contains(&format!("unknown field `{unknown}`")),
            "`{unknown}` was accepted:\n{stderr}"
        );
    }
}

#[test]
fn two_remote_keys_may_not_differ_only_in_case() {
    let stderr = rejected(
        r#"[remotes.core]
type = "git"
url = "https://e.example/a.git"

[remotes.Core]
type = "git"
url = "https://e.example/b.git"
"#,
    );
    assert!(
        stderr.contains("`core`") && stderr.contains("`Core`"),
        "{stderr}"
    );
    assert!(stderr.contains("differ only in case"), "{stderr}");
}

#[test]
fn a_remote_key_is_an_id() {
    for key in ["\"core.extra\"", "_hidden", "\"two words\""] {
        let stderr = rejected(&format!(
            r#"[remotes.{key}]
type = "git"
url = "https://e.example/a.git"
"#
        ));
        assert!(
            stderr.contains("is not a valid ID"),
            "`{key}` was accepted:\n{stderr}"
        );
    }
}

#[test]
fn a_remote_needs_a_type_to_be_one() {
    let stderr = rejected("[remotes.core]\nurl = \"https://e.example/a.git\"\n");
    assert!(stderr.contains("type"), "{stderr}");
}

/// Publish the corporate fixture as a local bare repository.
fn corporate() -> BareRepo {
    BareRepo::from_fixture("corporate")
}

/// What the fixture's seeded git configuration says, which several cases below
/// assert they installed unchanged.
const CORPORATE_GITCONFIG: &str = "[user]\n\temail = you@corp.example\n";

#[cfg(unix)]
#[test]
fn a_symlink_may_point_into_a_materialization() {
    let origin = corporate();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        &one_symlink("@core/files/zshrc", "~/.zshrc"),
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        link_target(&tree.home(".zshrc")),
        tree.path("repo").join("remotes/core/files/zshrc")
    );
    assert!(
        fs::read_to_string(tree.home(".zshrc"))
            .expect("the link should resolve")
            .contains("CORP_PROXY"),
        "the link resolved to something other than the remote's zshrc"
    );
}

#[cfg(unix)]
#[test]
fn both_spellings_of_a_reference_reach_the_same_file() {
    let origin = corporate();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        r#"[[actions]]
type = "symlink"
source = { remote = "core", path = "files/zshrc" }
dest = "~/.zshrc"
"#,
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        link_target(&tree.home(".zshrc")),
        tree.path("repo").join("remotes/core/files/zshrc")
    );
}

#[test]
fn a_copy_seeds_from_a_materialization() {
    let origin = corporate();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        &one_copy("@core/seed/gitconfig", "~/.gitconfig"),
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the seed should be installed"),
        CORPORATE_GITCONFIG
    );
    assert!(!tree.home(".gitconfig").is_symlink());
}

#[test]
fn a_directory_action_reads_its_children_from_a_materialization() {
    let origin = corporate();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        &one_copy_dir("@core/seed", "~/.config/seeds", false),
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        entries(&tree.home(".config/seeds")),
        ["gitconfig", "npmrc"],
        "the children came from somewhere other than the remote"
    );
}

#[test]
fn a_clone_list_may_live_in_a_materialization() {
    // Build the clone list at runtime because its URL contains a temporary path.
    let origin = corporate();
    let elsewhere = BareRepo::new();
    let plugin = elsewhere.another("plugin");
    origin.publish(
        "plugins.txt",
        &format!("{}\n", display(&plugin)),
        "add a plugin list",
    );
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        r#"[[actions]]
type = "git-clone-list"
source = "@core/plugins.txt"
dest-dir = "~/.plugins"
"#,
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".plugins/plugin/README.md")).expect("a cloned plugin"),
        "a plugin\n"
    );
}

#[test]
fn a_list_held_by_a_remote_is_named_the_way_the_manifest_wrote_it() {
    let origin = corporate();
    let tree = Tree::new();
    origin.publish(
        "plugins.txt",
        &format!("{}\n", written(&tree.path("nowhere.git"))),
        "add a list naming nothing",
    );
    tree.write_manifest(&declaring(
        &origin,
        r#"[[actions]]
type = "git-clone-list"
source = "@core/plugins.txt"
dest-dir = "~/.plugins"
"#,
    ));

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(
        stderr_of(&assertion).contains("@core/plugins.txt line 1"),
        "the list was not named as written:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_source_naming_an_undeclared_remote_is_refused_when_the_manifest_is_read() {
    let stderr = rejected(&one_symlink("@work/zshrc", "~/.zshrc"));
    for expected in ["@work/zshrc", "does not declare", "[remotes.work]"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_source_in_a_remote_this_machine_has_not_cloned_says_to_sync() {
    let origin = corporate();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        r#"[[actions]]
type = "copy"
id = "gitconfig"
source = "@core/seed/gitconfig"
dest = "~/.gitconfig"
"#,
    ));

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "gitconfig"])
        .assert()
        .failure()
        .code(1);

    for expected in ["remote `core` is not materialized", "batfiles sync"] {
        assert!(
            stderr_of(&assertion).contains(expected),
            "no `{expected}` in:\n{}",
            stderr_of(&assertion)
        );
    }
    assert!(!tree.home(".gitconfig").exists(), "the action installed");
}

#[test]
fn a_source_in_a_remote_this_machine_excludes_is_refused_by_name() {
    let origin = corporate();
    let tree = Tree::new();
    tree.write_manifest(&conditioned(
        &origin,
        "when",
        "work",
        "[vars]\nwork = \"false\"\n\n",
        &one_copy("@core/seed/gitconfig", "~/.gitconfig"),
    ));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    for expected in [
        "remote `core` is excluded on this machine",
        "when \"work\" is false",
    ] {
        assert!(
            stderr_of(&assertion).contains(expected),
            "no `{expected}` in:\n{}",
            stderr_of(&assertion)
        );
    }
    assert!(!tree.home(".gitconfig").exists(), "the action installed");
}

#[test]
fn a_materialization_left_by_an_earlier_run_is_kept_and_not_read() {
    let origin = corporate();
    let tree = Tree::new();
    tree.write_manifest(&conditioned(
        &origin,
        "when",
        "work",
        "[vars]\nwork = \"false\"\n\n",
        &one_copy("@core/seed/gitconfig", "~/.gitconfig"),
    ));

    tree.batfiles()
        .args(["sync", "--var", "work=true"])
        .assert()
        .success();
    assert_eq!(materialized(&tree, "seed/gitconfig"), CORPORATE_GITCONFIG);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().failure();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("remote core - skipped: when \"work\" is false"),
        "the remote was not reported as excluded:\n{stderr}"
    );
    assert!(
        stderr.contains("remote `core` is excluded on this machine"),
        "the stale materialization was read:\n{stderr}"
    );
    assert_eq!(
        materialized(&tree, "seed/gitconfig"),
        CORPORATE_GITCONFIG,
        "the materialization was removed"
    );
}

/// Where the materialization of `core` is, whether or not anything is there.
fn materialization(tree: &Tree) -> std::path::PathBuf {
    tree.path("repo").join("remotes/core")
}

#[test]
fn a_dry_run_materializes_nothing_where_there_is_no_materialization() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&declaring(&origin, &one_create_dir("~/.cache/zsh")));
    let before = snapshot(&tree.path("repo"));

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert_eq!(
        snapshot(&tree.path("repo")),
        before,
        "a dry run wrote into the repository"
    );
    assert!(
        stderr_of(&assertion).contains(&format!(
            "would clone {} from {}",
            display(&materialization(&tree)),
            display(&origin.origin())
        )),
        "the clone was not described:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_dry_run_over_an_existing_materialization_fetches_nothing() {
    // Check `FETCH_HEAD` and remote refs: an unchanged worktree alone would not detect a fetch.
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&declaring(&origin, ""));
    tree.batfiles().arg("sync").assert().success();
    let clone = materialization(&tree);
    fs::remove_file(clone.join(".git/FETCH_HEAD")).ok();
    let before = snapshot(&tree.path("repo").join("remotes"));

    origin.publish("PLUGINS.md", "one more\n", "second");
    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert_eq!(
        snapshot(&tree.path("repo").join("remotes")),
        before,
        "a dry run changed the materialization"
    );
    assert!(
        !clone.join(".git/FETCH_HEAD").exists(),
        "a dry run reached the network"
    );
    assert!(
        stderr_of(&assertion).contains(&format!("would update {}", display(&clone))),
        "the update was not described:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_dry_run_describes_the_materialization_as_it_stands() {
    let origin = corporate();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        &one_copy_dir("@core/seed", "~/.config/seeds", false),
    ));
    tree.batfiles().arg("sync").assert().success();
    fs::remove_dir_all(tree.home(".config/seeds")).expect("the installed seeds");

    origin.publish("seed/pypirc", "[distutils]\n", "seed the package index");
    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains(&format!(
            "would copy {}",
            display(&tree.home(".config/seeds/gitconfig"))
        )),
        "the child the materialization holds was not described:\n{stderr}"
    );
    assert!(
        !stderr.contains("pypirc"),
        "a dry run described content the materialization does not hold:\n{stderr}"
    );
}

#[test]
fn a_dry_run_refuses_a_source_in_a_remote_that_is_not_materialized() {
    let origin = corporate();
    let tree = Tree::new();
    tree.write_manifest(&declaring(
        &origin,
        &one_copy("@core/seed/gitconfig", "~/.gitconfig"),
    ));

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .failure()
        .code(1);

    for expected in ["remote `core` is not materialized", "batfiles sync"] {
        assert!(
            stderr_of(&assertion).contains(expected),
            "no `{expected}` in:\n{}",
            stderr_of(&assertion)
        );
    }
}

#[test]
fn an_excluded_remote_is_passed_over_in_both_modes() {
    let origin = BareRepo::new();
    for arguments in [&["sync", "-v"][..], &["sync", "-v", "--dry-run"][..]] {
        let tree = Tree::new();
        tree.write_manifest(&conditioned(
            &origin,
            "when",
            "work",
            "[vars]\nwork = \"false\"\n\n",
            &one_create_dir("~/.cache/zsh"),
        ));

        let assertion = tree.batfiles().args(arguments).assert().success();

        assert!(
            stderr_of(&assertion).contains("remote core - skipped: when \"work\" is false"),
            "{arguments:?} did not report the exclusion:\n{}",
            stderr_of(&assertion)
        );
        assert_eq!(
            entries(&tree.path("repo")),
            ["batfiles.toml"],
            "{arguments:?} brought an excluded remote down"
        );
    }
}

#[test]
fn a_dry_run_reads_no_more_of_an_excluded_remote_than_a_real_run_does() {
    let origin = corporate();
    let tree = Tree::new();
    tree.write_manifest(&conditioned(
        &origin,
        "when",
        "work",
        "[vars]\nwork = \"false\"\n\n",
        &one_copy("@core/seed/gitconfig", "~/.gitconfig"),
    ));
    tree.batfiles()
        .args(["sync", "--var", "work=true"])
        .assert()
        .success();

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .failure()
        .code(1);

    assert!(
        stderr_of(&assertion).contains("remote `core` is excluded on this machine"),
        "the stale materialization was read:\n{}",
        stderr_of(&assertion)
    );
}
