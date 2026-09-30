//! Unix CLI tests for destination backups, skipped conflicts, and interactive replacement.

use std::fs;
use std::os::unix::fs::PermissionsExt;

use crate::support::*;

/// A tree whose manifest links `shell/zshrc` to `~/.zshrc`, with a private file
/// of the user's already at the destination.
fn occupied_link() -> Tree {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# ours\n");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));
    let theirs = tree.home(".zshrc");
    fs::write(&theirs, "# theirs\n").expect("a file in the way");
    fs::set_permissions(&theirs, fs::Permissions::from_mode(0o600)).expect("a private mode");
    tree
}

/// `sync --interactive`, answering its questions with `answers`.
fn interactive(tree: &Tree, answers: &str) -> assert_cmd::assert::Assert {
    tree.batfiles()
        .args(["sync", "--interactive"])
        .write_stdin(answers)
        .assert()
}

fn assert_linked(tree: &Tree) {
    assert_eq!(
        link_target(&tree.home(".zshrc")),
        tree.path("repo").join("shell/zshrc")
    );
}

fn assert_left(tree: &Tree) {
    assert_eq!(
        fs::read_to_string(tree.home(".zshrc")).expect("their file"),
        "# theirs\n"
    );
}

#[test]
fn a_node_in_the_way_is_backed_up_beside_itself_and_replaced() {
    let tree = occupied_link();

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_linked(&tree);
    let backup = backup_of(&tree.home(".zshrc"));
    assert_eq!(
        fs::read_to_string(&backup).expect("the backup"),
        "# theirs\n"
    );
    assert_eq!(
        fs::metadata(&backup)
            .expect("the backup")
            .permissions()
            .mode()
            & 0o777,
        0o600,
        "the backup is readable by more than it was"
    );
    let name = backup.file_name().expect("a name").to_string_lossy();
    let stamp = name
        .strip_prefix(".zshrc.batfiles-backup-")
        .expect("the backup suffix");
    assert!(
        stamp.len() == 16 && stamp.ends_with('Z') && stamp.as_bytes()[8] == b'T',
        "`{stamp}` is not a UTC timestamp"
    );
    let stderr = stderr_of(&assertion);
    for expected in [
        format!(
            "backed up {} to {}",
            display(&tree.home(".zshrc")),
            display(&backup)
        ),
        format!("linked {}", display(&tree.home(".zshrc"))),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_directory_in_the_way_is_backed_up_whole() {
    let tree = Tree::new();
    tree.repo_file("nvim/init.lua", "-- ours\n");
    tree.write_manifest(&one_symlink("nvim", "~/.config/nvim"));
    fs::create_dir_all(tree.home(".config/nvim/lua")).expect("their directory");
    fs::write(tree.home(".config/nvim/lua/theirs.lua"), "-- theirs\n").expect("their file");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        link_target(&tree.home(".config/nvim")),
        tree.path("repo").join("nvim")
    );
    let backup = backup_of(&tree.home(".config/nvim"));
    assert_eq!(
        fs::read_to_string(backup.join("lua/theirs.lua")).expect("their file"),
        "-- theirs\n"
    );
}

#[test]
fn a_second_backup_never_takes_the_first() {
    let tree = occupied_link();
    tree.batfiles().arg("sync").assert().success();
    fs::remove_file(tree.home(".zshrc")).expect("the link");
    fs::write(tree.home(".zshrc"), "# theirs again\n").expect("another file in the way");

    tree.batfiles().arg("sync").assert().success();

    let contents: Vec<String> = backups_of(&tree.home(".zshrc"))
        .iter()
        .map(|backup| fs::read_to_string(backup).expect("a backup"))
        .collect();
    assert_eq!(contents.len(), 2, "{contents:?}");
    assert!(contents.contains(&"# theirs\n".to_owned()), "{contents:?}");
    assert!(
        contents.contains(&"# theirs again\n".to_owned()),
        "{contents:?}"
    );
}

#[test]
fn a_dry_run_says_what_it_would_back_up_and_changes_nothing() {
    let tree = occupied_link();
    let before = snapshot(&tree.path("home"));

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert_eq!(snapshot(&tree.path("home")), before);
    let stderr = stderr_of(&assertion);
    for expected in [
        format!(
            "would back up {} to {}",
            display(&tree.home(".zshrc")),
            display(&tree.home(".zshrc.batfiles-backup-"))
        ),
        format!("would link {}", display(&tree.home(".zshrc"))),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn no_overwrite_skips_the_node_and_the_run_goes_on() {
    let tree = occupied_link();
    tree.repo_file("shell/bashrc", "# bash\n");
    tree.write_manifest(&format!(
        "{}\n{}",
        one_symlink("shell/zshrc", "~/.zshrc"),
        one_symlink("shell/bashrc", "~/.bashrc")
    ));

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

    assert_left(&tree);
    assert!(backups_of(&tree.home(".zshrc")).is_empty());
    assert!(
        tree.home(".bashrc").is_symlink(),
        "the action after was not run"
    );
    let expected = format!(
        "skipped {}: it is a regular file",
        display(&tree.home(".zshrc"))
    );
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
}

#[test]
fn quiet_hides_a_skip() {
    let tree = occupied_link();

    tree.batfiles()
        .args(["sync", "--no-overwrite", "--quiet"])
        .assert()
        .success()
        .stderr("");
}

#[test]
fn interactive_backs_up_on_an_empty_answer() {
    let tree = occupied_link();

    let assertion = interactive(&tree, "\n").success();

    assert_linked(&tree);
    assert_eq!(
        fs::read_to_string(backup_of(&tree.home(".zshrc"))).expect("the backup"),
        "# theirs\n"
    );
    let expected = format!(
        "{} is a regular file: back up and replace (b), overwrite (o), or skip (s)? [b]",
        display(&tree.home(".zshrc"))
    );
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
}

#[test]
fn interactive_overwrites_without_a_backup_when_told_to() {
    let tree = occupied_link();

    let assertion = interactive(&tree, "o\n").success();

    assert_linked(&tree);
    assert!(backups_of(&tree.home(".zshrc")).is_empty());
    assert!(
        !tree.home(".zshrc.batfiles-old").exists(),
        "what was discarded is still there"
    );
    let expected = format!("discarded {}", display(&tree.home(".zshrc")));
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
}

#[test]
fn interactive_skips_when_told_to() {
    let tree = occupied_link();

    interactive(&tree, "s\n").success();

    assert_left(&tree);
    assert!(backups_of(&tree.home(".zshrc")).is_empty());
}

#[test]
fn interactive_asks_again_until_it_understands() {
    let tree = occupied_link();

    let assertion = interactive(&tree, "maybe\nS\n").success();

    assert_left(&tree);
    let stderr = stderr_of(&assertion);
    assert_eq!(
        stderr.matches("back up and replace (b)").count(),
        2,
        "{stderr}"
    );
    assert!(stderr.contains("answer b, o, or s"), "{stderr}");
}

#[test]
fn interactive_asks_even_under_quiet() {
    let tree = occupied_link();

    let assertion = tree
        .batfiles()
        .args(["sync", "--interactive", "--quiet"])
        .write_stdin("s\n")
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("back up and replace (b)"), "{stderr}");
}

#[test]
fn interactive_with_no_answer_fails_and_changes_nothing() {
    let tree = occupied_link();

    let assertion = interactive(&tree, "").failure().code(1);

    assert_left(&tree);
    assert!(backups_of(&tree.home(".zshrc")).is_empty());
    let expected = format!(
        "no answer about {}: standard input ended",
        display(&tree.home(".zshrc"))
    );
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
}

#[test]
fn interactive_is_refused_beside_a_dry_run() {
    let tree = occupied_link();

    tree.batfiles()
        .args(["sync", "--interactive", "--dry-run"])
        .assert()
        .failure()
        .code(2);

    assert_left(&tree);
}

#[test]
fn a_file_in_the_way_of_a_destination_directory_is_backed_up() {
    let tree = Tree::new();
    tree.repo_file("files/ackrc", "# ack\n");
    tree.write_manifest(&one_symlink_dir("files", "~/bin", false));
    fs::write(tree.home("bin"), "mine\n").expect("a file in the way");

    tree.batfiles().arg("sync").assert().success();

    assert!(tree.home("bin/ackrc").is_symlink());
    assert_eq!(
        fs::read_to_string(backup_of(&tree.home("bin"))).expect("the backup"),
        "mine\n"
    );
}

#[test]
fn a_file_in_the_way_of_a_parent_is_backed_up() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.config/zsh/zshrc"));
    fs::write(tree.home(".config"), "mine\n").expect("a file in the way");

    tree.batfiles().arg("sync").assert().success();

    assert!(tree.home(".config/zsh/zshrc").is_symlink());
    assert_eq!(
        fs::read_to_string(backup_of(&tree.home(".config"))).expect("the backup"),
        "mine\n"
    );
}

/// A tree whose manifest clones `origin` to `~/.oh-my-zsh`.
fn one_clone(origin: &BareRepo) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "git-clone"
source = "{}"
dest = "~/.oh-my-zsh"
"#,
        display(&origin.origin())
    ));
    tree
}

