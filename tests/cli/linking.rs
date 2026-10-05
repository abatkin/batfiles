//! Unix CLI tests for symlink installation, repair, and destination handling.

use std::fs;
use std::path::{Path, PathBuf};

use crate::support::*;

#[test]
fn a_symlink_action_parses_with_every_field_it_accepts() {
    let tree = Tree::new();
    tree.repo_file("files/zshrc", "# zsh\n");
    tree.write_manifest(
        r#"[[actions]]
type = "symlink"
id = "zshrc"
group = "shell"
source = "files/zshrc"
dest = "~/.zshrc"
"#,
    );

    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home(".zshrc").is_symlink());
}

#[test]
fn a_symlink_action_creates_the_link_and_says_so() {
    let tree = Tree::new();
    let source = tree.repo_file("shell/zshrc", "# zsh\n");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        format!(
            "linked {} -> {}\n",
            display(&tree.home(".zshrc")),
            display(&source)
        )
    );
    assert_eq!(link_target(&tree.home(".zshrc")), source);
}

#[test]
fn every_action_in_the_manifest_runs() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    tree.repo_file("shell/inputrc", "# readline\n");
    tree.write_manifest(&format!(
        "{}{}",
        one_symlink("shell/zshrc", "~/.zshrc"),
        one_symlink("shell/inputrc", "~/.inputrc")
    ));

    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home(".zshrc").is_symlink());
    assert!(tree.home(".inputrc").is_symlink());
}

#[test]
fn a_link_into_the_repository_is_repaired() {
    let tree = Tree::new();
    let stale = tree.repo_file("shell/zshrc.old", "# old\n");
    let wanted = tree.repo_file("shell/zshrc", "# zsh\n");
    std::os::unix::fs::symlink(&stale, tree.home(".zshrc")).expect("a stale link");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    assert_eq!(
        stderr_of(&assertion),
        format!(
            "relinked {} -> {} (was {})\n",
            display(&tree.home(".zshrc")),
            display(&wanted),
            display(&stale)
        )
    );
    assert_eq!(link_target(&tree.home(".zshrc")), wanted);
    assert!(stale.exists(), "the old source was removed");
}

#[test]
fn a_relative_link_pointing_at_the_wrong_file_is_repaired() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc.old", "# old\n");
    let wanted = tree.repo_file("shell/zshrc", "# zsh\n");
    std::os::unix::fs::symlink("../repo/shell/zshrc.old", tree.home(".zshrc"))
        .expect("a stale relative link");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(link_target(&tree.home(".zshrc")), wanted);
}

#[test]
fn a_relative_repository_still_yields_a_link_that_resolves() {
    let tree = Tree::new();
    let source = tree.repo_file("shell/zshrc", "# zsh\n");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    tree.batfiles()
        .current_dir(tree.root())
        .args(["sync", "--batfiles-dir", "repo", "--home-dir", "home"])
        .assert()
        .success();
    assert_eq!(link_target(&tree.home(".zshrc")), source);
    assert_eq!(
        fs::read_to_string(tree.home(".zshrc")).expect("the link resolves"),
        "# zsh\n"
    );
}

#[test]
fn a_relative_link_into_the_repository_is_recognized() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    std::os::unix::fs::symlink("../repo/shell/zshrc", tree.home(".zshrc"))
        .expect("a relative link");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    tree.batfiles().arg("sync").assert().success().stderr("");
    assert_eq!(
        link_target(&tree.home(".zshrc")),
        PathBuf::from("../repo/shell/zshrc"),
        "the spelling was rewritten"
    );
}

#[test]
fn a_link_that_only_looks_like_it_points_into_the_repository_is_not_ours() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    let escaping = tree.path("repo").join("../outside");
    // The target must exist; a broken link would be replaceable.
    fs::write(tree.path("outside"), "someone else's\n").expect("the target");
    std::os::unix::fs::symlink(&escaping, tree.home(".zshrc")).expect("an escaping link");

    let stderr = skipped(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
    assert!(
        stderr.contains(&display(&tree.home(".zshrc"))),
        "the destination was not named:\n{stderr}"
    );
    assert_eq!(link_target(&tree.home(".zshrc")), escaping);
}

#[test]
fn a_broken_link_at_a_destination_is_replaced_wherever_it_pointed() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    let nowhere = tree.path("nowhere");
    std::os::unix::fs::symlink(&nowhere, tree.home(".zshrc")).expect("a broken link");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!("(was {})", display(&nowhere))),
        "the replaced target was not named:\n{stderr}"
    );
    assert_eq!(
        link_target(&tree.home(".zshrc")),
        tree.path("repo/shell/zshrc")
    );
    assert!(
        !nowhere.exists(),
        "the far end of the broken link was created"
    );
}

