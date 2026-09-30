//! CLI tests for directory creation and copying. Symlink installation is covered in `linking`.

use std::fs;

use crate::support::*;

#[test]
fn a_create_dir_action_makes_the_directory_and_says_so() {
    let tree = Tree::new();
    tree.write_manifest(&one_create_dir("~/.local/share/zsh-plugins"));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        format!(
            "created {}\n",
            display(&tree.home(".local/share/zsh-plugins"))
        )
    );
    assert!(tree.home(".local/share/zsh-plugins").is_dir());
}

#[test]
fn a_create_dir_action_parses_with_every_field_it_accepts() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "create-dir"
id = "plugin-root"
group = "shell"
dest = "~/.config"
"#,
    );

    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home(".config").is_dir());
}

#[test]
fn a_create_dir_action_run_twice_changes_nothing() {
    let tree = Tree::new();
    tree.write_manifest(&one_create_dir("~/.config"));
    tree.batfiles().arg("sync").assert().success();
    fs::write(tree.home(".config/theirs"), "mine\n").expect("a file inside it");

    tree.batfiles().arg("sync").assert().success().stderr("");
    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let expected = format!("unchanged {}", display(&tree.home(".config")));
    assert!(
        stderr_of(&assertion).contains(&expected),
        "no `{expected}` in:\n{}",
        stderr_of(&assertion)
    );
    assert_eq!(entries(&tree.home(".config")), ["theirs"]);
}

#[test]
fn a_create_dir_action_over_a_file_backs_the_file_up() {
    let tree = Tree::new();
    fs::write(tree.home(".config"), "mine\n").expect("an existing file");
    tree.write_manifest(&one_create_dir("~/.config"));

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    assert!(tree.home(".config").is_dir(), "no directory was made");
    assert_eq!(
        fs::read_to_string(backup_of(&tree.home(".config"))).expect("the backup"),
        "mine\n"
    );
    for expected in [
        format!("backed up {}", display(&tree.home(".config"))),
        format!("created {}", display(&tree.home(".config"))),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

/// An existing symlink to a directory satisfies `create-dir`.
#[cfg(unix)]
#[test]
fn a_create_dir_destination_symlinked_elsewhere_is_satisfied_by_what_it_reaches() {
    let tree = Tree::new();
    let elsewhere = tree.path("elsewhere");
    fs::create_dir(&elsewhere).expect("a directory on another volume");
    std::os::unix::fs::symlink(&elsewhere, tree.home(".config")).expect("a deliberate link");
    tree.write_manifest(&one_create_dir("~/.config"));

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert!(tree.home(".config").is_symlink(), "the link was replaced");
}

/// Create a seed tree containing a file and a nested directory with another file.
fn with_seed(tree: &Tree) {
    tree.repo_file("seed/gitconfig", "[user]\n\temail = yours\n");
    tree.repo_file("seed/inputrc", "set editing-mode vi\n");
    tree.repo_file("seed/scripts/hello", "#!/bin/sh\necho hi\n");
}

#[test]
fn a_copy_action_seeds_a_file_and_says_so() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.config/git/config"));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        format!(
            "copied {} from {}\n",
            display(&tree.home(".config/git/config")),
            display(&tree.path("repo/seed/gitconfig"))
        )
    );
    assert_eq!(
        fs::read_to_string(tree.home(".config/git/config")).expect("the copy"),
        "[user]\n\temail = yours\n"
    );
    assert!(!tree.home(".config/git/config").is_symlink());
}

/// Report removal of a broken parent symlink before reporting the copy.
#[cfg(unix)]
#[test]
fn a_broken_link_above_a_copy_is_cleared_and_the_removal_reported_first() {
    let tree = Tree::new();
    with_seed(&tree);
    let nowhere = tree.path("nowhere");
    std::os::unix::fs::symlink(&nowhere, tree.home(".config")).expect("a broken link");
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.config/git/config"));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();

    assert_eq!(
        stderr_of(&assertion),
        format!(
            "removed a broken symlink to {} to make {}\ncopied {} from {}\n",
            display(&nowhere),
            display(&tree.home(".config")),
            display(&tree.home(".config/git/config")),
            display(&tree.path("repo/seed/gitconfig"))
        )
    );
}