#[test]
fn a_directory_that_is_not_a_clone_is_backed_up_and_cloned_over() {
    let origin = BareRepo::new();
    let tree = one_clone(&origin);
    let dest = tree.home(".oh-my-zsh");
    fs::create_dir_all(dest.join("custom")).expect("a directory in the way");
    fs::write(dest.join("custom/mine.zsh"), "echo mine\n").expect("their file");

    tree.batfiles().arg("sync").assert().success();

    assert!(dest.join("README.md").is_file(), "nothing was cloned");
    assert_eq!(
        fs::read_to_string(backup_of(&dest).join("custom/mine.zsh")).expect("their file"),
        "echo mine\n"
    );
}

#[test]
fn a_file_at_a_clone_destination_is_backed_up_and_cloned_over() {
    let origin = BareRepo::new();
    let tree = one_clone(&origin);
    let dest = tree.home(".oh-my-zsh");
    fs::write(&dest, "mine\n").expect("a file in the way");

    tree.batfiles().arg("sync").assert().success();

    assert!(dest.join("README.md").is_file(), "nothing was cloned");
    assert_eq!(
        fs::read_to_string(backup_of(&dest)).expect("the backup"),
        "mine\n"
    );
}

#[test]
fn a_failed_clone_puts_what_was_there_back() {
    let origin = BareRepo::new();
    let tree = one_clone(&origin);
    fs::remove_dir_all(origin.origin()).expect("an origin that is gone");
    let dest = tree.home(".oh-my-zsh");
    fs::write(&dest, "mine\n").expect("a file in the way");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);

    assert_eq!(fs::read_to_string(&dest).expect("their file"), "mine\n");
    assert!(backups_of(&dest).is_empty());
    let stderr = stderr_of(&assertion);
    for expected in [
        format!("backed up {}", display(&dest)),
        format!("restored {}", display(&dest)),
        "git clone failed".to_owned(),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_skipped_clone_list_entry_is_reported_as_a_skip() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "git-clone-list"
source = "plugins.txt"
dest-dir = "~/.plugins"
"#,
    );
    tree.repo_file(
        "plugins.txt",
        &format!("{} dest-name=taken\n", display(&origin.origin())),
    );
    fs::create_dir_all(tree.home(".plugins/taken")).expect("a directory in the way");

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

    let expected = format!(
        "skipped {}: it is a directory, and not a git clone",
        display(&tree.home(".plugins/taken"))
    );
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");
}

