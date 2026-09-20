//! `git-clone`: what a first run clones, what a later run does to the clone it
//! finds, what it refuses, and what a dry run says instead of any of it.
//!
//! Every test clones from a local bare repository (`guidance.md`, "Test
//! environments"). The dry-run tests assert the stronger of the two available
//! claims wherever they can: not only that the tree is unchanged, but that the
//! clone's own `FETCH_HEAD` is still absent — which is what says no git ran at
//! all, the way the fetching tests read [`Server::requests`].

use std::fs;

use crate::support::*;

/// The fixture pointed at a bare repository, which is how every test here
/// starts.
fn cloning(origin: &BareRepo) -> Tree {
    let tree = Tree::fixture("cloning");
    tree.point_at_origin(origin);
    tree
}

/// A tree whose manifest declares one `git-clone` at a destination of its own,
/// for the cases that are about what occupies that destination.
fn one_clone(origin: &BareRepo, dest: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "git-clone"
source = "{}"
dest = "{dest}"
"#,
        display(&origin.origin())
    ));
    tree
}

/// What the checked-out commit at a clone is, for comparing a worktree against
/// what its origin published.
fn head(clone: &std::path::Path) -> String {
    git(clone, &["rev-parse", "HEAD"])
}

#[test]
fn a_repository_is_cloned_where_nothing_is() {
    let origin = BareRepo::new();
    let tree = cloning(&origin);

    let assertion = tree.batfiles().arg("sync").assert().success();

    let clone = tree.home(".oh-my-zsh");
    assert_eq!(
        fs::read_to_string(clone.join("README.md")).expect("the cloned file"),
        "a plugin\n"
    );
    assert!(
        clone.join(".git").exists(),
        "the clone has no git directory"
    );
    assert!(
        stderr_of(&assertion).contains(&format!(
            "cloned {} from {}",
            display(&clone),
            display(&origin.origin())
        )),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_clone_is_made_under_directories_that_are_not_there_yet() {
    // `git clone` would create these itself. Batfiles makes them first anyway,
    // because rule 13's broken-symlink clearing happens on the way and git
    // would report one as a bare EEXIST.
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.local/share/plugins/omz");

    tree.batfiles().arg("sync").assert().success();

    assert!(tree.home(".local/share/plugins/omz/README.md").is_file());
}

/// The clearing the test above makes room for, said out loud. Gated only
/// because the fixture needs a broken symlink to build; what a clone says about
/// a link it removed is not platform-specific.
#[cfg(unix)]
#[test]
fn a_broken_link_above_a_clone_is_cleared_and_the_removal_reported_first() {
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.local/share/plugins/omz");
    let nowhere = tree.path("nowhere");
    let plugins = tree.home(".local/share/plugins");
    fs::create_dir_all(tree.home(".local/share")).expect("the directories above the link");
    std::os::unix::fs::symlink(&nowhere, &plugins).expect("a broken link");

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();

    // The removal first: it is the part the user may need to act on, and it is
    // true of a path the manifest names only by cloning underneath it.
    assert_eq!(
        stderr_of(&assertion),
        format!(
            "removed a broken symlink to {} to make {}\ncloned {} from {}\n",
            display(&nowhere),
            display(&plugins),
            display(&plugins.join("omz")),
            display(&origin.origin())
        )
    );
    assert!(tree.home(".local/share/plugins/omz/README.md").is_file());
}

#[test]
fn a_second_run_fast_forwards_the_clone_it_finds() {
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    let first = head(&clone);

    origin.publish("plugin.zsh", "echo hello\n", "second");
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_ne!(head(&clone), first, "the clone was not moved forward");
    assert_eq!(
        fs::read_to_string(clone.join("plugin.zsh")).expect("the new file"),
        "echo hello\n"
    );
    assert!(
        stderr_of(&assertion).contains(&format!("updated {}", display(&clone))),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_clone_that_is_already_current_is_left_alone_and_said_so_at_v() {
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    let before = head(&clone);

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();

    assert_eq!(head(&clone), before);
    assert!(
        stderr_of(&assertion).contains(&format!("unchanged {}", display(&clone))),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_clone_with_uncommitted_changes_is_warned_about_rather_than_updated() {
    // The rule this is written for: batfiles must not disturb work in progress,
    // and a run that stopped on one would strand every action after it. So the
    // run succeeds, the clone keeps its edit, and the user is told.
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    fs::write(clone.join("README.md"), "edited by hand\n").expect("a local edit");
    let before = head(&clone);

    origin.publish("plugin.zsh", "echo hello\n", "second");
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(head(&clone), before, "a dirty clone was moved anyway");
    assert_eq!(
        fs::read_to_string(clone.join("README.md")).expect("the edited file"),
        "edited by hand\n",
        "the local edit was lost"
    );
    assert!(
        stderr_of(&assertion).contains("uncommitted changes"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_clone_holding_local_commits_is_warned_about_rather_than_reset() {
    // Clean does not mean disposable. A worktree whose history has diverged is
    // skipped, because fast-forwarding it is impossible and anything else
    // discards commits batfiles did not make.
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    fs::write(clone.join("mine.zsh"), "echo mine\n").expect("a local file");
    git(&clone, &["add", "-A"]);
    git(
        &clone,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@e.invalid",
            "commit",
            "-m",
            "mine",
        ],
    );
    let mine = head(&clone);

    origin.publish("theirs.zsh", "echo theirs\n", "second");
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(head(&clone), mine, "a local commit was discarded");
    assert!(clone.join("mine.zsh").is_file());
    assert!(
        stderr_of(&assertion).contains("commits that"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_destination_holding_something_that_is_not_a_clone_is_refused() {
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    let dest = tree.home(".oh-my-zsh");
    fs::create_dir_all(dest.join("custom")).expect("a directory in the way");
    fs::write(dest.join("custom/mine.zsh"), "echo mine\n").expect("someone's file");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        fs::read_to_string(dest.join("custom/mine.zsh")).is_ok(),
        "the occupied destination was disturbed"
    );
    assert!(
        stderr_of(&assertion).contains("it is a directory, and not a git clone"),
        "{}",
        stderr_of(&assertion)
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_to_a_checkout_elsewhere_is_refused_rather_than_fetched_into() {
    // The invariant this action is most able to break. A symlink at the
    // destination resolves to a real checkout, so following it would put
    // `current_dir` inside somebody else's repository and fast-forward that —
    // a repository batfiles never installed and has no business moving. Rule 13
    // classifies the link rather than what it reaches, and refuses.
    use std::os::unix::fs::symlink;

    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    // Somebody's own checkout, which happens to be of the same repository —
    // the case where following the link would look like it worked.
    let theirs = tree.home("elsewhere");
    git(
        &tree.path("home"),
        &["clone", "--", &display(&origin.origin()), "elsewhere"],
    );
    let before = head(&theirs);
    origin.publish("plugin.zsh", "echo hello\n", "second");
    symlink(&theirs, tree.home(".oh-my-zsh")).expect("a link to their checkout");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert_eq!(head(&theirs), before, "their checkout was fast-forwarded");
    assert!(
        !theirs.join("plugin.zsh").exists(),
        "their checkout was fetched into"
    );
    assert!(
        tree.home(".oh-my-zsh").is_symlink(),
        "the link itself was disturbed"
    );
    assert!(
        stderr_of(&assertion).contains("already exists and is a symlink"),
        "{}",
        stderr_of(&assertion)
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_that_reaches_nothing_is_cleared_and_cloned_over() {
    // The other half of rule 13: a broken link holds no content and gives
    // access to none, so it is batfiles' to clear wherever it turns up. Handing
    // it to git as a working directory instead is what the classification above
    // prevents.
    use std::os::unix::fs::symlink;

    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    symlink(tree.home("gone"), tree.home(".oh-my-zsh")).expect("a broken link");

    let assertion = tree.batfiles().arg("sync").assert().success();

    let clone = tree.home(".oh-my-zsh");
    assert!(!clone.is_symlink(), "the link is still there");
    assert_eq!(
        fs::read_to_string(clone.join("README.md")).expect("the cloned file"),
        "a plugin\n"
    );
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("removed the symlink"), "{stderr}");
    assert!(stderr.contains("cloned"), "{stderr}");
}

/// A clone of `origin` made outside the tree batfiles manages, published one
/// commit behind the origin, for the tests about reaching somebody else's
/// checkout. Returns it and the commit it is sitting on.
fn a_checkout_elsewhere(tree: &Tree, origin: &BareRepo) -> (std::path::PathBuf, String) {
    let theirs = tree.home("elsewhere");
    git(
        &tree.path("home"),
        &["clone", "--", &display(&origin.origin()), "elsewhere"],
    );
    let at = head(&theirs);
    // So that a fast-forward, if one happened, would visibly move them.
    origin.publish("plugin.zsh", "echo hello\n", "second");
    (theirs, at)
}

#[cfg(unix)]
#[test]
fn a_git_directory_that_is_a_symlink_is_refused() {
    // The nastiest shape, and neither half of the check finds it alone. A `.git`
    // symlinked to another checkout's git directory leaves git using *that*
    // repository's refs while treating this destination as the worktree -- so
    // `rev-parse --show-toplevel` answers with this path and agrees, and a
    // fetch and fast-forward move the other checkout's branch. Only the
    // filesystem can say the git directory is not this destination's own.
    use std::os::unix::fs::symlink;

    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    let (theirs, before) = a_checkout_elsewhere(&tree, &origin);
    let dest = tree.home(".oh-my-zsh");
    fs::create_dir(&dest).expect("the destination");
    symlink(theirs.join(".git"), dest.join(".git")).expect("a borrowed git directory");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert_eq!(head(&theirs), before, "their checkout was fast-forwarded");
    assert!(
        stderr_of(&assertion).contains("belongs to a checkout somewhere else"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_clone_whose_worktree_is_configured_elsewhere_is_refused() {
    // The other half, which no filesystem check can see: a real `.git`
    // directory whose config points every git command at a different worktree.
    // This is the one `rev-parse --show-toplevel` catches.
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    let (theirs, before) = a_checkout_elsewhere(&tree, &origin);
    let dest = tree.home(".oh-my-zsh");
    git(
        &tree.path("home"),
        &["clone", "--", &display(&origin.origin()), ".oh-my-zsh"],
    );
    git(&dest, &["config", "core.worktree", &display(&theirs)]);

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert_eq!(head(&theirs), before, "their checkout was fast-forwarded");
    assert!(
        stderr_of(&assertion).contains("belongs to a checkout somewhere else"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn an_inherited_git_dir_does_not_redirect_the_update() {
    // Nothing on disk is wrong here: the destination's `.git` is a real
    // directory and git agrees the worktree is the destination. The redirect is
    // in the environment, so no check of the destination can see it -- with
    // `GIT_DIR` set, fetch and merge move the refs of whatever it names while
    // the destination stands still. batfiles clears the family instead.
    //
    // Not a contrived setting to inherit: a git hook, an editor plugin, and
    // `git rebase --exec` all export it to whatever they run.
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    let (theirs, theirs_before) = a_checkout_elsewhere(&tree, &origin);
    let before = head(&clone);

    tree.batfiles()
        .arg("sync")
        .env("GIT_DIR", theirs.join(".git"))
        .assert()
        .success();

    assert_eq!(
        head(&theirs),
        theirs_before,
        "the inherited GIT_DIR was fetched into and fast-forwarded"
    );
    assert_ne!(head(&clone), before, "the declared clone was not updated");
    assert_eq!(
        fs::read_to_string(clone.join("plugin.zsh")).expect("the new file"),
        "echo hello\n"
    );
}

#[test]
fn an_upstream_that_cannot_be_resolved_fails_rather_than_reading_as_untracked() {
    // Git exits 128 both for a branch with no upstream and for one whose
    // `branch.<name>.merge` names something unusable, so reading that failure
    // as "tracks nothing" would report a broken repository as an ordinary skip
    // and let the run succeed. The absences are asked of commands that spell
    // theirs as exit 1 instead, leaving this one an error.
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    git(&clone, &["config", "branch.main.merge", "not a ref"]);

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    let stderr = stderr_of(&assertion);
    assert!(!stderr.contains("tracks a remote"), "{stderr}");
    assert!(stderr.contains("git rev-parse failed"), "{stderr}");
}

#[test]
fn a_detached_head_is_a_skip_and_not_a_failure() {
    // The absence the check above must not turn into an error: nothing is
    // wrong with this checkout, it simply follows no branch.
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    git(&clone, &["checkout", "--detach", "--quiet"]);

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(
        stderr_of(&assertion).contains("not on a branch that tracks a remote"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_branch_with_no_upstream_configured_is_a_skip_and_not_a_failure() {
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    git(&clone, &["config", "--unset", "branch.main.merge"]);

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(
        stderr_of(&assertion).contains("not on a branch that tracks a remote"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn an_interrupted_clone_is_refused_rather_than_treated_as_finished() {
    // A clone writes straight into its destination, so this is the state rule
    // 15 is really about: a `.git` with nothing checked out. A tool that read
    // "something is there" as "already installed" would report success over it
    // on every run from then on.
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    let dest = tree.home(".oh-my-zsh");
    fs::create_dir(&dest).expect("the destination");
    git(&dest, &["init", "-b", "main"]);

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("incomplete or damaged clone"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn untracked_files_block_an_update_whatever_the_users_gitconfig_shows() {
    // `status.showUntrackedFiles = no` is a display preference, and this is a
    // safety decision. Batfiles asks for the untracked half explicitly so the
    // two cannot be coupled: an update that fast-forwarded here could overwrite
    // a file the user had put in the checkout by hand.
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    git(&clone, &["config", "status.showUntrackedFiles", "no"]);
    fs::write(clone.join("plugin.zsh"), "mine, not theirs\n").expect("an untracked file");
    let before = head(&clone);

    // The same path the upstream is about to publish, so a fast-forward would
    // be the thing that destroys it.
    origin.publish("plugin.zsh", "echo hello\n", "second");
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(head(&clone), before, "an untracked file did not block");
    assert_eq!(
        fs::read_to_string(clone.join("plugin.zsh")).expect("the untracked file"),
        "mine, not theirs\n",
        "the untracked file was overwritten"
    );
    assert!(
        stderr_of(&assertion).contains("uncommitted changes"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_directory_inside_a_repository_is_not_mistaken_for_a_clone_of_its_own() {
    // The reason a `.git` at the destination is what decides rather than
    // `rev-parse`: standing in a plain directory that happens to sit inside
    // somebody's repository, git reports a worktree, and an update on that
    // answer would fetch into and fast-forward *that* repository.
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/notes/plugins");
    git(&tree.path("home"), &["init", "-b", "main", "notes"]);
    fs::create_dir(tree.home("notes/plugins")).expect("a plain directory");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("it is a directory, and not a git clone"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_dry_run_clones_nothing_and_runs_no_git() {
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    let before = snapshot(&tree.path("home"));

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert_eq!(snapshot(&tree.path("home")), before);
    assert!(!tree.home(".oh-my-zsh").exists());
    assert!(
        stderr_of(&assertion).contains(&format!(
            "would clone {} from {}",
            display(&tree.home(".oh-my-zsh")),
            display(&origin.origin())
        )),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_dry_run_over_an_existing_clone_fetches_nothing() {
    // The case worth writing carefully. A tree snapshot alone would pass for an
    // implementation that fetched and then declined to merge, so this asserts
    // the thing that only a fetch produces: `FETCH_HEAD` is still absent, and
    // the clone has not learned about the commit its origin published.
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    fs::remove_file(clone.join(".git/FETCH_HEAD")).ok();
    let before = head(&clone);

    origin.publish("plugin.zsh", "echo hello\n", "second");
    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert_eq!(head(&clone), before, "a dry run moved the clone");
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
fn a_dry_run_says_it_would_update_whatever_occupies_the_destination() {
    // A dry run runs no git, so it cannot tell a healthy clone from a directory
    // that merely occupies the path — it reports from occupancy alone, and the
    // real run above is where the refusal happens. That is "intent, not
    // success" rather than a partial plan (`guidance.md`).
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    fs::create_dir(tree.home(".oh-my-zsh")).expect("a directory in the way");

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert!(
        stderr_of(&assertion).contains("would update"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_git_clone_source_is_not_read_as_a_repository_path_or_a_url() {
    // Neither rule the other action types answer to applies: a source here is
    // whatever git accepts, including the plain directory every test above
    // clones from. Only an empty one is refused as written.
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "git-clone"
source = ""
dest = "~/.oh-my-zsh"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("source is empty"),
        "{}",
        stderr_of(&assertion)
    );
}

/// A tree whose one `git-clone` declares a `ref`.
fn one_clone_at(origin: &BareRepo, dest: &str, git_ref: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "git-clone"
source = "{}"
dest = "{dest}"
ref = "{git_ref}"
"#,
        display(&origin.origin())
    ));
    tree
}

/// What a checkout is on: a branch by name, or `HEAD` where it is detached.
fn branch_of(clone: &std::path::Path) -> String {
    git(clone, &["rev-parse", "--abbrev-ref", "HEAD"])
}

#[test]
fn a_ref_names_the_branch_a_clone_is_put_on() {
    // What the field is for, and the reason it cannot be resolved by handing
    // the string to `rev-parse`: `next` has to mean the branch the origin
    // publishes, not the local one a clone happens to make.
    let origin = BareRepo::new();
    origin.publish_on("next", "next.zsh", "echo next\n", "on next");
    let tree = one_clone_at(&origin, "~/.oh-my-zsh", "next");

    let assertion = tree.batfiles().arg("sync").assert().success();

    let clone = tree.home(".oh-my-zsh");
    assert!(
        clone.join("next.zsh").is_file(),
        "the declared branch was not checked out"
    );
    assert_eq!(branch_of(&clone), "next");
    let stderr = stderr_of(&assertion);
    // One act, one line: a clone that was never anywhere else has nothing to
    // have switched from.
    assert!(
        stderr.contains(&format!(
            "cloned {} from {} at next",
            display(&clone),
            display(&origin.origin())
        )),
        "{stderr}"
    );
    assert!(!stderr.contains("switched"), "{stderr}");
}

#[test]
fn a_declared_branch_is_fast_forwarded_by_a_later_run() {
    // The steady state has to hold: a run after the first finds the clone on
    // the branch, brings what upstream published, and says so as an update.
    let origin = BareRepo::new();
    origin.publish_on("next", "next.zsh", "echo next\n", "on next");
    let tree = one_clone_at(&origin, "~/.oh-my-zsh", "next");
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    let first = head(&clone);

    origin.publish_on("next", "more.zsh", "echo more\n", "more on next");
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_ne!(head(&clone), first, "the declared branch was not advanced");
    assert_eq!(branch_of(&clone), "next");
    assert!(clone.join("more.zsh").is_file());
    assert!(
        stderr_of(&assertion).contains(&format!("updated {}", display(&clone))),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_ref_that_is_a_tag_pins_the_clone_and_keeps_it_there() {
    // A tag is not a branch, so the checkout is detached — and detached at the
    // right object is the steady state rather than something to correct, which
    // is what keeps a later run from dragging the pin forward.
    let origin = BareRepo::new();
    origin.tag("v1");
    origin.publish("after.zsh", "echo after\n", "after the tag");
    let tree = one_clone_at(&origin, "~/.oh-my-zsh", "v1");

    tree.batfiles().arg("sync").assert().success();

    let clone = tree.home(".oh-my-zsh");
    let pinned = head(&clone);
    assert_eq!(branch_of(&clone), "HEAD", "the clone is not detached");
    assert!(
        !clone.join("after.zsh").exists(),
        "the pin took a commit published after the tag"
    );

    origin.publish("later.zsh", "echo later\n", "later still");
    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();

    assert_eq!(head(&clone), pinned, "a pinned clone was moved");
    assert!(
        stderr_of(&assertion).contains(&format!("unchanged {}", display(&clone))),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_clone_switches_when_the_record_names_a_different_ref() {
    // The one case `switched` exists for: the repository is where it was asked
    // to be, and getting there changed the checkout rather than advancing it.
    let origin = BareRepo::new();
    origin.publish_on("next", "next.zsh", "echo next\n", "on next");
    let tree = one_clone_at(&origin, "~/.oh-my-zsh", "main");
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    assert_eq!(branch_of(&clone), "main");

    // The same repository and the same home, with the record's `ref` edited:
    // the case a user creates by changing their manifest.
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "git-clone"
source = "{}"
dest = "~/.oh-my-zsh"
ref = "next"
"#,
        display(&origin.origin())
    ));
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(branch_of(&clone), "next");
    assert!(clone.join("next.zsh").is_file());
    assert!(
        stderr_of(&assertion).contains(&format!("switched {} to next", display(&clone))),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_ref_switch_does_not_overwrite_a_file_the_clone_ignores() {
    // Git protects an untracked file from a checkout and an ignored one it does
    // not, on the reasoning that ignored content is build output. batfiles did
    // not create it either way, so it is not batfiles' to replace (rule 13):
    // here it is somebody's local notes at a path the branch being switched to
    // happens to track.
    let origin = BareRepo::new();
    origin.publish(".gitignore", "notes.local\n", "ignore it");
    origin.publish_ignored("next", "notes.local", "upstream version\n");
    let tree = one_clone_at(&origin, "~/.oh-my-zsh", "main");
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    fs::write(clone.join("notes.local"), "my own notes\n").expect("an ignored file of their own");

    tree.write_manifest(&format!(
        r#"[[actions]]
type = "git-clone"
source = "{}"
dest = "~/.oh-my-zsh"
ref = "next"
"#,
        display(&origin.origin())
    ));
    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert_eq!(
        fs::read_to_string(clone.join("notes.local")).expect("the ignored file"),
        "my own notes\n",
        "an ignored file was overwritten by a ref switch"
    );
    assert!(
        stderr_of(&assertion).contains("notes.local"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_fast_forward_stops_at_a_file_the_clone_keeps_and_upstream_starts_tracking() {
    // The same data, the same rule, and a hole `--no-overwrite-ignore` does not
    // cover: `git merge --ff-only` replaces an ignored file without a word, and
    // the dirty check cannot see one because ignored output deliberately does
    // not block an update. So the collision is asked about directly, and the
    // clone is left alone with a warning rather than the run being failed --
    // the terms every other conservative skip already has.
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    origin.publish(".gitignore", "notes.local\n", "ignore it");
    tree.batfiles().arg("sync").assert().success();
    fs::write(clone.join("notes.local"), "my own notes\n").expect("an ignored file of their own");
    let before = head(&clone);

    origin.publish_ignored("main", "notes.local", "upstream version\n");
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(clone.join("notes.local")).expect("the ignored file"),
        "my own notes\n",
        "an ignored file was overwritten by a fast-forward"
    );
    assert_eq!(head(&clone), before, "the clone was moved over it");
    assert!(
        stderr_of(&assertion).contains("it has a file of its own at notes.local"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_fast_forward_stops_at_a_file_upstream_renames_onto() {
    // The same collision, arriving in the shape a diff describes differently:
    // git records no rename and infers one from content, so a file moved onto
    // the ignored path is an `R` rather than an `A` while the merge writes it
    // exactly as it would an addition. A guard that filtered on additions alone
    // would pass over this and lose the file.
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    origin.publish(".gitignore", "notes.local\n", "ignore it");
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    fs::write(clone.join("notes.local"), "my own notes\n").expect("an ignored file of their own");
    let before = head(&clone);

    origin.publish_renamed("README.md", "notes.local");
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(clone.join("notes.local")).expect("the ignored file"),
        "my own notes\n",
        "an ignored file was overwritten by a renaming fast-forward"
    );
    assert_eq!(head(&clone), before, "the clone was moved over it");
    assert!(
        stderr_of(&assertion).contains("it has a file of its own at notes.local"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_ref_is_followed_on_whichever_remote_publishes_it() {
    // `resolve` searches every remote, so every remote has to be current: a
    // bare `git fetch` brings the one the *current branch* tracks, which would
    // leave a branch published on a second remote either invisible or stale.
    let origin = BareRepo::new();
    let elsewhere = BareRepo::new();
    elsewhere.publish_on("next", "next.zsh", "echo next\n", "on next");
    let tree = one_clone_at(&origin, "~/.oh-my-zsh", "main");
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    git(
        &clone,
        &["remote", "add", "other", &display(&elsewhere.origin())],
    );

    tree.write_manifest(&format!(
        r#"[[actions]]
type = "git-clone"
source = "{}"
dest = "~/.oh-my-zsh"
ref = "next"
"#,
        display(&origin.origin())
    ));
    tree.batfiles().arg("sync").assert().success();

    assert_eq!(branch_of(&clone), "next");
    assert!(
        clone.join("next.zsh").is_file(),
        "the second remote's branch was not fetched"
    );
}

#[test]
fn a_ref_that_is_not_a_branch_name_goes_down_the_detached_path() {
    // A `ref` is a string the manifest wrote, so it can be a revision
    // expression. `refs/remotes/origin/main~1` is something `rev-parse`
    // evaluates and `refs/remotes/origin/HEAD` is a real ref in any clone, so
    // both would enter the branch case and then fail making a local branch by
    // that name. Neither is a branch name, and both resolve to a commit.
    let origin = BareRepo::new();
    origin.publish("second.zsh", "echo second\n", "second");
    let tree = one_clone_at(&origin, "~/.oh-my-zsh", "main~1");

    tree.batfiles().arg("sync").assert().success();

    let clone = tree.home(".oh-my-zsh");
    assert_eq!(branch_of(&clone), "HEAD", "the clone is not detached");
    assert!(
        !clone.join("second.zsh").exists(),
        "`main~1` was not read as the commit before the tip"
    );
}

#[test]
fn a_ref_that_resolves_to_nothing_fails_and_says_which_one() {
    // A repository bug rather than a state of the checkout, so it is a failure
    // where the update rules are warnings.
    let origin = BareRepo::new();
    let tree = one_clone_at(&origin, "~/.oh-my-zsh", "no-such-branch");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("cannot follow `no-such-branch`"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn an_empty_ref_is_refused_as_the_manifest_is_read() {
    // Read as an absent one it would silently mean "follow whatever branch the
    // clone is on", which is not what a record asking for a ref meant.
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "git-clone"
source = "https://e.example/a.git"
dest = "~/.oh-my-zsh"
ref = ""
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert!(
        stderr_of(&assertion).contains("ref is empty"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_dry_run_names_the_ref_it_would_clone_at_without_resolving_one() {
    // The ref on the line is the declared string, which is knowable without
    // git; a dry run runs none, so nothing here was resolved.
    let origin = BareRepo::new();
    let tree = one_clone_at(&origin, "~/.oh-my-zsh", "no-such-branch");
    let before = snapshot(&tree.path("home"));

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert_eq!(snapshot(&tree.path("home")), before);
    assert!(
        stderr_of(&assertion).contains(&format!(
            "would clone {} from {} at no-such-branch",
            display(&tree.home(".oh-my-zsh")),
            display(&origin.origin())
        )),
        "{}",
        stderr_of(&assertion)
    );
}
