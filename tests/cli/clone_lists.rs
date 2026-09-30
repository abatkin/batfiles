//! CLI tests for clone-list parsing, preparation, and execution using local bare repositories.
//! Invalid-list cases fail before contacting their placeholder URLs.

use std::fs;

use crate::support::*;

/// A repository whose one action reads `plugins.txt`, with that file written as
/// given.
fn one_list(list: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "git-clone-list"
id = "plugins"
source = "plugins.txt"
dest-dir = "~/.plugins"
"#,
    );
    tree.repo_file("plugins.txt", list);
    tree
}

/// The `clonelist` fixture with its two placeholder repositories filled in.
fn clone_list(origin: &BareRepo) -> Tree {
    let tree = Tree::fixture("clonelist");
    tree.point_file_at(
        "manifests/zsh-plugins.txt",
        "plugin",
        &display(&origin.another("zsh-syntax-highlighting")),
    );
    tree.point_file_at(
        "manifests/zsh-plugins.txt",
        "prompt",
        &display(&origin.another("powerlevel10k")),
    );
    tree
}

/// Where the fixture's clones go.
fn plugins(tree: &Tree, name: &str) -> std::path::PathBuf {
    tree.home(&format!(".oh-my-zsh/custom/plugins/{name}"))
}

#[test]
fn every_repository_a_list_names_is_cloned_under_the_dest_dir() {
    let origin = BareRepo::new();
    let tree = clone_list(&origin);

    let assertion = tree.batfiles().arg("sync").assert().success();

    for name in ["zsh-syntax-highlighting", "p10k", "zsh-z"] {
        assert!(
            plugins(&tree, name).join("README.md").is_file(),
            "{name} was not cloned"
        );
    }
    assert!(
        !plugins(&tree, "work-tools").exists(),
        "the gated entry was cloned anyway"
    );
    let stderr = stderr_of(&assertion);
    let at = |name: &str| {
        stderr
            .find(&display(&plugins(&tree, name)))
            .unwrap_or_else(|| panic!("{name} is not in the report:\n{stderr}"))
    };
    assert!(at("zsh-syntax-highlighting") < at("p10k"), "{stderr}");
    assert!(at("p10k") < at("zsh-z"), "{stderr}");
}

#[test]
fn the_directory_the_clones_go_in_is_made_first() {
    let tree = one_list("");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();

    assert!(tree.home(".plugins").is_dir(), "the directory was not made");
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!("created {}", display(&tree.home(".plugins")))),
        "{stderr}"
    );
    assert!(
        stderr.contains("no repositories to clone in plugins.txt"),
        "{stderr}"
    );
}

#[test]
fn a_ref_on_an_entry_is_honored() {
    let origin = BareRepo::new();
    origin.publish_on("next", "next.zsh", "echo next\n", "on next");
    let tree = one_list(&format!("{} ref=next\n", display(&origin.origin())));

    tree.batfiles().arg("sync").assert().success();

    let clone = tree.home(".plugins/origin");
    assert!(
        clone.join("next.zsh").is_file(),
        "the entry's ref was not followed"
    );
}

#[test]
fn an_entry_carries_a_condition_of_its_own() {
    let origin = BareRepo::new();
    let tree = one_list(&format!(
        "{origin} dest-name=everywhere\n\
         {origin} dest-name=only-at-work when=\"work\" id=at-work\n\
         {origin} dest-name=not-on-windows unless=\"facts.family == 'windows'\"\n",
        origin = display(&origin.origin())
    ));
    tree.write_manifest(&format!(
        r#"[vars]
work = "false"

{}"#,
        fs::read_to_string(tree.manifest()).expect("the manifest")
    ));

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();

    assert_eq!(
        entries(&tree.home(".plugins")),
        ["everywhere", "not-on-windows"]
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "not cloning {} (id=at-work, plugins.txt line 2): when \"work\" is false",
            display(&origin.origin())
        )),
        "the closed entry should say why it was passed over:\n{stderr}"
    );
}

#[test]
fn naming_a_list_waives_its_own_condition_and_decides_its_entries_all_the_same() {
    // Applying the list waives its action condition, but preserves each entry's condition.
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(
        r#"[vars]
work = "false"

[[actions]]
type = "git-clone-list"
id = "plugins"
source = "plugins.txt"
dest-dir = "~/.plugins"
when = "work"
"#,
    );
    tree.repo_file(
        "plugins.txt",
        &format!(
            "{origin} dest-name=everywhere\n\
             {origin} dest-name=only-at-work when=\"work\"\n",
            origin = display(&origin.origin())
        ),
    );

    tree.batfiles().arg("sync").assert().success();
    assert!(!tree.home(".plugins").exists());

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "plugins", "-v"])
        .assert()
        .success();

    assert_eq!(entries(&tree.home(".plugins")), ["everywhere"]);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "not cloning {} (plugins.txt line 2): when \"work\" is false",
            display(&origin.origin())
        )),
        "the entry's own gate should still close it:\n{stderr}"
    );
}