#[test]
fn a_remote_materialization_in_the_way_is_refused_whatever_the_policy() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[remotes.core]\ntype = \"git\"\nurl = \"{}\"\n",
        display(&origin.origin())
    ));
    let dest = tree.path("repo/remotes/core");
    fs::create_dir_all(&dest).expect("a directory in the way");

    for args in [&["sync"][..], &["sync", "--interactive"]] {
        let assertion = tree
            .batfiles()
            .args(args)
            .write_stdin("o\n")
            .assert()
            .failure()
            .code(1);
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains("it is a directory, and not a git clone"),
            "{args:?}: {stderr}"
        );
        assert!(
            !stderr.contains("back up and replace"),
            "{args:?}: {stderr}"
        );
    }
    assert!(dest.is_dir());
    assert!(backups_of(&dest).is_empty());
}

#[test]
fn a_destination_holding_the_link_source_is_never_set_aside() {
    let tree = Tree::new();
    let source = tree.repo_file("nested/a", "# a\n");
    let dest = tree.path("repo/nested");
    tree.write_manifest(&one_symlink("nested/a", &display(&dest)));

    for args in [&["sync"][..], &["sync", "--interactive"]] {
        let assertion = tree
            .batfiles()
            .args(args)
            .write_stdin("o\n")
            .assert()
            .failure()
            .code(1);
        let stderr = stderr_of(&assertion);
        assert!(
            stderr.contains(&format!(
                "cannot replace {}: {} is inside it",
                display(&dest),
                display(&source)
            )),
            "{args:?}: {stderr}"
        );
        assert!(
            !stderr.contains("back up and replace"),
            "{args:?}: {stderr}"
        );
    }
    assert_eq!(fs::read_to_string(&source).expect("the source"), "# a\n");
    assert!(backups_of(&dest).is_empty());
}

