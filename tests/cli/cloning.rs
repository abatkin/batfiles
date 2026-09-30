//! CLI tests for Git cloning, updates, destination validation, and dry runs. Use local bare
//! repositories and inspect `FETCH_HEAD` for unwanted fetches.

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

/// Return the clone's checked-out commit ID.
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
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.local/share/plugins/omz");

    tree.batfiles().arg("sync").assert().success();

    assert!(tree.home(".local/share/plugins/omz/README.md").is_file());
}

/// Report removal of a broken parent symlink before reporting the clone.
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
fn a_destination_holding_something_that_is_not_a_clone_is_named_when_skipped() {
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    let dest = tree.home(".oh-my-zsh");
    fs::create_dir_all(dest.join("custom")).expect("a directory in the way");
    fs::write(dest.join("custom/mine.zsh"), "echo mine\n").expect("someone's file");

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

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
fn a_symlink_to_a_checkout_elsewhere_is_never_fetched_into() {
    use std::os::unix::fs::symlink;

    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    let theirs = tree.home("elsewhere");
    git(
        &tree.path("home"),
        &["clone", "--", &display(&origin.origin()), "elsewhere"],
    );
    let before = head(&theirs);
    origin.publish("plugin.zsh", "echo hello\n", "second");
    symlink(&theirs, tree.home(".oh-my-zsh")).expect("a link to their checkout");

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

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
        stderr_of(&assertion).contains("it is a symlink to"),
        "{}",
        stderr_of(&assertion)
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_that_reaches_nothing_is_cleared_and_cloned_over() {
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

/// Create an external checkout one commit behind `origin`. Return its path and commit ID.
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
fn a_git_directory_that_is_a_symlink_is_not_this_clones_own() {
    // A symlinked `.git` can pass the worktree-root check while redirecting another checkout's
    // refs.
    use std::os::unix::fs::symlink;

    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    let (theirs, before) = a_checkout_elsewhere(&tree, &origin);
    let dest = tree.home(".oh-my-zsh");
    fs::create_dir(&dest).expect("the destination");
    symlink(theirs.join(".git"), dest.join(".git")).expect("a borrowed git directory");

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

    assert_eq!(head(&theirs), before, "their checkout was fast-forwarded");
    assert!(
        stderr_of(&assertion).contains("belongs to a checkout somewhere else"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_clone_whose_worktree_is_configured_elsewhere_is_not_this_clones_own() {
    // A real `.git` can still redirect writes through `core.worktree`.
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    let (theirs, before) = a_checkout_elsewhere(&tree, &origin);
    let dest = tree.home(".oh-my-zsh");
    git(
        &tree.path("home"),
        &["clone", "--", &display(&origin.origin()), ".oh-my-zsh"],
    );
    git(&dest, &["config", "core.worktree", &display(&theirs)]);

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

    assert_eq!(head(&theirs), before, "their checkout was fast-forwarded");
    assert!(
        stderr_of(&assertion).contains("belongs to a checkout somewhere else"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn an_inherited_git_dir_does_not_redirect_the_update() {
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
    // Git uses exit 128 for both missing and malformed upstreams; malformed configuration must
    // fail.
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
fn an_interrupted_clone_is_not_treated_as_finished() {
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/.oh-my-zsh");
    let dest = tree.home(".oh-my-zsh");
    fs::create_dir(&dest).expect("the destination");
    git(&dest, &["init", "-b", "main"]);

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

    assert!(
        stderr_of(&assertion).contains("incomplete or damaged clone"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn untracked_files_block_an_update_whatever_the_users_gitconfig_shows() {
    let origin = BareRepo::new();
    let tree = cloning(&origin);
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    git(&clone, &["config", "status.showUntrackedFiles", "no"]);
    fs::write(clone.join("plugin.zsh"), "mine, not theirs\n").expect("an untracked file");
    let before = head(&clone);

    // Publish a path already occupied by an untracked local file.
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
    // Git discovers parent repositories from plain subdirectories; updates must require this
    // destination's own clone.
    let origin = BareRepo::new();
    let tree = one_clone(&origin, "~/notes/plugins");
    git(&tree.path("home"), &["init", "-b", "main", "notes"]);
    fs::create_dir(tree.home("notes/plugins")).expect("a plain directory");

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

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
    // Check `FETCH_HEAD` and remote refs to catch fetches that leave the worktree unchanged.
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
    let origin = BareRepo::new();
    origin.publish_on("next", "next.zsh", "echo next\n", "on next");
    let tree = one_clone_at(&origin, "~/.oh-my-zsh", "main");
    tree.batfiles().arg("sync").assert().success();
    let clone = tree.home(".oh-my-zsh");
    assert_eq!(branch_of(&clone), "main");

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
    // Git checkout normally overwrites ignored files; batfiles must preserve them.
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
    // Fast-forward merges can overwrite ignored files without making the worktree dirty.
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
    // A rename can overwrite an ignored path just like an addition; guard both diff statuses.
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
    // Ref resolution searches every remote, including those the current branch does not track.
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
    // Full refs and revision expressions resolve to commits, but cannot name local branches.
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