#[test]
fn a_copy_action_parses_with_every_field_it_accepts() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(
        r#"[[actions]]
type = "copy"
id = "gitconfig"
group = "git"
source = "seed/gitconfig"
dest = "~/.gitconfig"
"#,
    );

    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home(".gitconfig").is_file());
}

#[test]
fn a_copy_action_leaves_an_occupied_destination_exactly_as_it_is() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));
    fs::write(tree.home(".gitconfig"), "[user]\n\temail = mine\n").expect("their own file");

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the file"),
        "[user]\n\temail = mine\n"
    );

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let expected = format!("kept {}", display(&tree.home(".gitconfig")));
    assert!(
        stderr_of(&assertion).contains(&expected),
        "no `{expected}` in:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_copy_action_run_twice_changes_nothing() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));
    tree.batfiles().arg("sync").assert().success();
    fs::write(tree.home(".gitconfig"), "edited by hand\n").expect("the user's edit");

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the file"),
        "edited by hand\n"
    );
}

#[test]
fn a_copy_action_installs_a_directory_whole() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        entries(&tree.home(".seed")),
        ["gitconfig", "inputrc", "scripts"]
    );
    assert_eq!(
        fs::read_to_string(tree.home(".seed/scripts/hello")).expect("the nested copy"),
        "#!/bin/sh\necho hi\n"
    );
}

#[test]
fn a_copy_action_over_an_existing_directory_does_nothing_at_all() {
    let tree = Tree::new();
    with_seed(&tree);
    fs::create_dir(tree.home(".seed")).expect("a directory already there");
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert_eq!(entries(&tree.home(".seed")), Vec::<String>::new());
}

#[test]
fn a_copy_dir_action_seeds_every_direct_child() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy_dir("seed", "~/installed", false));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        entries(&tree.home("installed")),
        ["gitconfig", "inputrc", "scripts"]
    );
    assert_eq!(
        fs::read_to_string(tree.home("installed/scripts/hello")).expect("the nested copy"),
        "#!/bin/sh\necho hi\n"
    );
}

#[test]
fn a_copy_dir_action_keeps_what_is_there_and_seeds_the_rest() {
    let tree = Tree::new();
    with_seed(&tree);
    fs::create_dir(tree.home("installed")).expect("an existing destination");
    fs::write(tree.home("installed/gitconfig"), "mine\n").expect("their own file");
    tree.write_manifest(&one_copy_dir("seed", "~/installed", false));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        fs::read_to_string(tree.home("installed/gitconfig")).expect("the file"),
        "mine\n"
    );
    assert_eq!(
        fs::read_to_string(tree.home("installed/inputrc")).expect("the seeded sibling"),
        "set editing-mode vi\n"
    );
}

#[test]
fn a_child_directory_that_is_already_there_is_kept_whole_rather_than_merged() {
    let tree = Tree::new();
    with_seed(&tree);
    fs::create_dir_all(tree.home("installed/scripts")).expect("their own directory");
    fs::write(tree.home("installed/scripts/theirs"), "# theirs\n").expect("their own file");
    tree.write_manifest(&one_copy_dir("seed", "~/installed", false));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(entries(&tree.home("installed/scripts")), ["theirs"]);
    assert!(tree.home("installed/gitconfig").is_file());
}

