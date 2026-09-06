//! Executing symlink actions.
//!
//! Gated as a whole by its declaration in `main.rs`: where batfiles cannot make
//! a symlink it refuses the action before resolving anything, so none of these
//! has a meaningful non-unix form, and several build their fixtures with
//! `symlink` themselves. An action type that is not platform-specific does not
//! belong here.

use std::fs;
use std::path::{Path, PathBuf};

use crate::support::*;

#[test]
fn a_symlink_action_parses_with_every_field_it_accepts() {
    // The counterpart to the rejections outside this module: a record
    // spelling out every field `symlink` takes is accepted and carried out.
    let tree = Tree::new();
    tree.repo_file("files/zshrc", "# zsh\n");
    tree.write_manifest(
        "[[actions]]\n\
         type = \"symlink\"\n\
         id = \"zshrc\"\n\
         group = \"shell\"\n\
         source = \"files/zshrc\"\n\
         dest = \"~/.zshrc\"\n",
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
    // Repointing a link batfiles would have made loses nothing: the link holds
    // no content of its own, and the file it pointed at is untouched.
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
    // The spelling decides nothing: this one resolves into the repository,
    // so it is repairable, and the replacement is written anchored like any
    // other. The neighbouring cases cover a relative link that is already
    // right and an absolute one that is wrong.
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
    // A symlink stores the target it is handed, and a relative one is read back
    // from the link's own directory — not from wherever batfiles was run. So a
    // relative root has to be anchored before it is written into a link, or the
    // command reports success and leaves something pointing nowhere.
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
    // Someone may well have written this link by hand, or with a tool that
    // spells targets relatively. It points where the action asks, so there is
    // nothing to do.
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
fn a_link_that_only_looks_like_it_points_into_the_repository_is_refused() {
    // `<repo>/../outside` starts with the repository when compared as text and
    // leaves it when resolved. Reading the spelling rather than the destination
    // would delete a link batfiles never made.
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    let escaping = tree.path("repo").join("../outside");
    // The target has to exist: a link reaching nothing is replaceable
    // whatever it names, so a missing one would prove the wrong thing.
    fs::write(tree.path("outside"), "someone else's\n").expect("the target");
    std::os::unix::fs::symlink(&escaping, tree.home(".zshrc")).expect("an escaping link");

    let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
    assert!(
        stderr.contains(&display(&tree.home(".zshrc"))),
        "the destination was not named:\n{stderr}"
    );
    assert_eq!(link_target(&tree.home(".zshrc")), escaping);
}

#[test]
fn a_broken_link_at_a_destination_is_replaced_wherever_it_pointed() {
    // Rule 13 protects data, and a link reaching nothing gives access to
    // none — so unlike a link that leaves the repository and lands on
    // something, this one is batfiles' to repoint.
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
    // A repository may name a source that is itself a broken link — it is
    // there, and linking at it is what was asked for. The destination then
    // reaches nothing either, so "already right" has to be decided before
    // "broken", or every run relinks a link that is correct.
    let tree = Tree::new();
    fs::create_dir(tree.path("repo/shell")).expect("a source directory");
    std::os::unix::fs::symlink("nowhere", tree.path("repo/shell/zshrc")).expect("a broken source");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));
    tree.batfiles().arg("sync").assert().success();

    tree.batfiles().arg("sync").assert().success().stderr("");
}

#[test]
fn a_symlink_inside_its_source_is_refused_even_over_a_link_it_could_repair() {
    // The refusal has to cover repairing as well as creating. A destination
    // inside the source that already holds a replaceable link takes the
    // repair arm, and a check that only guards the vacant one lets exactly
    // this case through — writing into the repository after removing what
    // was there.
    let tree = Tree::new();
    let repo = seeded_repository_in_the_home(&tree);
    // Broken, so it is batfiles' to replace and the repair arm is reached.
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
    // Refused before the old link was removed, so nothing was destroyed on
    // the way to failing.
    assert_eq!(
        link_target(&repo.join("seed/inner")),
        tree.path("nowhere"),
        "the link was replaced despite the refusal"
    );
}