#[test]
fn a_link_at_a_deliberately_broken_source_is_still_left_alone() {
    let tree = Tree::new();
    fs::create_dir(tree.path("repo/shell")).expect("a source directory");
    std::os::unix::fs::symlink("nowhere", tree.path("repo/shell/zshrc")).expect("a broken source");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));
    tree.batfiles().arg("sync").assert().success();

    tree.batfiles().arg("sync").assert().success().stderr("");
}

#[test]
fn a_symlink_inside_its_source_is_refused_even_over_a_link_it_could_repair() {
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    // A broken link exercises the repair path.
    std::os::unix::fs::symlink(tree.path("nowhere"), repo.join("seed/inner"))
        .expect("a replaceable link");
    fs::write(
        repo.join("batfiles.toml"),
        one_symlink("seed", "~/dotfiles/seed/inner"),
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
    assert_eq!(
        link_target(&repo.join("seed/inner")),
        tree.path("nowhere"),
        "the link was replaced despite the refusal"
    );
}

#[test]
fn a_symlink_whose_destination_is_inside_its_source_is_refused() {
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    fs::write(
        repo.join("batfiles.toml"),
        one_symlink("seed", "~/dotfiles/seed/inner"),
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
fn a_symlink_dir_whose_destination_is_inside_its_source_is_refused() {
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    fs::write(
        repo.join("batfiles.toml"),
        one_symlink_dir("seed", "~/dotfiles/seed/inner", false),
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
fn a_link_that_is_already_right_is_left_alone() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));
    tree.batfiles().arg("sync").assert().success();

    tree.batfiles().arg("sync").assert().success().stderr("");
    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);
    let expected = format!("unchanged {}", display(&tree.home(".zshrc")));
    assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
}

#[test]
fn quiet_suppresses_what_sync_did() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    tree.batfiles()
        .args(["sync", "--quiet"])
        .assert()
        .success()
        .stderr("");
    assert!(tree.home(".zshrc").is_symlink());
}

#[test]
fn a_missing_parent_of_a_destination_is_created() {
    let tree = Tree::new();
    let source = tree.repo_file("nvim/init.lua", "-- nvim\n");
    tree.write_manifest(&one_symlink("nvim/init.lua", "~/.config/nvim/init.lua"));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(link_target(&tree.home(".config/nvim/init.lua")), source);
}

#[test]
fn a_file_in_the_way_of_a_parent_is_named_for_what_it_is() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    fs::write(tree.home(".config"), "not a directory\n").expect("a file in the way");

    let stderr = skipped(&tree, &one_symlink("shell/zshrc", "~/.config/zsh/zshrc"));
    for expected in [display(&tree.home(".config")), "a regular file".to_owned()] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(
        fs::read_to_string(tree.home(".config")).expect("the file"),
        "not a directory\n"
    );
}

#[test]
fn a_broken_link_in_the_way_of_a_parent_is_cleared_and_the_removal_reported() {
    let tree = Tree::new();
    let source = tree.repo_file("nvim/init.lua", "-- nvim\n");
    fs::create_dir(tree.home(".config")).expect("a config directory");
    let nowhere = tree.path("nowhere");
    std::os::unix::fs::symlink(&nowhere, tree.home(".config/nvim")).expect("a broken link");
    tree.write_manifest(&one_symlink("nvim/init.lua", "~/.config/nvim/init.lua"));

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "removed a broken symlink to {} to make {}",
            display(&nowhere),
            display(&tree.home(".config/nvim"))
        )),
        "the removal was not reported:\n{stderr}"
    );
    assert_eq!(link_target(&tree.home(".config/nvim/init.lua")), source);
}

/// Run `sync` against a manifest expected to fail while executing, and return
/// the diagnostic. Status 1: the command started work and stopped.
fn refused(tree: &Tree, manifest: &str) -> String {
    tree.write_manifest(manifest);
    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    stderr_of(&assertion)
}

/// Run `sync --no-overwrite` successfully and return its skip diagnostics.
fn skipped(tree: &Tree, manifest: &str) -> String {
    tree.write_manifest(manifest);
    let assertion = tree
        .batfiles()
        .args(["sync", "--no-overwrite"])
        .assert()
        .success();
    stderr_of(&assertion)
}

