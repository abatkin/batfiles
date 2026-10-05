//! `--refresh-content`: seeds installed again over what is already at their
//! destinations, with the conflict policy deciding what happens to it.

use std::fs;

use crate::support::*;

/// A tree seeding `seed/gitconfig` at `~/.gitconfig`, already synchronized, and
/// then edited on both sides: the repository publishes new content and the
/// user changed their copy.
fn a_seed_both_sides_changed() -> Tree {
    let tree = Tree::new();
    tree.repo_file("seed/gitconfig", "# first\n");
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));
    tree.batfiles().arg("sync").assert().success();
    tree.repo_file("seed/gitconfig", "# second\n");
    fs::write(tree.home(".gitconfig"), "# mine\n").expect("the user's edit");
    tree
}

fn refresh(tree: &Tree, extra: &[&str]) -> assert_cmd::assert::Assert {
    tree.batfiles()
        .args(["sync", "--refresh-content"])
        .args(extra)
        .assert()
}

#[test]
fn an_occupied_seed_is_kept_without_the_option() {
    let tree = a_seed_both_sides_changed();

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the seed"),
        "# mine\n"
    );
    assert!(backups_of(&tree.home(".gitconfig")).is_empty());
}

#[test]
fn a_changed_seed_is_backed_up_and_installed_again() {
    let tree = a_seed_both_sides_changed();

    let assertion = refresh(&tree, &[]).success();

    let dest = tree.home(".gitconfig");
    assert_eq!(fs::read_to_string(&dest).expect("the seed"), "# second\n");
    let backup = backup_of(&dest);
    assert_eq!(fs::read_to_string(&backup).expect("the backup"), "# mine\n");
    let stderr = stderr_of(&assertion);
    for expected in [
        format!("backed up {} to {}", display(&dest), display(&backup)),
        format!(
            "refreshed {} from {}",
            display(&dest),
            display(&tree.path("repo/seed/gitconfig"))
        ),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_seed_already_as_it_would_be_is_left_alone() {
    let tree = a_seed_both_sides_changed();
    refresh(&tree, &[]).success();

    let assertion = refresh(&tree, &["-v"]).success();

    assert_eq!(backups_of(&tree.home(".gitconfig")).len(), 1);
    let expected = format!("unchanged {}", display(&tree.home(".gitconfig")));
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    assert!(!stderr.contains("refreshed"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn a_seed_differing_only_in_its_mode_is_refreshed() {
    use std::os::unix::fs::PermissionsExt;

    let tree = Tree::new();
    let source = tree.repo_file("bin/tool", "#!/bin/sh\n");
    fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).expect("an executable");
    tree.write_manifest(&one_copy("bin/tool", "~/bin/tool"));
    tree.batfiles().arg("sync").assert().success();
    let dest = tree.home("bin/tool");
    fs::set_permissions(&dest, fs::Permissions::from_mode(0o644)).expect("a narrowed mode");

    refresh(&tree, &[]).success();

    let mode = |path| fs::metadata(path).expect("a file").permissions().mode() & 0o777;
    assert_eq!(mode(&dest), 0o755);
    assert_eq!(mode(&backup_of(&dest)), 0o644);
}

#[test]
fn a_directory_seed_is_replaced_whole_and_what_only_it_held_goes_with_the_backup() {
    let tree = Tree::new();
    tree.repo_file("seed/nvim/init.lua", "-- first\n");
    tree.write_manifest(&one_copy("seed/nvim", "~/.config/nvim"));
    tree.batfiles().arg("sync").assert().success();
    tree.repo_file("seed/nvim/init.lua", "-- second\n");
    let dest = tree.home(".config/nvim");
    fs::write(dest.join("local.lua"), "-- mine\n").expect("a file only the user has");

    refresh(&tree, &[]).success();

    assert_eq!(entries(&dest), ["init.lua"]);
    assert_eq!(
        fs::read_to_string(dest.join("init.lua")).expect("the seed"),
        "-- second\n"
    );
    let backup = backup_of(&dest);
    assert_eq!(entries(&backup), ["init.lua", "local.lua"]);
    assert_eq!(
        fs::read_to_string(backup.join("init.lua")).expect("the backup"),
        "-- first\n"
    );
}

#[test]
fn a_refresh_compares_only_what_the_filters_select() {
    let tree = Tree::new();
    tree.repo_file("seed/nvim/init.lua", "-- init\n");
    tree.repo_file("seed/nvim/local.lua", "-- the repository's own\n");
    tree.write_manifest(&format!(
        "{}exclude = \"local.lua\"\n",
        one_copy("seed/nvim", "~/.config/nvim")
    ));
    tree.batfiles().arg("sync").assert().success();
    let dest = tree.home(".config/nvim");
    assert_eq!(entries(&dest), ["init.lua"]);

    let assertion = refresh(&tree, &["-v"]).success();

    assert!(backups_of(&dest).is_empty());
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!("unchanged {}", display(&dest))),
        "{stderr}"
    );
}

#[test]
fn copy_dir_refreshes_each_child_on_its_own() {
    let tree = Tree::new();
    tree.repo_file("seed/same", "same\n");
    tree.repo_file("seed/changed", "first\n");
    tree.write_manifest(&one_copy_dir("seed", "~/seeds", false));
    tree.batfiles().arg("sync").assert().success();
    tree.repo_file("seed/changed", "second\n");

    refresh(&tree, &[]).success();

    assert_eq!(
        fs::read_to_string(tree.home("seeds/changed")).expect("the seed"),
        "second\n"
    );
    assert_eq!(backups_of(&tree.home("seeds/changed")).len(), 1);
    assert!(backups_of(&tree.home("seeds/same")).is_empty());
}

#[test]
fn a_dry_run_says_what_a_refresh_would_do_and_changes_nothing() {
    let tree = a_seed_both_sides_changed();
    let before = snapshot(&tree.path("home"));

    let assertion = refresh(&tree, &["--dry-run"]).success();

    assert_eq!(snapshot(&tree.path("home")), before);
    let dest = display(&tree.home(".gitconfig"));
    let stderr = stderr_of(&assertion);
    for expected in [
        format!("would back up {dest} to {dest}.batfiles-backup-"),
        format!("would refresh {dest} from"),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn no_overwrite_leaves_every_occupied_seed_as_it_is() {
    let tree = a_seed_both_sides_changed();

    refresh(&tree, &["--no-overwrite"]).success();

    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the seed"),
        "# mine\n"
    );
    assert!(backups_of(&tree.home(".gitconfig")).is_empty());
}

#[test]
fn interactive_asks_only_where_the_content_differs() {
    let tree = a_seed_both_sides_changed();
    tree.repo_file("seed/same", "same\n");
    tree.write_manifest(&format!(
        "{}\n{}",
        one_copy("seed/gitconfig", "~/.gitconfig"),
        one_copy("seed/same", "~/.same")
    ));
    tree.batfiles().arg("sync").assert().success();

    let assertion = tree
        .batfiles()
        .args(["sync", "--refresh-content", "--interactive"])
        .write_stdin("s\n")
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert_eq!(
        stderr.matches("back up and replace (b)").count(),
        1,
        "{stderr}"
    );
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the seed"),
        "# mine\n"
    );
}

#[cfg(unix)]
#[test]
fn a_link_into_the_repository_at_a_seed_is_replaced_without_a_backup() {
    let tree = Tree::new();
    let source = tree.repo_file("seed/gitconfig", "# seed\n");
    tree.write_manifest(&one_copy("seed/gitconfig", "~/.gitconfig"));
    std::os::unix::fs::symlink(&source, tree.home(".gitconfig")).expect("a link to ours");

    refresh(&tree, &[]).success();

    let dest = tree.home(".gitconfig");
    assert!(!dest.is_symlink(), "the link was kept");
    assert_eq!(fs::read_to_string(&dest).expect("the seed"), "# seed\n");
    assert!(backups_of(&dest).is_empty());
}

/// A tree fetching `/tool` to `~/bin/tool` and `/tool.tar.gz` to
/// `~/.local/tool`, from `server`.
fn fetching(server: &Server) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "fetch-file"
source = "{address}/tool"
dest = "~/bin/tool"

[[actions]]
type = "fetch-archive"
source = "{address}/tool.tar.gz"
dest = "~/.local/tool"
"#,
        address = server.address()
    ));
    tree
}