#[test]
fn a_symlink_whose_destination_is_inside_its_source_is_refused() {
    // Linking a directory into itself means nothing, and making the link
    // would write into the repository — which is the one place `sync` never
    // writes. Refused on the same terms as the copy actions, by where the
    // two paths resolve rather than by how they are spelled.
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
    // The same hazard one level up, and worse than for a single link: the
    // destination directory is created before the children are enumerated,
    // so it would be among them and would be linked into itself.
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
    // And it found out before creating the destination directory, which is
    // what would have put it among the children.
    assert_eq!(entries(&repo.join("seed")), ["a"]);
}

#[test]
fn a_link_that_is_already_right_is_left_alone() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    tree.write_manifest(&one_symlink("shell/zshrc", "~/.zshrc"));
    tree.batfiles().arg("sync").assert().success();

    // A converged repository is the common case, so it says nothing at all —
    // and `-v` is how you check that it looked.
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
    // A destination under a regular file reads as vacant — nothing is at
    // it — so the refusal falls to whoever makes the parents, which is the
    // step that can say *which* component is the problem. Reporting the
    // kernel's answer where it arose would name the path below the file.
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    fs::write(tree.home(".config"), "not a directory\n").expect("a file in the way");

    let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.config/zsh/zshrc"));
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
    // The parent is made, because the link that was there reached nothing.
    // Silently is the one way it must not happen: a run that removes a node
    // says so, even one it is entitled to remove.
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

#[test]
fn a_destination_holding_a_file_is_refused_and_the_file_is_left() {
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    fs::write(tree.home(".zshrc"), "mine\n").expect("an existing file");

    let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
    for expected in [display(&tree.home(".zshrc")), "a regular file".to_owned()] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(
        fs::read_to_string(tree.home(".zshrc")).expect("the file"),
        "mine\n"
    );
}

#[test]
fn a_destination_holding_a_directory_is_refused_and_the_directory_is_left() {
    let tree = Tree::new();
    tree.repo_file("nvim/init.lua", "-- nvim\n");
    fs::create_dir(tree.home(".config")).expect("an existing directory");
    fs::write(tree.home(".config/theirs"), "mine\n").expect("a file inside it");

    let stderr = refused(&tree, &one_symlink("nvim/init.lua", "~/.config"));
    for expected in [display(&tree.home(".config")), "a directory".to_owned()] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(
        fs::read_to_string(tree.home(".config/theirs")).expect("the file"),
        "mine\n"
    );
}

/// A destination that is none of the three node types batfiles reasons
/// about.
///
/// A fifo rather than a unix socket: binding one needs `socket(2)`, which a
/// restricted runner may refuse, and caps the path at the length of
/// `sun_path`, which a long `TMPDIR` exceeds on its own. `mkfifo` is an
/// ordinary filesystem call in a directory the suite already writes to.
fn mkfifo(path: &Path) {
    let status = std::process::Command::new("mkfifo")
        .arg(path)
        .status()
        .expect("mkfifo(1) is POSIX and this module is unix-only");
    assert!(status.success(), "mkfifo {} failed", path.display());
}

#[test]
fn a_destination_that_is_neither_file_directory_nor_link_is_refused() {
    // Whatever this is, batfiles has no way to give it back, which is the
    // whole of rule 13's reasoning.
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    let fifo = tree.home(".zshrc");
    mkfifo(&fifo);

    let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
    assert!(
        stderr.contains("neither a regular file"),
        "the node found there was not described:\n{stderr}"
    );
    assert!(fifo.exists(), "the fifo was removed");
}

