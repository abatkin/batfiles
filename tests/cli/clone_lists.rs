//! `git-clone-list`: what the list may say, when it is read, and what a run
//! does with the repositories it names.
//!
//! Every repository here is a local bare one (`guidance.md`, "Test
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
        "[[actions]]\n\
         type = \"git-clone-list\"\n\
         id = \"plugins\"\n\
         source = \"plugins.txt\"\n\
         dest-dir = \"~/.plugins\"\n",
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
fn an_entry_that_fails_costs_that_entry_and_not_the_run() {
    // The decision a list has to make and one repository never did. The middle
    // entry's directory is occupied by something batfiles did not put there, so
    // it cannot be cloned into -- and the entry after it is a different
    // repository, which can.
    let origin = BareRepo::new();
    let tree = one_list(&format!(
        "{origin} dest-name=first\n\
         {origin} dest-name=taken\n\
         {origin} dest-name=last\n",
        origin = display(&origin.origin())
    ));
    fs::create_dir_all(tree.home(".plugins/taken")).expect("a directory in the way");
    fs::write(tree.home(".plugins/taken/mine.zsh"), "echo mine\n").expect("someone's file");

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(tree.home(".plugins/first/README.md").is_file());
    assert!(
        tree.home(".plugins/last/README.md").is_file(),
        "an entry after a failing one was stranded"
    );
    assert_eq!(
        fs::read_to_string(tree.home(".plugins/taken/mine.zsh")).expect("the file in the way"),
        "echo mine\n",
        "the occupied destination was disturbed"
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "not cloning {} (plugins.txt line 2): ",
            display(&origin.origin())
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains("it is a directory, and not a git clone"),
        "{stderr}"
    );
}

#[test]
fn a_failing_entry_names_its_id_where_it_has_one() {
    // The name the entry answers to, beside the line a reader has to edit.
    let origin = BareRepo::new();
    let tree = one_list(&format!(
        "{} id=p10k dest-name=taken\n",
        display(&origin.origin())
    ));
    fs::create_dir_all(tree.home(".plugins/taken")).expect("a directory in the way");

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(
        stderr_of(&assertion).contains("(id=p10k, plugins.txt line 1)"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn an_action_after_one_is_carried_out() {
    // A list must not strand what follows it, whether its entries clone or not.
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zshrc\n");
    tree.write_manifest(&format!(
        "[[actions]]\n\
         type = \"git-clone-list\"\n\
         source = \"plugins.txt\"\n\
         dest-dir = \"~/.plugins\"\n\
         \n{}",
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
    let tree = one_list(&format!("{} dest-name=taken\n", display(&origin.origin())));
    fs::create_dir_all(tree.home(".plugins/taken")).expect("a directory in the way");

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
        "{}\n\
         [[actions]]\n\
         type = \"git-clone-list\"\n\
         source = \"plugins.txt\"\n\
         dest-dir = \"~/.plugins\"\n",
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
        "[[actions]]\n\
         type = \"git-clone-list\"\n\
         source = \"plugins.txt\"\n\
         dest-dir = \"~/.plugins\"\n",
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
    // The strong claim, asserted the way `cloning` asserts it: `FETCH_HEAD`
    // staying absent is what says no git ran, where an unchanged tree would
    // also pass for an implementation that fetched and declined to merge.
    let origin = BareRepo::new();
    let tree = clone_list(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = plugins(&tree, "zsh-syntax-highlighting");
    fs::remove_file(clone.join(".git/FETCH_HEAD")).ok();

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert!(
        !clone.join(".git/FETCH_HEAD").exists(),
        "a dry run reached the network"
    );
    assert!(
        stderr_of(&assertion).contains(&format!("would update {}", display(&clone))),
        "{}",
        stderr_of(&assertion)
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
            "https://e.example/a.git when=\"os == 'linux'\"\n",
            "conditions arrive at step 5.6",
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
        "[[actions]]\n\
         type = \"git-clone-list\"\n\
         source = \"plugins.txt\"\n\
         dest = \"~/.plugins\"\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("unknown field `dest`"),
        "{}",
        stderr_of(&assertion)
    );

    let tree = Tree::new();
    tree.write_manifest(
        "[[actions]]\n\
         type = \"git-clone-list\"\n\
         source = \"../outside.txt\"\n\
         dest-dir = \"~/.plugins\"\n",
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