#[test]
fn a_copy_dir_action_creates_its_destination_directory() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy_dir("seed", "~/installed", false));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    let expected = format!("created {}", display(&tree.home("installed")));
    assert!(
        stderr_of(&assertion).contains(&expected),
        "no `{expected}` in:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_copy_dir_with_nothing_in_it_creates_its_destination_and_says_so() {
    let tree = Tree::new();
    fs::create_dir_all(tree.path("repo/seed")).expect("an empty source directory");
    tree.write_manifest(&one_copy_dir("seed", "~/installed", false));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        format!("created {}\n", display(&tree.home("installed")))
    );
    assert_eq!(entries(&tree.home("installed")), Vec::<String>::new());

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    assert!(
        stderr_of(&assertion).contains("no children to copy"),
        "the empty directory was not reported:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn copy_dir_dots_every_installed_name_and_nothing_else() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy_dir("seed", "~", true));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        entries(&tree.path("home")),
        [".gitconfig", ".inputrc", ".scripts"]
    );
    // Only the top level is dotted; what is inside keeps its own names.
    assert_eq!(entries(&tree.home(".scripts")), ["hello"]);
}

#[test]
fn a_copy_dir_child_that_is_already_a_dotfile_is_refused_under_dot_prefix() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.repo_file("seed/.hidden", "# oops\n");
    tree.write_manifest(&one_copy_dir("seed", "~", true));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    for expected in [".hidden", "..hidden"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_copy_dirs_source_that_is_not_a_directory_is_refused() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy_dir("seed/gitconfig", "~/installed", false));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    for expected in [
        "not a directory".to_owned(),
        display(&tree.path("repo/seed/gitconfig")),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_copy_source_the_repository_does_not_have_is_refused() {
    let tree = Tree::new();
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains(&display(&tree.path("repo/seed/gitconfig"))),
        "the source was not named:\n{}",
        stderr_of(&assertion)
    );
    assert!(!tree.home(".gitconfig").exists(), "something was written");
}

#[test]
fn a_copy_whose_destination_is_inside_its_source_is_refused() {
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    fs::write(
        repo.join("batfiles.toml"),
        one_copy("seed", "~/dotfiles/seed/inner"),
    )
    .expect("a manifest");

    let assertion = tree
        .batfiles()
        .args(["--batfiles-dir", &display(&repo), "sync"])
        .assert()
        .failure()
        .code(1);
    assert!(
        stderr_of(&assertion).contains("which is inside it"),
        "unexpected stderr:\n{}",
        stderr_of(&assertion)
    );
    assert_eq!(entries(&repo.join("seed")), ["a"]);
}

#[test]
fn a_copy_dir_whose_destination_is_inside_its_source_is_refused() {
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    fs::write(
        repo.join("batfiles.toml"),
        one_copy_dir("seed", "~/dotfiles/seed/inner", false),
    )
    .expect("a manifest");

    let assertion = tree
        .batfiles()
        .args(["--batfiles-dir", &display(&repo), "sync"])
        .assert()
        .failure()
        .code(1);
    assert!(
        stderr_of(&assertion).contains("which is inside it"),
        "unexpected stderr:\n{}",
        stderr_of(&assertion)
    );
    assert_eq!(entries(&repo.join("seed")), ["a"]);
}

#[test]
fn a_copy_dir_checks_its_source_before_it_checks_its_destination() {
    // Both source-kind and containment checks would fail; report the source-kind error first.
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    fs::write(
        repo.join("batfiles.toml"),
        one_copy_dir("seed/a", "~/dotfiles/seed/a/inner", false),
    )
    .expect("a manifest");

    let assertion = tree
        .batfiles()
        .args(["--batfiles-dir", &display(&repo), "sync"])
        .assert()
        .failure()
        .code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("not a directory"),
        "the source was not what was reported:\n{stderr}"
    );
    assert!(
        !stderr.contains("which is inside it"),
        "the destination was reported ahead of the source:\n{stderr}"
    );
}

