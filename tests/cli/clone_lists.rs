//! `git-clone-list`: what the list may say, and when it is read.
//!
//! Nothing here clones — that is step 4.5 — so what these assert is the half
//! that runs today: the list is read and checked as the repository is loaded,
//! before any action has touched the home, and a fault in it names the file and
//! the line it is on.

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

#[test]
fn a_valid_list_is_read_and_the_action_says_what_it_cannot_do_yet() {
    // The whole of what 4.4 built, end to end: the fixture's list parses, and
    // the action says it cloned nothing rather than passing over it in silence.
    // A warning and not a failure, so the run carries on -- which is why the
    // warning has to be there.
    let tree = Tree::fixture("clonelist");

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(
        !tree.home(".oh-my-zsh").exists(),
        "something was installed by an action that is not built"
    );
    assert!(
        stderr_of(&assertion).contains(&format!(
            "not cloning into {}: git-clone-list is read and checked, \
             and cloning arrives at step 4.5",
            display(&tree.home(".oh-my-zsh/custom/plugins"))
        )),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn an_action_after_one_is_carried_out() {
    // What a warning buys over a failure, and the reason the run continues: an
    // action declared after a list must not be stranded by it.
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
    tree.repo_file("plugins.txt", "https://e.example/a.git\n");

    tree.batfiles().arg("sync").assert().success();

    assert!(
        tree.home(".zshrc").is_file(),
        "the later action was stranded"
    );
}

#[test]
fn what_is_not_cloned_is_said_at_a_volume_quiet_does_not_hide() {
    // `--quiet` drops what a run did; it does not drop a warning about what it
    // did not do. This is the one line standing between a successful `sync` and
    // a user believing their plugins are installed.
    let tree = one_list("https://e.example/a.git\n");

    let assertion = tree.batfiles().args(["sync", "--quiet"]).assert().success();

    assert!(
        stderr_of(&assertion).contains("not cloning into"),
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
fn a_dry_run_reads_the_list_and_says_the_same_thing_a_real_run_does() {
    // Reading is a read, so it happens in both modes. The warning carries no
    // tense because there is nothing to put in one: this action does the same
    // nothing either way, which is what makes the two runs word for word alike
    // rather than alike but for a verb.
    let tree = one_list("https://e.example/a.git\n");
    let before = snapshot(&tree.path("home"));

    let dry = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();
    let real = tree.batfiles().arg("sync").assert().success();

    assert_eq!(snapshot(&tree.path("home")), before);
    assert!(
        stderr_of(&dry).contains("not cloning into"),
        "{}",
        stderr_of(&dry)
    );
    assert_eq!(stderr_of(&dry), stderr_of(&real));
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
    let tree = one_list("https://e.example/a.git\n");
    fs::write(
        tree.home("plugins.txt"),
        "https://e.example/b.git colour=blue\n",
    )
    .expect("a decoy in the home");

    // The decoy would be refused if it were the list that was read, so a run
    // that succeeds is the assertion.
    tree.batfiles().arg("sync").assert().success();
}