#[test]
fn an_entry_whose_condition_cannot_be_decided_costs_that_entry_and_not_the_list() {
    let origin = BareRepo::new();
    let tree = one_list(&format!(
        "{origin} dest-name=first\n\
         {origin} dest-name=second when=\"nothing_declares_this\"\n\
         {origin} dest-name=third\n",
        origin = display(&origin.origin())
    ));

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    for expected in [
        &format!(
            "not cloning {} (plugins.txt line 2)",
            display(&origin.origin())
        ),
        "when \"nothing_declares_this\" cannot be evaluated",
        "batfiles vars set nothing_declares_this",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(entries(&tree.home(".plugins")), ["first", "third"]);
}

#[test]
fn an_entry_that_fails_costs_that_entry_and_not_the_run() {
    let origin = BareRepo::new();
    let tree = one_list(&format!(
        "{origin} dest-name=first\n\
         {missing} dest-name=gone\n\
         {origin} dest-name=last\n",
        origin = display(&origin.origin()),
        missing = display(&missing_repository(&origin)),
    ));

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(tree.home(".plugins/first/README.md").is_file());
    assert!(
        tree.home(".plugins/last/README.md").is_file(),
        "an entry after a failing one was stranded"
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "not cloning {} (plugins.txt line 2): git clone failed",
            display(&missing_repository(&origin))
        )),
        "{stderr}"
    );
}

/// A repository path beside `origin` that nothing is at, so cloning it fails.
fn missing_repository(origin: &BareRepo) -> std::path::PathBuf {
    origin.origin().with_file_name("missing.git")
}