#[test]
fn a_destination_holding_a_file_is_named_and_left_when_skipped() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    fs::write(tree.home(".zshrc"), "mine\n").expect("an existing file");

    let stderr = skipped(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
    for expected in [display(&tree.home(".zshrc")), "a regular file".to_owned()] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(
        fs::read_to_string(tree.home(".zshrc")).expect("the file"),
        "mine\n"
    );
}

#[test]
fn a_destination_holding_a_directory_is_named_and_left_when_skipped() {
    let tree = Tree::new();
    tree.repo_file("nvim/init.lua", "-- nvim\n");
    fs::create_dir(tree.home(".config")).expect("an existing directory");
    fs::write(tree.home(".config/theirs"), "mine\n").expect("a file inside it");

    let stderr = skipped(&tree, &one_symlink("nvim/init.lua", "~/.config"));
    for expected in [display(&tree.home(".config")), "a directory".to_owned()] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(
        fs::read_to_string(tree.home(".config/theirs")).expect("the file"),
        "mine\n"
    );
}

/// Create a FIFO at `path` for unsupported-node tests.
fn mkfifo(path: &Path) {
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("mkfifo(1) is POSIX and this module is unix-only");
    assert!(status.success(), "mkfifo {} failed", path.display());
}

#[test]
fn a_destination_that_is_neither_file_directory_nor_link_is_named_when_skipped() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    let fifo = tree.home(".zshrc");
    mkfifo(&fifo);

    let stderr = skipped(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
    assert!(
        stderr.contains("neither a regular file"),
        "the node found there was not described:\n{stderr}"
    );
    assert!(fifo.exists(), "the fifo was removed");
}

#[test]
fn a_destination_holding_a_link_out_of_the_repository_is_not_ours() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    let elsewhere = tree.path("elsewhere");
    // The target must exist; a broken link would be replaceable.
    fs::write(&elsewhere, "someone else's\n").expect("the target");
    std::os::unix::fs::symlink(&elsewhere, tree.home(".zshrc")).expect("an unmanaged link");

    let stderr = skipped(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
    let expected = format!(
        "a symlink to {}, which is outside the repository",
        display(&elsewhere)
    );
    for expected in [display(&tree.home(".zshrc")), expected] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(link_target(&tree.home(".zshrc")), elsewhere);
}

#[test]
fn a_link_in_the_way_is_named_as_written_and_as_it_resolves() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    // An existing target ensures this tests the refusal message for a live link.
    fs::write(tree.path("elsewhere"), "someone else's\n").expect("the target");
    std::os::unix::fs::symlink("../elsewhere", tree.home(".zshrc")).expect("an unmanaged link");

    let stderr = skipped(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
    for expected in ["../elsewhere".to_owned(), display(&tree.path("elsewhere"))] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(
        link_target(&tree.home(".zshrc")),
        PathBuf::from("../elsewhere")
    );
}

#[test]
fn a_source_the_repository_does_not_have_is_refused() {
    let tree = Tree::new();
    let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
    assert!(
        stderr.contains(&display(&tree.path("repo").join("shell/zshrc"))),
        "the source was not named:\n{stderr}"
    );
    assert!(
        !tree.home(".zshrc").is_symlink(),
        "a dangling link was made"
    );
}

// `symlink-dir`: one link per direct child of a directory, all of them in
// one destination directory.

/// Create source files and a populated directory for child-installation tests.
fn with_children(tree: &Tree) {
    tree.repo_file("files/zshrc", "# zsh\n");
    tree.repo_file("files/ackrc", "--smart-case\n");
    tree.repo_file("files/config/starship.toml", "# prompt\n");
}

#[test]
fn a_symlink_dir_action_links_every_direct_child() {
    let tree = Tree::new();
    with_children(&tree);
    tree.write_manifest(&one_symlink_dir("files", "~/installed", false));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        entries(&tree.home("installed")),
        ["ackrc", "config", "zshrc"]
    );
    for child in ["ackrc", "config", "zshrc"] {
        assert_eq!(
            link_target(&tree.home(&format!("installed/{child}"))),
            tree.path("repo").join("files").join(child),
            "`{child}` does not point into the repository"
        );
    }
}