/// A failed copy must leave its destination absent.
#[cfg(unix)]
#[test]
fn a_copy_that_fails_partway_leaves_nothing_at_its_destination() {
    let tree = Tree::new();
    tree.repo_file("seed/a-file", "good\n");
    // Sorted order puts `a-file` first, so one file is already written when the
    // link stops the copy.
    std::os::unix::fs::symlink("/nowhere", tree.path("repo/seed/b-link")).expect("a link");
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        fs::symlink_metadata(tree.home(".seed")).is_err(),
        "a partial copy was left where the next run would keep it"
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains("it is a symlink"),
        "the second run did not report the same problem:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_seeded_file_is_not_written_at_its_destination() {
    let tree = Tree::new();
    with_seed(&tree);
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));
    tree.batfiles().arg("sync").assert().success();

    assert_eq!(entries(&tree.path("home")), [".gitconfig"]);
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the copy"),
        "[user]\n\temail = yours\n"
    );
}

#[test]
fn something_at_the_staging_path_is_named_rather_than_removed() {
    let tree = Tree::new();
    with_seed(&tree);
    let in_the_way = tree.home(".seed.batfiles-incomplete");
    fs::create_dir(&in_the_way).expect("something already at the staging path");
    fs::write(in_the_way.join("irreplaceable"), "mine\n").expect("data inside it");
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains(&display(&in_the_way)),
        "the path in the way was not named:\n{}",
        stderr_of(&assertion)
    );
    assert_eq!(
        fs::read_to_string(in_the_way.join("irreplaceable")).expect("the data survives"),
        "mine\n"
    );
    assert!(
        fs::symlink_metadata(tree.home(".seed")).is_err(),
        "the destination was installed over a failure"
    );
}

/// Incomplete copies must not expose broader permissions than their source.
#[cfg(unix)]
#[test]
fn a_copy_is_never_readable_by_more_people_than_its_source() {
    use std::os::unix::fs::PermissionsExt;

    let tree = Tree::new();
    let private = tree.path("repo/seed");
    tree.repo_file("seed/held/token", "secret\n");
    // The read-only child prevents cleanup, leaving staging permissions inspectable after
    // failure.
    let held = private.join("held");
    fs::set_permissions(&held, fs::Permissions::from_mode(0o555)).expect("a read-only source");
    // Sorted after `held`, so the copy stops with the secret already written.
    std::os::unix::fs::symlink("/nowhere", private.join("z-link")).expect("a link");
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).expect("a private source");
    tree.write_manifest(&one_copy("seed", "~/.private"));

    tree.batfiles().arg("sync").assert().failure().code(1);

    let leftover = tree.home(".private.batfiles-incomplete");
    let mode = fs::metadata(&leftover)
        .expect("the leftover")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        format!("{mode:o}"),
        "700",
        "an incomplete copy of a private directory was left reachable by anyone"
    );

    // Restore permissions for temporary-directory cleanup.
    for path in [&held, &leftover.join("held")] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("writable again");
    }
}

/// Keep an existing destination that resolves into the source.
#[cfg(unix)]
#[test]
fn a_destination_resolving_into_the_source_is_kept_rather_than_refused() {
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    fs::create_dir(repo.join("seed/inner")).expect("somewhere inside the source");
    std::os::unix::fs::symlink(repo.join("seed/inner"), tree.home(".seed")).expect("their link");
    fs::write(
        repo.join("batfiles.toml"),
        format!(
            "{}{}",
            one_copy("seed", "~/.seed"),
            one_create_dir("~/later-action-ran")
        ),
    )
    .expect("a manifest");

    tree.batfiles()
        .args(["--batfiles-dir", &display(&repo), "sync"])
        .assert()
        .success();
    assert!(
        tree.home(".seed").is_symlink(),
        "the destination was not left alone"
    );
    assert!(tree.home("later-action-ran").is_dir());
}