#[test]
fn a_seed_refresh_never_sets_aside_its_own_source() {
    let tree = Tree::new();
    let source = tree.repo_file("nested/a", "# a\n");
    let dest = tree.path("repo/nested");
    tree.write_manifest(&one_copy("nested/a", &display(&dest)));

    let assertion = tree
        .batfiles()
        .args(["sync", "--refresh-content"])
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("is inside it"), "{stderr}");
    assert_eq!(fs::read_to_string(&source).expect("the source"), "# a\n");
    assert!(backups_of(&dest).is_empty());
}

#[test]
fn a_clone_list_entry_that_fails_after_its_backup_costs_only_that_entry() {
    let origin = BareRepo::new();
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "git-clone-list"
source = "plugins.txt"
dest-dir = "~/.plugins"
"#,
    );
    tree.repo_file(
        "plugins.txt",
        &format!(
            "{origin} dest-name=taken ref=no-such-ref\n{origin} dest-name=last\n",
            origin = display(&origin.origin())
        ),
    );
    let taken = tree.home(".plugins/taken");
    fs::create_dir_all(&taken).expect("a directory in the way");
    fs::write(taken.join("mine.zsh"), "echo mine\n").expect("their file");

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert!(tree.home(".plugins/last/README.md").is_file());
    let backup = backup_of(&taken);
    assert_eq!(
        fs::read_to_string(backup.join("mine.zsh")).expect("their file"),
        "echo mine\n"
    );
    let stderr = stderr_of(&assertion);
    for expected in [
        "warning: not cloning".to_owned(),
        "no-such-ref".to_owned(),
        format!("is now at {}", display(&backup)),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_directory_with_damaged_git_metadata_is_a_conflict() {
    let origin = BareRepo::new();
    let tree = one_clone(&origin);
    let dest = tree.home(".oh-my-zsh");
    fs::create_dir_all(dest.join(".git")).expect("an empty git directory");

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();
    let expected = format!(
        "skipped {}: it is an incomplete or damaged clone",
        display(&dest)
    );
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");

    tree.batfiles().arg("sync").assert().success();
    assert!(dest.join("README.md").is_file(), "nothing was cloned");
    assert!(backup_of(&dest).join(".git").is_dir());
}

#[test]
fn a_link_source_reached_through_a_repository_alias_is_never_set_aside() {
    // Resolving the source traverses the destination symlink; replacing that link would create
    // a self-reference.
    let tree = Tree::new();
    let repo = tree.path("repo");
    tree.repo_file("real/a", "# a\n");
    std::os::unix::fs::symlink("real", repo.join("nested")).expect("an aliased directory");
    let alias = tree.path("alias");
    std::os::unix::fs::symlink(&repo, &alias).expect("an alias of the repository");
    let dest = repo.join("nested");
    tree.write_manifest(&one_symlink("nested/a", &display(&dest)));

    let assertion = tree
        .batfiles()
        .env("BATFILES_DIR", &alias)
        .arg("sync")
        .assert()
        .failure()
        .code(1);

    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("is inside it"), "{stderr}");
    assert_eq!(link_target(&dest), std::path::PathBuf::from("real"));
}