#[test]
fn a_directory_child_is_one_link_with_its_contents_reached_through_it() {
    let tree = Tree::new();
    with_children(&tree);
    tree.write_manifest(&one_symlink_dir("files", "~/installed", false));

    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home("installed/config").is_symlink());
    assert_eq!(
        fs::read_to_string(tree.home("installed/config/starship.toml"))
            .expect("the directory link resolves"),
        "# prompt\n"
    );

    tree.repo_file("files/config/added-later.toml", "# later\n");
    assert!(tree.home("installed/config/added-later.toml").exists());
}

#[test]
fn dot_prefix_dots_every_installed_name_and_nothing_else() {
    let tree = Tree::new();
    with_children(&tree);
    tree.write_manifest(&one_symlink_dir("files", "~", true));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(entries(&tree.path("home")), [".ackrc", ".config", ".zshrc"]);
    assert_eq!(
        link_target(&tree.home(".zshrc")),
        tree.path("repo").join("files/zshrc")
    );
}

/// A `symlink-dir` over `files` into `~/installed`, with filter fields appended.
fn filtered_symlink_dir(filters: &str) -> String {
    format!(
        "{}{filters}\n",
        one_symlink_dir("files", "~/installed", false)
    )
}

#[test]
fn include_links_only_the_children_it_matches() {
    let tree = Tree::new();
    with_children(&tree);
    tree.write_manifest(&filtered_symlink_dir(r#"include = ["*rc", "config"]"#));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        entries(&tree.home("installed")),
        ["ackrc", "config", "zshrc"]
    );

    tree.write_manifest(&filtered_symlink_dir(r#"include = "z*""#));
    fs::remove_dir_all(tree.home("installed")).expect("a fresh destination");
    tree.batfiles().arg("sync").assert().success();
    assert_eq!(entries(&tree.home("installed")), ["zshrc"]);
}

#[test]
fn exclude_leaves_out_the_children_it_matches_and_beats_include() {
    let tree = Tree::new();
    with_children(&tree);
    tree.repo_file("files/.hidden", "# dotted\n");
    tree.write_manifest(&filtered_symlink_dir(
        "include = \"*\"\nexclude = [\"config\", \"a*\"]",
    ));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(entries(&tree.home("installed")), [".hidden", "zshrc"]);
}

#[test]
fn a_filter_matches_the_source_name_before_dot_prefix() {
    let tree = Tree::new();
    with_children(&tree);
    tree.write_manifest(&format!(
        "{}include = \"zshrc\"\n",
        one_symlink_dir("files", "~", true)
    ));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(entries(&tree.path("home")), [".zshrc"]);
}

#[test]
fn a_pattern_that_matched_nothing_is_said_at_verbose_only() {
    let tree = Tree::new();
    with_children(&tree);
    tree.write_manifest(&filtered_symlink_dir(r#"exclude = ["config", "*.bak"]"#));

    let quiet = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    assert!(
        !stderr_of(&quiet).contains("matched nothing"),
        "{}",
        stderr_of(&quiet)
    );

    let verbose = tree
        .batfiles()
        .args(["--color", "never", "-v", "sync"])
        .assert()
        .success();
    let stderr = stderr_of(&verbose);
    assert!(
        stderr.contains(&format!(
            "exclude pattern `*.bak` matched nothing in {}",
            display(&tree.path("repo").join("files"))
        )),
        "{stderr}"
    );
    assert!(!stderr.contains("`config` matched nothing"), "{stderr}");
}

#[test]
fn filters_that_select_no_child_still_make_the_destination() {
    let tree = Tree::new();
    with_children(&tree);
    tree.write_manifest(&filtered_symlink_dir("include = []"));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "-v", "sync"])
        .assert()
        .success();
    assert!(tree.home("installed").is_dir());
    assert!(entries(&tree.home("installed")).is_empty());
    assert!(
        stderr_of(&assertion).contains("no selected children to link in"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_dry_run_reports_only_the_children_a_filter_selects() {
    let tree = Tree::new();
    with_children(&tree);
    tree.write_manifest(&filtered_symlink_dir(r#"exclude = "config""#));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync", "--dry-run"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);
    assert!(stderr.contains("would link"), "{stderr}");
    assert!(stderr.contains("installed/zshrc"), "{stderr}");
    assert!(!stderr.contains("installed/config"), "{stderr}");
    assert!(!tree.home("installed").exists());
}

#[test]
fn the_children_are_linked_in_a_stable_order() {
    let tree = Tree::new();
    for name in ["zshrc", "ackrc", "inputrc", "curlrc"] {
        tree.repo_file(&format!("files/{name}"), "# rc\n");
    }
    tree.write_manifest(&one_symlink_dir("files", "~", true));

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    let reported: Vec<String> = stderr_of(&assertion)
        .lines()
        .filter_map(|line| Some(line.strip_prefix("linked ")?.split(' ').next()?.to_owned()))
        .collect();
    let expected: Vec<String> = [".ackrc", ".curlrc", ".inputrc", ".zshrc"]
        .iter()
        .map(|name| display(&tree.home(name)))
        .collect();
    assert_eq!(reported, expected);
}

#[test]
fn an_existing_destination_directory_is_used_rather_than_refused() {
    let tree = Tree::new();
    with_children(&tree);
    fs::create_dir(tree.home("bin")).expect("an existing directory");
    fs::write(tree.home("bin/theirs"), "mine\n").expect("a file inside it");
    tree.write_manifest(&one_symlink_dir("files", "~/bin", false));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        entries(&tree.home("bin")),
        ["ackrc", "config", "theirs", "zshrc"]
    );
    assert_eq!(
        fs::read_to_string(tree.home("bin/theirs")).expect("the file"),
        "mine\n"
    );
}

#[test]
fn a_destination_directory_symlinked_elsewhere_is_followed() {
    let tree = Tree::new();
    with_children(&tree);
    let elsewhere = tree.path("elsewhere");
    fs::create_dir(&elsewhere).expect("a directory on another volume");
    std::os::unix::fs::symlink(&elsewhere, tree.home("bin")).expect("a deliberate link");
    tree.write_manifest(&one_symlink_dir("files", "~/bin", false));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(entries(&elsewhere), ["ackrc", "config", "zshrc"]);
    assert!(tree.home("bin").is_symlink(), "the link was replaced");
}

#[test]
fn a_symlink_dir_run_twice_changes_nothing() {
    let tree = Tree::new();
    with_children(&tree);
    tree.write_manifest(&one_symlink_dir("files", "~", true));
    tree.batfiles().arg("sync").assert().success();

    tree.batfiles().arg("sync").assert().success().stderr("");
    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);
    for name in [".ackrc", ".config", ".zshrc"] {
        let expected = format!("unchanged {}", display(&tree.home(name)));
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn an_empty_source_directory_still_makes_its_destination_and_links_nothing() {
    let tree = Tree::new();
    fs::create_dir_all(tree.path("repo/files")).expect("an empty source directory");
    tree.write_manifest(&one_symlink_dir("files", "~/installed", false));

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
        stderr_of(&assertion).contains("no children to link"),
        "the empty directory was not reported:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_child_that_is_already_a_dotfile_is_refused_under_dot_prefix() {
    let tree = Tree::new();
    with_children(&tree);
    tree.repo_file("files/.hidden", "# oops\n");

    let stderr = refused(&tree, &one_symlink_dir("files", "~", true));
    for expected in [".hidden", "..hidden"] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert!(
        !tree.home("..hidden").exists(),
        "the doubly-dotted name was installed anyway"
    );
}

#[test]
fn a_source_directory_that_is_not_a_directory_is_refused() {
    let tree = Tree::new();
    with_children(&tree);

    let stderr = refused(&tree, &one_symlink_dir("files/zshrc", "~/installed", false));
    for expected in [
        "not a directory".to_owned(),
        display(&tree.path("repo/files/zshrc")),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn a_source_directory_the_repository_does_not_have_is_refused() {
    let tree = Tree::new();
    let stderr = refused(&tree, &one_symlink_dir("files", "~/installed", false));
    assert!(
        stderr.contains(&display(&tree.path("repo/files"))),
        "the source was not named:\n{stderr}"
    );
}

#[test]
fn a_destination_directory_holding_a_file_is_named_when_skipped() {
    let tree = Tree::new();
    with_children(&tree);
    fs::write(tree.home("bin"), "mine\n").expect("an existing file");

    let stderr = skipped(&tree, &one_symlink_dir("files", "~/bin", false));
    for expected in [display(&tree.home("bin")), "a regular file".to_owned()] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(
        fs::read_to_string(tree.home("bin")).expect("the file"),
        "mine\n"
    );
}

#[test]
fn a_destination_directory_that_is_a_broken_link_is_replaced_and_said_so() {
    let tree = Tree::new();
    with_children(&tree);
    let nowhere = tree.path("nowhere");
    std::os::unix::fs::symlink(&nowhere, tree.home("bin")).expect("a broken link");
    tree.write_manifest(&one_symlink_dir("files", "~/bin", false));

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);
    for expected in [
        format!(
            "removed a broken symlink to {} to make {}",
            display(&nowhere),
            display(&tree.home("bin"))
        ),
        format!("created {}", display(&tree.home("bin"))),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert!(tree.home("bin").is_dir(), "the directory was not created");
    assert_eq!(entries(&tree.home("bin")), ["ackrc", "config", "zshrc"]);
}

#[test]
fn a_broken_destination_directory_is_replaced_wherever_it_pointed() {
    let tree = Tree::new();
    with_children(&tree);
    let inside = tree.path("repo/missing");
    std::os::unix::fs::symlink(&inside, tree.home("bin")).expect("a broken link");
    tree.write_manifest(&one_symlink_dir("files", "~/bin", false));

    tree.batfiles().arg("sync").assert().success();
    assert!(tree.home("bin").is_dir(), "the directory was not created");
    assert!(
        !tree.path("repo/missing").exists(),
        "the far end of the link was created"
    );
}

#[test]
fn a_broken_link_above_the_directory_being_made_is_cleared_too() {
    let tree = Tree::new();
    with_children(&tree);
    let nowhere = tree.path("nowhere");
    fs::create_dir(tree.home("a")).expect("an existing directory");
    std::os::unix::fs::symlink(&nowhere, tree.home("a/broken")).expect("a broken link");
    tree.write_manifest(&one_create_dir("~/a/broken/b/c"));

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "removed a broken symlink to {} to make {}",
            display(&nowhere),
            display(&tree.home("a/broken"))
        )),
        "the ancestor removal was not reported:\n{stderr}"
    );
    assert!(tree.home("a/broken/b/c").is_dir(), "nothing was created");
}

#[test]
fn a_link_reaching_nothing_through_a_file_is_replaceable_too() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    let through_a_file = tree.path("afile").join("nope");
    fs::write(tree.path("afile"), "not a directory\n").expect("a file");
    std::os::unix::fs::symlink(&through_a_file, tree.home(".zshrc")).expect("a broken link");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));

    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        link_target(&tree.home(".zshrc")),
        tree.path("repo/shell/zshrc")
    );
    assert_eq!(
        fs::read_to_string(tree.path("afile")).expect("the file"),
        "not a directory\n",
        "the file the link resolved through was touched"
    );
}

#[test]
fn a_skipped_child_destination_leaves_the_rest_of_the_action_to_run() {
    let tree = Tree::new();
    with_children(&tree);
    // `config` sorts between `ackrc` and `zshrc`.
    fs::create_dir(tree.home("installed")).expect("the destination directory");
    fs::write(tree.home("installed/config"), "mine\n").expect("an occupied child");

    let stderr = skipped(&tree, &one_symlink_dir("files", "~/installed", false));
    assert!(
        stderr.contains(&format!(
            "skipped {}: it is a regular file",
            display(&tree.home("installed/config"))
        )),
        "the child was not named:\n{stderr}"
    );
    for child in ["ackrc", "zshrc"] {
        assert!(
            tree.home("installed").join(child).is_symlink(),
            "`{child}` was not installed"
        );
    }
    assert_eq!(
        fs::read_to_string(tree.home("installed/config")).expect("the file"),
        "mine\n"
    );
}

#[test]
fn a_child_link_batfiles_owns_is_repaired() {
    let tree = Tree::new();
    with_children(&tree);
    let stale = tree.repo_file("files/zshrc.old", "# old\n");
    fs::create_dir(tree.home("installed")).expect("the destination directory");
    std::os::unix::fs::symlink(&stale, tree.home("installed/zshrc")).expect("a stale link");

    tree.write_manifest(&one_symlink_dir("files", "~/installed", false));
    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        link_target(&tree.home("installed/zshrc")),
        tree.path("repo").join("files/zshrc")
    );
    assert!(stale.exists(), "the old source was removed");
}

// Relative symlink targets resolve from the physical parent directory, including when that
// parent is reached through a symlink.

/// A home whose `~/bin` is a symlink to `~/.local/bin`, with the repository
/// at `~/dotfiles` holding `bin/tool`, and one existing link already at the
/// destination, spelled as given.
///
/// Returns the manifest to run and the physical path of that existing link.
fn through_an_aliased_parent(tree: &Tree, existing: &str) -> (String, PathBuf) {
    let repo = tree.home("dotfiles");
    fs::create_dir_all(repo.join("bin")).expect("a repository");
    fs::write(repo.join("bin/tool"), "#!/bin/sh\n# ours\n").expect("the source");

    // This is the target reached from the physical parent, rather than from `~/bin`.
    fs::create_dir_all(tree.home(".local/dotfiles/bin")).expect("a neighbour");
    fs::write(tree.home(".local/dotfiles/bin/tool"), "# theirs\n").expect("their file");
    fs::write(tree.home(".local/dotfiles/bin/other"), "# theirs\n").expect("their file");

    fs::create_dir_all(tree.home(".local/bin")).expect("the real directory");
    std::os::unix::fs::symlink(".local/bin", tree.home("bin")).expect("the alias");

    let link = tree.home(".local/bin/tool");
    std::os::unix::fs::symlink(existing, &link).expect("the existing link");

    fs::write(
        repo.join("batfiles.toml"),
        one_symlink_dir("bin", "~/bin", false),
    )
    .expect("a manifest");
    (display(&repo), link)
}

/// Run `sync --no-overwrite` against the supplied repository path.
fn sync_against(tree: &Tree, repo: &str) -> assert_cmd::assert::Assert {
    tree.batfiles()
        .args([
            "--color",
            "never",
            "--batfiles-dir",
            repo,
            "sync",
            "--no-overwrite",
        ])
        .assert()
}

#[test]
fn a_link_reached_through_an_aliased_parent_is_not_called_ours() {
    // From `~/bin` this target appears to reach the repository; from the physical parent
    // `~/.local/bin`, it reaches an unrelated directory.
    let tree = Tree::new();
    let (repo, link) = through_an_aliased_parent(&tree, "../dotfiles/bin/tool");

    let assertion = sync_against(&tree, &repo).success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&display(&tree.home(".local/dotfiles/bin/tool"))),
        "the skip did not say where the link really points:\n{stderr}"
    );
    assert_eq!(
        link_target(&link),
        PathBuf::from("../dotfiles/bin/tool"),
        "the unmanaged link was touched"
    );
}