#[test]
fn a_failing_entry_names_its_id_where_it_has_one() {
    let origin = BareRepo::new();
    let tree = one_list(&format!(
        "{} id=p10k dest-name=gone\n",
        display(&missing_repository(&origin))
    ));

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(
        stderr_of(&assertion).contains("(id=p10k, plugins.txt line 1)"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn an_entry_whose_ref_resolves_to_nothing_costs_that_entry_and_keeps_the_clone() {
    // Ref resolution fails after cloning. Retain the clone, warn again on later runs, and
    // continue other entries.
    let origin = BareRepo::new();
    let tree = one_list(&format!(
        "{origin} dest-name=first\n\
         {origin} dest-name=pinned ref=no-such-branch\n\
         {origin} dest-name=last\n",
        origin = display(&origin.origin())
    ));

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(tree.home(".plugins/first/README.md").is_file());
    assert!(
        tree.home(".plugins/last/README.md").is_file(),
        "an entry after a failing ref was stranded"
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("cannot follow `no-such-branch`"),
        "{stderr}"
    );
    assert!(stderr.contains("(plugins.txt line 2)"), "{stderr}");
    let pinned = tree.home(".plugins/pinned");
    assert!(pinned.join("README.md").is_file());
    assert!(
        !stderr.contains(&format!("cloned {}", display(&pinned))),
        "an entry that failed reported a clone: {stderr}"
    );

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(pinned.join("README.md").is_file());
    assert!(
        stderr_of(&assertion).contains("cannot follow `no-such-branch`"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_dest_dir_somebody_else_keeps_is_installed_into_and_left_alone() {
    let origin = BareRepo::new();
    let tree = one_list(&format!("{} dest-name=ours\n", display(&origin.origin())));
    fs::create_dir_all(tree.home(".plugins/theirs")).expect("a directory of their own");
    fs::write(tree.home(".plugins/theirs/mine.zsh"), "echo mine\n").expect("someone's file");

    tree.batfiles().arg("sync").assert().success();

    assert!(tree.home(".plugins/ours/README.md").is_file());
    assert_eq!(
        fs::read_to_string(tree.home(".plugins/theirs/mine.zsh")).expect("their file"),
        "echo mine\n",
        "a directory the list does not name was disturbed"
    );
}

#[test]
fn a_skipped_dest_dir_skips_every_entry() {
    let origin = BareRepo::new();
    let tree = one_list(&format!("{}\n", display(&origin.origin())));
    fs::write(tree.home(".plugins"), "not a directory\n").expect("a file in the way");

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(tree.home(".plugins")).expect("the file in the way"),
        "not a directory\n",
        "the occupied dest-dir was disturbed"
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "skipped {}: it is a regular file",
            display(&tree.home(".plugins"))
        )),
        "{stderr}"
    );
    assert!(
        !stderr.contains("clon"),
        "an entry was attempted under a dest-dir that was skipped: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn a_dest_dir_reached_through_a_symlink_is_installed_into() {
    use std::os::unix::fs::symlink;

    let origin = BareRepo::new();
    let tree = one_list(&format!("{}\n", display(&origin.origin())));
    let elsewhere = tree.home("volume/plugins");
    fs::create_dir_all(&elsewhere).expect("a directory of their own");
    symlink(&elsewhere, tree.home(".plugins")).expect("a link to it");

    tree.batfiles().arg("sync").assert().success();

    assert!(
        elsewhere.join("origin/README.md").is_file(),
        "the clone did not land at the far end of the link"
    );
    assert!(
        tree.home(".plugins").is_symlink(),
        "the link itself was disturbed"
    );
}

#[test]
fn an_action_after_one_is_carried_out() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zshrc\n");
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "git-clone-list"
source = "plugins.txt"
dest-dir = "~/.plugins"

{}"#,
        one_copy("shell/zshrc", "~/.zshrc")
    ));
    tree.repo_file("plugins.txt", &format!("{}\n", display(&origin.origin())));

    tree.batfiles().arg("sync").assert().success();

    assert!(tree.home(".plugins/origin/README.md").is_file());
    assert!(
        tree.home(".zshrc").is_file(),
        "the later action was stranded"
    );
}

#[test]
fn what_was_not_cloned_is_said_at_a_volume_quiet_does_not_hide() {
    let origin = BareRepo::new();
    let tree = one_list(&format!(
        "{} dest-name=gone\n",
        display(&missing_repository(&origin))
    ));

    let assertion = tree.batfiles().args(["sync", "--quiet"]).assert().success();

    assert!(
        stderr_of(&assertion).contains("not cloning"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_malformed_list_stops_the_run_before_the_first_action() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zshrc\n");
    tree.write_manifest(&format!(
        r#"{}
[[actions]]
type = "git-clone-list"
source = "plugins.txt"
dest-dir = "~/.plugins"
"#,
        one_symlink("shell/zshrc", "~/.zshrc")
    ));
    tree.repo_file(
        "plugins.txt",
        "https://e.example/a.git\n\
         https://e.example/b.git colour=blue\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        !tree.home(".zshrc").exists(),
        "the earlier action ran before the list was checked"
    );
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("plugins.txt"), "{stderr}");
    assert!(stderr.contains("line 2"), "{stderr}");
    assert!(stderr.contains("unknown key `colour`"), "{stderr}");
}

#[test]
fn a_list_the_repository_does_not_have_is_a_missing_source() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "git-clone-list"
source = "plugins.txt"
dest-dir = "~/.plugins"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("no such file in the repository"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_list_belonging_to_an_action_the_run_passes_over_is_not_read() {
    let tree = one_list("https://e.example/a.git colour=blue\n");

    tree.batfiles()
        .args(["sync", "--skip-action", "plugins"])
        .assert()
        .success();
}

#[test]
fn a_dry_run_says_one_line_per_entry_and_clones_none_of_them() {
    let origin = BareRepo::new();
    let tree = clone_list(&origin);
    let before = snapshot(&tree.path("home"));

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert_eq!(snapshot(&tree.path("home")), before);
    let stderr = stderr_of(&assertion);
    for name in ["zsh-syntax-highlighting", "p10k", "zsh-z"] {
        assert!(
            stderr.contains(&format!("would clone {}", display(&plugins(&tree, name)))),
            "{name} is not in the report:\n{stderr}"
        );
    }
}

#[test]
fn a_dry_run_over_clones_that_are_there_fetches_nothing() {
    // An unchanged checkout alone would not detect an unwanted fetch; check `FETCH_HEAD` too.
    let origin = BareRepo::new();
    let tree = clone_list(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clones = ["zsh-syntax-highlighting", "p10k", "zsh-z"].map(|name| plugins(&tree, name));
    for clone in &clones {
        // Absent already where the entry was cloned at a `ref`, which resolves
        // without fetching.
        fs::remove_file(clone.join(".git/FETCH_HEAD")).ok();
    }

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    for clone in &clones {
        assert!(
            !clone.join(".git/FETCH_HEAD").exists(),
            "a dry run reached the network for {}",
            display(clone)
        );
        assert!(
            stderr.contains(&format!("would update {}", display(clone))),
            "{stderr}"
        );
    }
}

/// Create a two-entry list with only the first clone installed, one commit behind its origin.
fn half_installed(first: &BareRepo, second: &BareRepo) -> Tree {
    let entry =
        |origin: &BareRepo, name| format!("{} dest-name={name}\n", display(&origin.origin()));
    let tree = one_list(&entry(first, "already"));
    tree.batfiles().arg("sync").assert().success();

    tree.repo_file(
        "plugins.txt",
        &format!("{}{}", entry(first, "already"), entry(second, "missing")),
    );
    first.publish("plugin.zsh", "echo hello\n", "second");
    tree
}

#[test]
fn a_list_clones_what_is_missing_and_updates_what_is_there() {
    let first = BareRepo::new();
    let second = BareRepo::new();
    let tree = half_installed(&first, &second);
    let already = tree.home(".plugins/already");
    let before = git(&already, &["rev-parse", "HEAD"]);

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_ne!(
        git(&already, &["rev-parse", "HEAD"]),
        before,
        "the clone that was already there was not updated"
    );
    assert_eq!(
        fs::read_to_string(already.join("plugin.zsh")).expect("the new file"),
        "echo hello\n"
    );
    let missing = tree.home(".plugins/missing");
    assert!(
        missing.join("README.md").is_file(),
        "the entry with no clone was not cloned"
    );
    let stderr = stderr_of(&assertion);
    let updated = format!("updated {}", display(&already));
    let cloned = format!(
        "cloned {} from {}",
        display(&missing),
        display(&second.origin())
    );
    assert!(stderr.contains(&updated), "{stderr}");
    assert!(stderr.contains(&cloned), "{stderr}");
    assert!(
        stderr.find(&updated) < stderr.find(&cloned),
        "the entries were not reported in list order:\n{stderr}"
    );
}

#[test]
fn a_dry_run_over_a_half_installed_list_says_both_and_runs_no_git() {
    let first = BareRepo::new();
    let second = BareRepo::new();
    let tree = half_installed(&first, &second);
    let already = tree.home(".plugins/already");
    fs::remove_file(already.join(".git/FETCH_HEAD")).ok();
    let before = snapshot(&tree.path("home"));

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert_eq!(snapshot(&tree.path("home")), before);
    assert!(
        !already.join(".git/FETCH_HEAD").exists(),
        "a dry run reached the network"
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!("would update {}", display(&already))),
        "{stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "would clone {} from {}",
            display(&tree.home(".plugins/missing")),
            display(&second.origin())
        )),
        "{stderr}"
    );
}

#[test]
fn every_fault_names_what_is_wrong_with_the_line() {
    for (list, expected) in [
        (
            "https://e.example/a.git https://e.example/b.git\n",
            "which is not `key=value` metadata",
        ),
        (
            "https://e.example/a.git when=\"work &&\"\n",
            "is not a valid condition",
        ),
        (
            "https://e.example/a.git when=\"work\" unless=\"work\"\n",
            "writes both `when` and `unless`",
        ),
        (
            "https://e.example/a.git dest-name=../elsewhere\n",
            "not one ordinary directory name",
        ),
        (
            "https://e.example/a.git ref=\"unfinished\n",
            "opens a quote that never closes",
        ),
        ("https://e.example/a.git id=ack.vim\n", "is not a valid ID"),
        ("git@host:\n", "write `dest-name=` to say what to call"),
        (
            "https://e.example/a.git\n\
             https://elsewhere.example/a.git\n",
            "which line 1 already clones into",
        ),
        (
            "https://e.example/Plugin.git\n\
             https://elsewhere.example/plugin.git\n",
            "differs only in case",
        ),
    ] {
        let tree = one_list(list);

        let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

        assert!(
            stderr_of(&assertion).contains(expected),
            "`{list}` should be refused with `{expected}`:\n{}",
            stderr_of(&assertion)
        );
    }
}

#[test]
fn the_list_is_a_repository_path_and_the_clone_directory_is_a_dest_dir() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "git-clone-list"
source = "plugins.txt"
dest = "~/.plugins"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("unknown field `dest`"),
        "{}",
        stderr_of(&assertion)
    );

    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "git-clone-list"
source = "../outside.txt"
dest-dir = "~/.plugins"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("resolves outside the repository"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_list_is_read_from_the_repository_and_not_from_the_home() {
    let tree = one_list("# no repositories yet\n");
    fs::write(
        tree.home("plugins.txt"),
        "https://e.example/b.git colour=blue\n",
    )
    .expect("a decoy in the home");

    // The home-directory decoy is invalid, so success confirms it was not read.
    tree.batfiles().arg("sync").assert().success();
}