#[test]
fn a_destination_holding_a_link_out_of_the_repository_is_refused() {
    // Someone else made this link, and where it points is not batfiles' to
    // decide.
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    let elsewhere = tree.path("elsewhere");
    // The target has to exist: a link reaching nothing is replaceable
    // whatever it names, so a missing one would prove the wrong thing.
    fs::write(&elsewhere, "someone else's\n").expect("the target");
    std::os::unix::fs::symlink(&elsewhere, tree.home(".zshrc")).expect("an unmanaged link");

    let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
    // An absolute target resolves to itself, so it is named once and not
    // reported as though two paths were involved.
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
fn a_refused_link_is_named_as_written_and_as_it_resolves() {
    // The spelling is what the user will see from `ls`; the resolved path is
    // what the refusal was decided on. A relative target is where the two
    // differ, and neither alone explains the other.
    let tree = Tree::new();
    tree.repo_file("shell/zshrc", "# zsh\n");
    // Reachable, so that what is under test is how the refusal names the
    // link rather than whether it is refused at all.
    fs::write(tree.path("elsewhere"), "someone else's\n").expect("the target");
    std::os::unix::fs::symlink("../elsewhere", tree.home(".zshrc")).expect("an unmanaged link");

    let stderr = refused(&tree, &one_symlink("shell/zshrc", "~/.zshrc"));
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
    // A link to nothing is silent breakage, and the repository not containing
    // what it names is a mistake batfiles can see.
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

/// A repository holding `files/{ackrc,zshrc}` and a directory child
/// `files/config/` with something inside it, which is the shape every case
/// below reasons about.
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

    // The destination directory did not exist, and holds exactly the three
    // children — no more, and nothing renamed.
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
    // Decision 3: every direct child becomes exactly one symlink, whatever
    // it is. Nothing descends, so a file added under `files/config/` later
    // appears without the manifest or a further sync mentioning it.
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

    // Added after the sync, and reachable with no second run.
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
    // The name in the repository is undotted, which is the point: a
    // repository reads better without a tree of dot-files in it.
    assert_eq!(
        link_target(&tree.home(".zshrc")),
        tree.path("repo").join("files/zshrc")
    );
}

#[test]
fn the_children_are_linked_in_a_stable_order() {
    // `read_dir` yields whatever order the filesystem holds. An action that
    // reports its work differently on every machine is one nobody can diff.
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
    // `dest-dir` is the container the links go in, not a node the action
    // installs, so finding one already there is the ordinary case — and it
    // may hold things batfiles did not put there.
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
    // Unlike a destination, which is judged without following a final
    // link, `dest-dir` is resolved: someone whose `~/.config` lives on
    // another volume put that link there deliberately.
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
    // Not an error: a directory that is empty today is a repository in
    // progress, not a manifest that cannot be honored. The destination is
    // made anyway — it is what the action was told to fill, and `create-dir`
    // makes exactly that directory when a manifest asks for it outright —
    // and saying so is what keeps a run that changed the home from being
    // silent about it.
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
    // `..hidden` is a legal file name and never the one that was meant, so
    // the mistake is named rather than installed.
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
    // There are no children to link, and linking the file itself is what a
    // `symlink` action is for.
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
fn a_destination_directory_holding_a_file_is_refused() {
    let tree = Tree::new();
    with_children(&tree);
    fs::write(tree.home("bin"), "mine\n").expect("an existing file");

    let stderr = refused(&tree, &one_symlink_dir("files", "~/bin", false));
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
    // Nothing resolves there, so the directory looks absent — and creating
    // it fails with a bare `EEXIST` naming nothing unless the link is
    // cleared first. It holds nothing, so clearing it destroys nothing.
    let tree = Tree::new();
    with_children(&tree);
    let nowhere = tree.path("nowhere");
    std::os::unix::fs::symlink(&nowhere, tree.home("bin")).expect("a broken link");
    tree.write_manifest(&one_symlink_dir("files", "~/bin", false));

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();
    let stderr = stderr_of(&assertion);
    // The removal is its own line, and comes first: it is the part the user
    // may need to act on, and it is true of a path they did not name.
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
    // Where a broken link points decides nothing: it reaches no content
    // either way, so one naming a path inside the repository is cleared on
    // the same terms as one naming a path outside it.
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
    // The link is at an ancestor nobody named, so a single `mkdir -p` hits
    // it and reports `EEXIST` against the path that does *not* exist. Each
    // level is asked the same question the named directory is, so the link
    // is found where it actually is and the removal names that path.
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
    // `<some-file>/child` resolves nowhere, but the kernel says so with
    // `ENOTDIR` rather than `ENOENT`. Reading only the second calls this
    // link someone else's data and refuses a destination holding nothing.
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
fn an_occupied_child_destination_stops_the_action_where_it_stands() {
    // Rule 13 inside one action. Partial application within a
    // `symlink-dir` is the same story as partial application across a
    // manifest: earlier children stay, later ones are not attempted.
    let tree = Tree::new();
    with_children(&tree);
    // `config` sorts between `ackrc` and `zshrc`, so one child is installed
    // before the refusal and one is never reached.
    fs::create_dir(tree.home("installed")).expect("the destination directory");
    fs::write(tree.home("installed/config"), "mine\n").expect("an occupied child");

    let stderr = refused(&tree, &one_symlink_dir("files", "~/installed", false));
    assert!(
        stderr.contains(&display(&tree.home("installed/config"))),
        "the child was not named:\n{stderr}"
    );
    assert!(
        tree.home("installed/ackrc").is_symlink(),
        "the child before the refusal was rolled back"
    );
    assert!(
        fs::symlink_metadata(tree.home("installed/zshrc")).is_err(),
        "a child after the refusal was installed"
    );
    assert_eq!(
        fs::read_to_string(tree.home("installed/config")).expect("the file"),
        "mine\n"
    );
}

#[test]
fn a_child_link_batfiles_owns_is_repaired() {
    // The same rule `symlink` follows, reached through the same code: a
    // link pointing elsewhere in the repository holds no content of its
    // own, so repointing it loses nothing.
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

// A destination reached through a symlinked parent. `~/bin -> ~/.local/bin`
// is an ordinary arrangement, and a relative link sitting in it is read by
// the operating system from the directory it is *physically* in. Composing
// that answer from the written path instead classifies the link against a
// directory it is not in, and every case below is a way for that to go
// wrong (`guidance.md`, rule 14).

/// A home whose `~/bin` is a symlink to `~/.local/bin`, with the repository
/// at `~/dotfiles` holding `bin/tool`, and one existing link already at the
/// destination, spelled as given.
///
/// Returns the manifest to run and the physical path of that existing link.
fn through_an_aliased_parent(tree: &Tree, existing: &str) -> (String, PathBuf) {
    let repo = tree.home("dotfiles");
    fs::create_dir_all(repo.join("bin")).expect("a repository");
    fs::write(repo.join("bin/tool"), "#!/bin/sh\n# ours\n").expect("the source");

    // What a relative link in `~/.local/bin` reaches by climbing out of it,
    // which is not what the same spelling reaches from `~/bin`.
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

/// `sync` against a repository that is not the tree's default one.
fn sync_against(tree: &Tree, repo: &str) -> assert_cmd::assert::Assert {
    tree.batfiles()
        .args(["--color", "never", "--batfiles-dir", repo, "sync"])
        .assert()
}

#[test]
fn a_link_reached_through_an_aliased_parent_is_not_called_ours() {
    // Read from `~/bin`, `../dotfiles/bin/tool` looks like `~/dotfiles`.
    // Read from `~/.local/bin`, where the link actually is, it is
    // `~/.local/dotfiles` — someone else's. Judging it lexically reported
    // the repository as installed while `~/bin/tool` ran the wrong program,
    // and said nothing at all, which is the worst way to be wrong.
    let tree = Tree::new();
    let (repo, link) = through_an_aliased_parent(&tree, "../dotfiles/bin/tool");

    let assertion = sync_against(&tree, &repo).failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&display(&tree.home(".local/dotfiles/bin/tool"))),
        "the refusal did not say where the link really points:\n{stderr}"
    );
    assert_eq!(
        link_target(&link),
        PathBuf::from("../dotfiles/bin/tool"),
        "the unmanaged link was touched"
    );
}

#[test]
fn a_link_reached_through_an_aliased_parent_is_not_deleted_as_ours() {
    // The same misreading, one step further: here the lexical answer is
    // inside the repository but not what the action wants, so the link was
    // deleted and replaced rather than merely mistaken for correct.
    let tree = Tree::new();
    let (repo, link) = through_an_aliased_parent(&tree, "../dotfiles/bin/other");

    let assertion = sync_against(&tree, &repo).failure().code(1);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&display(&tree.home(".local/dotfiles/bin/other"))),
        "the refusal did not say where the link really points:\n{stderr}"
    );
    assert_eq!(
        link_target(&link),
        PathBuf::from("../dotfiles/bin/other"),
        "an unmanaged link was destroyed"
    );
}

#[test]
fn a_correct_link_reached_through_an_aliased_parent_is_left_alone() {
    // The other direction of the same misreading, and the one a user hits
    // by doing everything right: this link resolves to exactly what the
    // action installs, and was refused as pointing outside the repository.
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
    // The other half of resolving what is already there: the repository
    // root has to be compared in the same space. Selected through a
    // symlink, its written form and its resolved form are the same place by
    // two names — and comparing across the two calls every freshly written
    // link stale, relinking the whole repository on every run and never
    // reaching a quiet one. A `/home` that is a symlink is enough to do it.
    let tree = Tree::new();
    let (_, _) = through_an_aliased_parent(&tree, "../dotfiles/bin/tool");
    fs::remove_file(tree.home(".local/bin/tool")).expect("start from nothing");

    std::os::unix::fs::symlink(tree.path("home"), tree.path("by-another-name"))
        .expect("a symlinked route to the home");
    let aliased = display(&tree.path("by-another-name/dotfiles"));

    sync_against(&tree, &aliased).success();
    // Quiet: everything it just wrote is recognised as already right.
    sync_against(&tree, &aliased).success().stderr("");
}

// The `leaf` fixture: several actions over a directory tree, as opposed to
// the manifests above, which are written inline to isolate one rule each.
// Only its links are in here; the half that needs no symlink is outside,
// with the tests that can run anywhere.

/// Every link `tests/fixtures/leaf` installs, in the order it installs
/// them, all of them after the leaf directories and seeds.
///
/// Written out rather than read back from the manifest: a test that derives
/// its expectations from the file under test asserts nothing. The last
/// three are the one `symlink-dir` action expanded — one entry per child,
/// dotted and in sorted order, because that is what the run produces.
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

    // A link to a directory is only worth making if what is under it reads
    // back, and `lua/plugins.lua` is reachable no other way.
    assert!(
        fs::read_to_string(tree.home(".config/nvim/lua/plugins.lua"))
            .expect("the directory link resolves")
            .contains("vim-fugitive")
    );

    // The link is only a usable command if what it reaches is executable,
    // which is a property of the repository rather than of batfiles — so
    // this is here to keep the fixture honest about being one someone
    // keeps, and it fails if the mode is lost getting the fixture in place.
    use std::os::unix::fs::PermissionsExt;
    let installed = fs::metadata(tree.home(".local/bin/batgrep")).expect("the link resolves");
    assert!(
        installed.permissions().mode() & 0o111 != 0,
        "`bin/batgrep` installed as a file nobody can run"
    );

    // Only `batfiles.toml` has intrinsic meaning: `README.md` is a file it
    // never names, so nothing of it reaches the home on its own. Every file
    // the installed configuration refers to does — `.zshrc` sources the
    // other two shell files, everything the `copy-dir` seeds, and a history
    // file in the directory the `create-dir` makes, and a fixture whose
    // shell would fail to start is not one anybody would keep.
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

#[test]
fn an_occupied_destination_stops_the_run_where_it_stands() {
    // Rule 13 at repository scale. What one action's worth of it cannot
    // show is what happens to the rest of the list: the run stops, so the
    // actions after the refusal are not attempted. The other half of the
    // guarantee — which of two actions runs first, where one depends on the
    // other — is `actions::the_first_of_two_seeds_naming_one_destination_is_
    // the_one_that_lands`.
    let tree = Tree::fixture("leaf");
    let occupied = "[user]\n\temail = mine\n";
    fs::write(tree.home(".gitconfig"), occupied).expect("an existing file");
    // Found rather than counted, so an action added to the fixture ahead of
    // this one does not silently move the two halves of the assertion.
    let refused = LEAF_LINKS
        .iter()
        .position(|(_, dest)| *dest == ".gitconfig")
        .expect("the occupied destination is one the fixture declares");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);
    for expected in [
        display(&tree.home(".gitconfig")),
        "a regular file".to_owned(),
    ] {
        assert!(stderr.contains(&expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(
        fs::read_to_string(tree.home(".gitconfig")).expect("the file"),
        occupied
    );

    // Everything ahead of the refusal, links and seeds alike: the whole
    // portable half is declared before the first link, so it is all of it
    // before this one.
    assert_leaf_portable_actions(&tree);
    for (_, dest) in &LEAF_LINKS[..refused] {
        assert!(tree.home(dest).is_symlink(), "`{dest}` was not installed");
    }
    for (_, dest) in &LEAF_LINKS[refused + 1..] {
        // Not `exists`, which follows the link and would call a dangling
        // one absent.
        assert!(
            fs::symlink_metadata(tree.home(dest)).is_err(),
            "`{dest}` was installed after the refusal"
        );
    }
}