#[test]
fn a_link_reached_through_an_aliased_parent_is_not_deleted_as_ours() {
    let tree = Tree::new();
    let (repo, link) = through_an_aliased_parent(&tree, "../dotfiles/bin/other");

    let assertion = sync_against(&tree, &repo).success();
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&display(&tree.home(".local/dotfiles/bin/other"))),
        "the skip did not say where the link really points:\n{stderr}"
    );
    assert_eq!(
        link_target(&link),
        PathBuf::from("../dotfiles/bin/other"),
        "an unmanaged link was destroyed"
    );
}

#[test]
fn a_correct_link_reached_through_an_aliased_parent_is_left_alone() {
    let tree = Tree::new();
    let (repo, link) = through_an_aliased_parent(&tree, "../../dotfiles/bin/tool");
    assert_eq!(
        fs::canonicalize(&link).expect("the link resolves"),
        fs::canonicalize(tree.home("dotfiles/bin/tool")).expect("the source"),
        "the fixture is wrong: this link should already be correct"
    );

    sync_against(&tree, &repo).success().stderr("");
    assert_eq!(
        link_target(&link),
        PathBuf::from("../../dotfiles/bin/tool"),
        "a correct link was rewritten"
    );
}

#[test]
fn a_repository_reached_through_a_symlink_still_converges() {
    let tree = Tree::new();
    let (_, _) = through_an_aliased_parent(&tree, "../dotfiles/bin/tool");
    fs::remove_file(tree.home(".local/bin/tool")).expect("start from nothing");

    std::os::unix::fs::symlink(tree.path("home"), tree.path("by-another-name"))
        .expect("a symlinked route to the home");
    let aliased = display(&tree.path("by-another-name/dotfiles"));

    sync_against(&tree, &aliased).success();
    sync_against(&tree, &aliased).success().stderr("");
}