const TOOL: &[Member] = &[Member::File("README.md", 0o644, "# tool\n")];

#[test]
fn fetched_seeds_are_downloaded_again() {
    let server = Server::new(&[
        ("/tool", Reply::Body("#!/bin/sh\necho tool\n")),
        ("/tool.tar.gz", Reply::Bytes(tarball(TOOL))),
    ]);
    let tree = fetching(&server);
    tree.batfiles().arg("sync").assert().success();
    fs::write(tree.home("bin/tool"), "mine\n").expect("the user's edit");
    fs::write(tree.home(".local/tool/README.md"), "mine\n").expect("the user's edit");

    refresh(&tree, &[]).success();

    assert_eq!(server.requests(), 4);
    assert_eq!(
        fs::read_to_string(tree.home("bin/tool")).expect("the file"),
        "#!/bin/sh\necho tool\n"
    );
    assert_eq!(
        fs::read_to_string(tree.home(".local/tool/README.md")).expect("the archive"),
        "# tool\n"
    );
    assert_eq!(backups_of(&tree.home("bin/tool")).len(), 1);
    assert_eq!(backups_of(&tree.home(".local/tool")).len(), 1);
}

#[test]
fn a_dry_run_refresh_fetches_nothing() {
    let server = Server::new(&[
        ("/tool", Reply::Body("#!/bin/sh\necho tool\n")),
        ("/tool.tar.gz", Reply::Bytes(tarball(TOOL))),
    ]);
    let tree = fetching(&server);
    tree.batfiles().arg("sync").assert().success();

    refresh(&tree, &["--dry-run"]).success();
    refresh(&tree, &["--no-overwrite"]).success();

    assert_eq!(server.requests(), 2);
}

#[test]
fn clone_forwards_the_options_to_its_synchronization() {
    let origin = BareRepo::new();
    origin.publish("seed/gitconfig", "# theirs\n", "a seed");
    origin.publish(
        "batfiles.toml",
        &one_copy("seed/gitconfig", "~/.gitconfig"),
        "declare it",
    );
    let tree = Tree::roots();
    fs::write(tree.home(".gitconfig"), "# mine\n").expect("the user's file");

    tree.batfiles()
        .args(["clone", &display(&origin.origin()), "--refresh-content"])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the seed"),
        "# theirs\n"
    );
    assert_eq!(
        fs::read_to_string(backup_of(&tree.home(".gitconfig"))).expect("the backup"),
        "# mine\n"
    );
}