/// A failed copy must leave its destination absent even if staging cleanup fails.
#[cfg(unix)]
#[test]
fn a_copy_that_fails_leaves_no_destination_even_when_it_cannot_clean_up() {
    use std::os::unix::fs::PermissionsExt;

    let tree = Tree::new();
    tree.repo_file("seed/a-dir/inner", "x\n");
    // Sorted after `a-dir`, so the read-only copy already exists when this
    // stops the run.
    std::os::unix::fs::symlink("/nowhere", tree.path("repo/seed/b-link")).expect("a link");
    let read_only = tree.path("repo/seed/a-dir");
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o555)).expect("a read-only source");
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        fs::symlink_metadata(tree.home(".seed")).is_err(),
        "a partial copy was left where the next run would keep it"
    );
    assert!(
        stderr_of(&assertion).contains("could not remove the incomplete work"),
        "the leftover was not reported:\n{}",
        stderr_of(&assertion)
    );

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    assert!(
        stderr_of(&assertion).contains("something is already at"),
        "the second run did not name what was in the way:\n{}",
        stderr_of(&assertion)
    );
    assert!(
        fs::symlink_metadata(tree.home(".seed")).is_err(),
        "the destination appeared on the second run"
    );

    // Restore permissions for temporary-directory cleanup.
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o755)).expect("the source back");
    let leftover = tree.home(".seed.batfiles-incomplete/a-dir");
    fs::set_permissions(&leftover, fs::Permissions::from_mode(0o755)).expect("the leftover back");
}

/// Keep a destination symlink and leave its target unchanged.
#[cfg(unix)]
#[test]
fn a_seed_does_not_write_through_a_symlink_at_its_destination() {
    let tree = Tree::new();
    with_seed(&tree);
    let theirs = tree.path("theirs");
    fs::write(&theirs, "theirs\n").expect("their file");
    std::os::unix::fs::symlink(&theirs, tree.home(".gitconfig")).expect("their link");
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert!(
        tree.home(".gitconfig").is_symlink(),
        "the link was replaced"
    );
    assert_eq!(
        fs::read_to_string(&theirs).expect("their file"),
        "theirs\n",
        "the seed was written through the link"
    );
}

/// Copies preserve source permissions.
#[cfg(unix)]
#[test]
fn a_copy_carries_the_permissions_of_what_it_copied() {
    use std::os::unix::fs::PermissionsExt;

    let tree = Tree::new();
    with_seed(&tree);
    let script = tree.path("repo/seed/scripts/hello");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("an executable source");
    let private = tree.path("repo/seed/private");
    fs::create_dir(&private).expect("a directory nobody else may read");
    fs::write(private.join("token"), "secret\n").expect("something in it");
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).expect("its mode");

    tree.write_manifest(&one_copy("seed", "~/.seed"));
    tree.batfiles().arg("sync").assert().success();

    let installed = fs::metadata(tree.home(".seed/scripts/hello")).expect("the copy");
    assert!(
        installed.permissions().mode() & 0o111 != 0,
        "an executable source installed as a file nobody can run"
    );
    let directory = fs::metadata(tree.home(".seed/private")).expect("the copied directory");
    assert_eq!(
        directory.permissions().mode() & 0o777,
        0o700,
        "a private source directory was installed more broadly readable"
    );
    assert_eq!(
        fs::read_to_string(tree.home(".seed/private/token")).expect("its contents"),
        "secret\n"
    );
}