/// Expected `(source, destination)` links from the leaf fixture, in installation order.
const LEAF_LINKS: [(&str, &str); 10] = [
    ("shell/zshrc", ".zshrc"),
    ("shell/zshenv", ".zshenv"),
    ("shell/aliases.zsh", ".config/zsh/aliases.zsh"),
    ("git/gitconfig", ".gitconfig"),
    ("git/gitignore", ".config/git/ignore"),
    ("editor/nvim", ".config/nvim"),
    ("bin/batgrep", ".local/bin/batgrep"),
    ("files/ackrc", ".ackrc"),
    ("files/curlrc", ".curlrc"),
    ("files/inputrc", ".inputrc"),
];

#[test]
fn syncing_a_real_repository_installs_every_action_and_nothing_else() {
    let tree = Tree::fixture("leaf");
    tree.batfiles().arg("sync").assert().success();

    assert_leaf_portable_actions(&tree);

    for (source, dest) in LEAF_LINKS {
        assert_eq!(
            link_target(&tree.home(dest)),
            tree.path("repo").join(source),
            "`{dest}` does not point at `{source}`"
        );
    }

    assert!(
        fs::read_to_string(tree.home(".config/nvim/lua/plugins.lua"))
            .expect("the directory link resolves")
            .contains("vim-fugitive")
    );

    // Check that copying the fixture preserved its executable mode.
    use std::os::unix::fs::PermissionsExt;
    let installed = fs::metadata(tree.home(".local/bin/batgrep")).expect("the link resolves");
    assert!(
        installed.permissions().mode() & 0o111 != 0,
        "`bin/batgrep` installed as a file nobody can run"
    );

    assert_eq!(
        entries(&tree.path("home")),
        [
            ".ackrc",
            ".cache",
            ".config",
            ".curlrc",
            ".gitconfig",
            ".inputrc",
            ".local",
            ".zshenv",
            ".zshrc"
        ]
    );
}