#[test]
fn a_git_that_fails_on_a_healthy_clone_is_not_a_conflict() {
    // Broken global Git configuration must not make a healthy clone look like a replaceable
    // conflict.
    let origin = BareRepo::new();
    let tree = one_clone(&origin);
    tree.batfiles().arg("sync").assert().success();
    let broken = tree.path("broken.gitconfig");
    fs::write(&broken, "[core\n").expect("a malformed configuration");

    for args in [&["sync", "--no-overwrite"][..], &["sync"]] {
        let assertion = tree
            .batfiles()
            .env("GIT_CONFIG_GLOBAL", &broken)
            .args(args)
            .assert()
            .failure()
            .code(1);
        let stderr = stderr_of(&assertion);
        assert!(!stderr.contains("damaged"), "{args:?}: {stderr}");
        assert!(!stderr.contains("skipped"), "{args:?}: {stderr}");
    }
    let dest = tree.home(".oh-my-zsh");
    assert!(dest.join("README.md").is_file());
    assert!(backups_of(&dest).is_empty());
}

#[test]
fn a_git_directory_whose_head_is_not_one_is_damaged() {
    let origin = BareRepo::new();
    let tree = one_clone(&origin);
    let dest = tree.home(".oh-my-zsh");
    for dir in ["objects", "refs"] {
        fs::create_dir_all(dest.join(".git").join(dir)).expect("a git layout");
    }
    fs::write(dest.join(".git/HEAD"), "not a head\n").expect("a garbled HEAD");

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("incomplete or damaged clone"), "{stderr}");
}

#[test]
fn a_clone_whose_object_store_is_a_link_is_updated_rather_than_replaced() {
    // A symlinked objects directory is valid Git storage.
    let origin = BareRepo::new();
    let tree = one_clone(&origin);
    tree.batfiles().arg("sync").assert().success();
    let dest = tree.home(".oh-my-zsh");
    let store = tree.path("objects");
    fs::rename(dest.join(".git/objects"), &store).expect("the object store moved");
    std::os::unix::fs::symlink(&store, dest.join(".git/objects")).expect("a linked store");
    fs::write(dest.join("mine.zsh"), "echo mine\n").expect("local content");
    origin.publish("plugin.zsh", "echo hello\n", "second");

    let assertion = interactive(&tree, "o\n").success();

    let stderr = stderr_of(&assertion);
    assert!(!stderr.contains("back up and replace"), "{stderr}");
    assert_eq!(
        fs::read_to_string(dest.join("mine.zsh")).expect("local content"),
        "echo mine\n"
    );
    assert!(backups_of(&dest).is_empty());
}

#[test]
fn a_clone_whose_commondir_reaches_nothing_is_damaged() {
    let origin = BareRepo::new();
    let tree = one_clone(&origin);
    tree.batfiles().arg("sync").assert().success();
    let dest = tree.home(".oh-my-zsh");
    fs::write(
        dest.join(".git/commondir"),
        format!("{}\n", display(&tree.path("nowhere"))),
    )
    .expect("a commondir to nothing");

    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();

    let expected = format!(
        "skipped {}: it is an incomplete or damaged clone",
        display(&dest)
    );
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
}