/// Reject symlinks inside a copied directory.
#[cfg(unix)]
#[test]
fn a_symlink_inside_a_copied_tree_is_refused_rather_than_flattened() {
    let tree = Tree::new();
    with_seed(&tree);
    let link = tree.path("repo/seed/link-to-gitconfig");
    std::os::unix::fs::symlink("gitconfig", &link).expect("a link in the repository");
    tree.write_manifest(&one_copy("seed", "~/.seed"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    for expected in [display(&link), "it is a symlink".to_owned()] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

/// Follow a symlink when resolving the manifest's source path.
#[cfg(unix)]
#[test]
fn a_source_the_manifest_named_through_a_link_is_followed() {
    let tree = Tree::new();
    with_seed(&tree);
    std::os::unix::fs::symlink("gitconfig", tree.path("repo/seed/aliased"))
        .expect("a link the repository stores");
    tree.write_manifest(&one_copy("seed/aliased", "~/.gitconfig"));

    tree.batfiles().arg("sync").assert().success();
    assert!(
        !tree.home(".gitconfig").is_symlink(),
        "the link was reproduced instead of what it names"
    );
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the copy"),
        "[user]\n\temail = yours\n"
    );
}

/// When two seeds share a destination, the first declared seed supplies its content.
#[test]
fn the_first_of_two_seeds_naming_one_destination_is_the_one_that_lands() {
    let (winner, loser, dest) = LEAF_ORDERED_PAIR;
    let tree = Tree::fixture("leaf");
    let repo = tree.path("repo");
    let installed = tree.home(dest);

    // Non-Unix runs fail at a later symlink action, after both seeds have run.
    let assertion = tree.batfiles().args(["sync", "-v"]).assert();
    let stderr = stderr_of(&assertion);

    let contents = fs::read_to_string(&installed).expect("the seeded file");
    assert_eq!(
        contents,
        fs::read_to_string(repo.join(winner)).expect("the winning source"),
        "`{dest}` does not hold `{winner}`, so the seeds ran out of order"
    );
    // Guards the assertion above: two identical sources would satisfy it
    // whichever one landed.
    assert_ne!(
        contents,
        fs::read_to_string(repo.join(loser)).expect("the losing source"),
        "`{winner}` and `{loser}` hold the same bytes, so this proves nothing"
    );

    let copied = format!(
        "copied {} from {}",
        display(&installed),
        display(&repo.join(winner))
    );
    let kept = format!("kept {}", display(&installed));
    let said = |line: &str| {
        stderr
            .find(line)
            .unwrap_or_else(|| panic!("no `{line}` in:\n{stderr}"))
    };
    assert!(
        said(&copied) < said(&kept),
        "the seeds reported out of order:\n{stderr}"
    );
}

/// Keep portable fixture actions before symlink actions so unsupported platforms execute them
/// before failing.
#[test]
fn the_leaf_manifest_declares_its_portable_actions_before_its_symlinks() {
    // The closing quote is what keeps `symlink` from matching `symlink-dir`,
    // and `copy` from matching `copy-dir`.
    const PORTABLE: [&str; 3] = [
        "type = \"create-dir\"",
        "type = \"copy\"",
        "type = \"copy-dir\"",
    ];
    const LINKING: [&str; 2] = ["type = \"symlink\"", "type = \"symlink-dir\""];

    let manifest = fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/leaf/batfiles.toml"),
    )
    .expect("the fixture manifest");
    let boundary = LINKING
        .iter()
        .filter_map(|declaration| manifest.find(declaration))
        .min()
        .expect("the fixture declares an action that makes symlinks");

    for declaration in PORTABLE {
        assert!(
            manifest.contains(declaration),
            "the fixture declares no `{declaration}`"
        );
        assert!(
            !manifest[boundary..].contains(declaration),
            "`{declaration}` is declared after the first action that makes a symlink"
        );
    }
}

/// On platforms without symlink support, execute earlier actions and stop at the first symlink
/// action.
#[cfg(not(unix))]
#[test]
fn the_actions_that_need_no_symlink_run_where_symlinks_cannot_be_made() {
    let tree = Tree::fixture("leaf");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`symlink` actions are not supported"),
        "unexpected stderr:\n{stderr}"
    );

    assert_leaf_portable_actions(&tree);
    assert!(
        !tree.home(".zshrc").exists(),
        "a symlink action ran on a platform that cannot make one"
    );
}

/// Report an unsupported-platform error for symlink actions.
#[cfg(not(unix))]
#[test]
fn a_symlink_action_reports_that_the_platform_cannot_run_it() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("`symlink` actions are not supported"),
        "unexpected stderr:\n{stderr}"
    );
    assert!(
        !tree.home(".zshrc").exists(),
        "the destination was touched anyway"
    );
}