/// Create the personal leaf fixture with an inclusion of the corporate remote.
fn personal_composed_with(origin: &BareRepo) -> Tree {
    let tree = Tree::fixture("leaf");
    let personal = fs::read_to_string(tree.manifest()).expect("the fixture manifest");
    tree.write_manifest(&format!(
        r#"[remotes.corporate]
type = "git"
url = "{}"

{personal}
[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
"#,
        display(&origin.origin())
    ));
    tree
}

#[test]
fn syncing_a_composed_repository_assembles_the_personal_and_corporate_halves() {
    let origin = BareRepo::from_fixture("corporate");
    let tree = personal_composed_with(&origin);

    tree.batfiles().arg("sync").assert().success();

    assert_leaf_portable_actions(&tree);
    for (source, dest) in LEAF_LINKS {
        assert_eq!(
            link_target(&tree.home(dest)),
            tree.path("repo").join(source),
            "`{dest}` does not point at `{source}`"
        );
    }

    let materialized = tree.path("repo").join("remotes/corporate");
    for (source, dest) in [
        ("files/zshrc", ".zshrc.corporate"),
        ("files/p10k.zsh", ".p10k.zsh"),
    ] {
        assert_eq!(
            link_target(&tree.home(dest)),
            materialized.join(source),
            "`{dest}` was not linked into the remote's tree"
        );
    }
    for name in ["gitconfig", "npmrc"] {
        let seeded = tree.home(".config/corporate").join(name);
        assert!(
            !seeded.is_symlink(),
            "`{name}` was linked rather than seeded"
        );
        assert_eq!(
            fs::read_to_string(&seeded).unwrap_or_else(|error| panic!("`{name}`: {error}")),
            fs::read_to_string(materialized.join("seed").join(name)).expect("the remote's file"),
            "`{name}` does not hold what the remote published"
        );
    }

    assert_eq!(
        entries(&tree.path("home")),
        [
            ".ackrc",
            ".cache",
            ".config",
            ".curlrc",
            ".gitconfig",
            ".inputrc",
            ".local",
            ".p10k.zsh",
            ".zshenv",
            ".zshrc",
            ".zshrc.corporate"
        ]
    );
}

#[test]
fn a_composed_repository_converges_on_a_second_sync() {
    let origin = BareRepo::from_fixture("corporate");
    let tree = personal_composed_with(&origin);

    tree.batfiles().arg("sync").assert().success();
    tree.batfiles().arg("sync").assert().success().stderr("");
}

#[test]
fn an_occupied_destination_is_backed_up_and_the_run_goes_on() {
    let tree = Tree::fixture("leaf");
    let occupied = "[user]\n\temail = mine\n";
    fs::write(tree.home(".gitconfig"), occupied).expect("an existing file");

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);
    let backup = backup_of(&tree.home(".gitconfig"));
    assert_eq!(fs::read_to_string(&backup).expect("the backup"), occupied);
    assert!(
        stderr.contains(&format!(
            "backed up {} to {}",
            display(&tree.home(".gitconfig")),
            display(&backup)
        )),
        "the backup was not reported:\n{stderr}"
    );

    assert_leaf_portable_actions(&tree);
    for (_, dest) in &LEAF_LINKS {
        assert!(tree.home(dest).is_symlink(), "`{dest}` was not installed");
    }
}
