//! `git-clone-list`: what the list may say, when it is read, and what a run
//! does with the repositories it names.
//!
//! Every repository here is a local bare one (`architecture.md`, "Test
//! environments"). The parsing tests name `e.example` and never reach the
//! action: a list with a fault in it stops the run as the repository is loaded,
//! which is half of what these assert.

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
    // The whole of the action, end to end, in the three shapes a real list is
    // written in: a name derived from the repository, a `dest-name`, and a
    // pinned entry.
    let origin = BareRepo::new();
    let tree = clone_list(&origin);

    let assertion = tree.batfiles().arg("sync").assert().success();

    for name in ["zsh-syntax-highlighting", "p10k", "zsh-z"] {
        assert!(
            plugins(&tree, name).join("README.md").is_file(),
            "{name} was not cloned"
        );
    }
    // And the fourth shape: the entry this machine's variables close.
    assert!(
        !plugins(&tree, "work-tools").exists(),
        "the gated entry was cloned anyway"
    );
    // In list order, which is what makes a list an ordered document rather than
    // a set.
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
    // Ahead of the first entry, so an empty list still leaves the directory a
    // shell is configured to read.
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
    // Per-machine plugin selection, which is what a condition on an entry is
    // for: one list, and the machine decides which of its repositories it
    // wants. The closed entry is reported where the rest of the list is.
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
    // Two gates with one name between them. `apply-action` names the record,
    // so the record's own `when` is waived -- and an entry's is not, because an
    // entry is not what was named and nothing finer than a record can be. The
    // list is read for this run either way: the entries are decided where every
    // list's are, ahead of the action.
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

    // The record's own condition, honored: a run that did not name it clones
    // nothing and never opens the list.
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
    // The second place a gate is settled, and it answers the way the first
    // does: the entry is passed over with a warning, and the entries around it
    // are cloned as they would have been. No `-v`, since a warning is printed
    // whether or not the run asked for detail.
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
    // The decision a list has to make and one repository never did. The middle
    // entry names a repository that is not there, so it cannot be cloned --
    // and the entry after it is a different repository, which can.
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
    // The name the entry answers to, beside the line a reader has to edit.
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
    // A `ref` is resolved after the fetch, so the clone is already at the
    // destination by the time the entry fails -- a whole repository at the
    // wrong ref. It is left there rather than taken away: what rule 15 forbids
    // is a later run mistaking wreckage for finished work, and this converges on
    // a warning instead, said again on every run until the list is fixed.
    //
    // On `git-clone` the same failure stops the run (`cloning`). Here it is one
    // repository out of a list of them, so the entries after it still install.
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
    // The clone the failing entry made, and the line it never got to say.
    let pinned = tree.home(".plugins/pinned");
    assert!(pinned.join("README.md").is_file());
    assert!(
        !stderr.contains(&format!("cloned {}", display(&pinned))),
        "an entry that failed reported a clone: {stderr}"
    );

    // The steady state, which is the half worth pinning: the run still
    // succeeds, the clone stays where it is, and the warning comes back.
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
    // The shape the personal repository has: a plugin directory that already
    // holds checkouts and files batfiles did not make. A `dest-dir` is a
    // container rather than a destination (`docs/repoformat.md`), so entries
    // land beside what is there and nothing is replaced.
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
    // The other half of the container rule: a `dest-dir` something else is in
    // the way of is not about one repository, so where that conflict is
    // skipped no entry is attempted rather than every one of them failing the
    // same way.
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
    // A container follows its final symlink where a destination never does
    // (`docs/repoformat.md`), and this is where that costs something: the
    // clones land in a directory somebody else made, through a link batfiles
    // did not make either. It is safe for the reason the rule gives -- nothing
    // here replaces what it finds -- and a home whose plugin directory is a link
    // onto another volume is an ordinary arrangement rather than one to refuse.
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
    // A list must not strand what follows it, whether its entries clone or not.
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
    // `--quiet` drops what a run did; it does not drop a warning about what it
    // did not do. This is the one line standing between a successful `sync` and
    // a user believing every plugin is installed.
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
    // What reading the list early buys, and the reason it is read at all
    // before the action is reached: the symlink is declared first and ordinary
    // ordering would have installed it by the time a broken line was found.
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
    // A run that will never reach an action must not be failed by it, which is
    // the rule a missing `source` already follows. Skipping the action is how a
    // user works around a list they cannot fix today.
    let tree = one_list("https://e.example/a.git colour=blue\n");

    tree.batfiles()
        .args(["sync", "--skip-action", "plugins"])
        .assert()
        .success();
}

#[test]
fn a_dry_run_says_one_line_per_entry_and_clones_none_of_them() {
    // The entries are in hand in both modes -- the list was read with the
    // repository -- so a dry run describes every one of them. It runs no git,
    // so it resolves no ref and reaches no network.
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
    // The strong claim, asserted the way `cloning` asserts it and over every
    // entry rather than one: `FETCH_HEAD` staying absent is what says no git
    // ran, where an unchanged tree would also pass for an implementation that
    // fetched and declined to merge.
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

/// A list whose first entry has been cloned and whose second has not, with the
/// first origin one commit ahead of that clone.
///
/// The state a list is in whenever a repository grows one: the list is edited by
/// pasting a URL onto the end of it, and the run after that finds everything
/// before the new line already installed.
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
    // The shape one repository could not be in. Two origins rather than one, so
    // that what tells the entries apart is where they came from as well as
    // where they landed.
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
    // Still list order, whichever of the two an entry needed.
    assert!(
        stderr.find(&updated) < stderr.find(&cloned),
        "the entries were not reported in list order:\n{stderr}"
    );
}

#[test]
fn a_dry_run_over_a_half_installed_list_says_both_and_runs_no_git() {
    // The same list under `--dry-run`: one line per entry in the tense the mode
    // dictates, from occupancy alone. Nothing is fetched for the clone that is
    // there and nothing is cloned for the entry that has none.
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
    // One case per rule the format keeps, driven through the binary so that the
    // wording a user sees is what is asserted.
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
    // The two fields answer to the two ordinary rules, and the record is closed
    // around them: `dest` is what the actions that install at a name write, and
    // this one installs into a directory.
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
    // The list is a repository file, which is what makes reading it early
    // legitimate. A file of the same name in the home is not it.
    // The repository's list declares nothing, so a run that reads it clones
    // nothing and reaches no network.
    let tree = one_list("# no repositories yet\n");
    fs::write(
        tree.home("plugins.txt"),
        "https://e.example/b.git colour=blue\n",
    )
    .expect("a decoy in the home");

    // The decoy would be refused if it were the list that was read, so a run
    // that succeeds is the assertion.
    tree.batfiles().arg("sync").assert().success();
}
