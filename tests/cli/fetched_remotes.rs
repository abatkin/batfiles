//! CLI tests for file/archive remote fetching, refresh, ownership, and source references.
//! Local-server request counts verify skipped downloads.

use std::fs;
use std::path::PathBuf;

use crate::support::*;

/// The body of the file remote.
const PATHOGEN: &str = "\" pathogen.vim\ncall pathogen#infect()\n";

/// A later version of it.
const PATHOGEN_2: &str = "\" pathogen.vim, version two\n";

/// A tool published as a release tarball under one versioned directory.
const FZF: &[Member] = &[
    Member::Directory("fzf-0.1.0", 0o755),
    Member::Directory("fzf-0.1.0/bin", 0o755),
    Member::File("fzf-0.1.0/bin/fzf", 0o755, "#!/bin/sh\necho fzf\n"),
];

/// The paths the server here answers.
fn server() -> Server {
    Server::new(&[
        ("/pathogen.vim", Reply::Body(PATHOGEN)),
        ("/pathogen-2.vim", Reply::Body(PATHOGEN_2)),
        ("/fzf.tar.gz", Reply::Bytes(tarball(FZF))),
    ])
}

/// A manifest declaring the file remote `pathogen` at `path` on `server`, with
/// `extra` fields, and linking it into the home.
fn linking_a_file(server: &Server, path: &str, extra: &str) -> String {
    format!(
        r#"[remotes.pathogen]
type = "file"
url = "{}{path}"
{extra}
[[actions]]
type = "symlink"
source = "@pathogen"
dest = "~/.vim/autoload/pathogen.vim"
"#,
        server.address()
    )
}

/// A manifest declaring the archive remote `fzf` and linking its executable.
fn linking_an_archive(server: &Server) -> String {
    format!(
        r#"[remotes.fzf]
type = "archive"
url = "{}/fzf.tar.gz"
archive-root = "*"

[[actions]]
type = "symlink"
source = "@fzf/bin/fzf"
dest = "~/bin/fzf"
"#,
        server.address()
    )
}

/// Where the remote `id` is materialized.
fn materialization(tree: &Tree, id: &str) -> PathBuf {
    tree.path("repo").join("remotes").join(id)
}

/// The stamp recording what that materialization was fetched from.
fn stamp(tree: &Tree, id: &str) -> PathBuf {
    tree.path("repo")
        .join("remotes")
        .join(format!("{id}.batfiles-source"))
}

#[test]
fn a_file_remote_is_the_file_and_is_named_whole() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_a_file(&server, "/pathogen.vim", ""));

    let assertion = tree.batfiles().arg("sync").assert().success();
    let stderr = stderr_of(&assertion);

    let fetched = materialization(&tree, "pathogen");
    assert_eq!(
        fs::read_to_string(&fetched).expect("the materialization is a file"),
        PATHOGEN
    );
    assert_eq!(
        link_target(&tree.home(".vim/autoload/pathogen.vim")),
        fetched,
        "the link does not point at the materialization"
    );
    assert!(
        stderr.contains(&format!(
            "fetched {} from {}/pathogen.vim",
            display(&fetched),
            server.address()
        )),
        "the fetch was not reported:\n{stderr}"
    );
    assert!(
        stamp(&tree, "pathogen").is_file(),
        "no stamp records the fetch"
    );
}

#[test]
fn an_archive_remote_is_unpacked_and_read_like_a_tree() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_an_archive(&server));

    let assertion = tree.batfiles().arg("sync").assert().success();

    let unpacked = materialization(&tree, "fzf");
    assert!(
        unpacked.join("bin/fzf").is_file(),
        "the archive was not unpacked with its root stripped:\n{}",
        stderr_of(&assertion)
    );
    assert_eq!(link_target(&tree.home("bin/fzf")), unpacked.join("bin/fzf"));
    assert!(
        stderr_of(&assertion).contains(&format!("extracted {}", display(&unpacked))),
        "the extraction was not reported:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_file_remote_may_be_read_from_this_machine() {
    let tree = Tree::new();
    let published = tree.path("published.vim");
    fs::write(&published, PATHOGEN).expect("the published file");
    tree.write_manifest(&format!(
        r#"[remotes.pathogen]
type = "file"
url = "{}"

[[actions]]
type = "copy"
source = {{ remote = "pathogen" }}
dest = "~/.vim/autoload/pathogen.vim"
"#,
        file_url(&published)
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".vim/autoload/pathogen.vim")).expect("the copy"),
        PATHOGEN
    );
}

#[test]
fn a_later_sync_fetches_nothing_that_is_already_there_as_declared() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_a_file(&server, "/pathogen.vim", ""));
    tree.batfiles().arg("sync").assert().success();

    let quiet = tree.batfiles().arg("sync").assert().success();
    let verbose = tree.batfiles().args(["sync", "-v"]).assert().success();

    assert_eq!(
        server.requests(),
        1,
        "an unchanged remote was fetched again"
    );
    assert!(
        !stderr_of(&quiet).contains("fetch"),
        "an unchanged remote said something:\n{}",
        stderr_of(&quiet)
    );
    assert!(
        stderr_of(&verbose).contains(&format!(
            "unchanged {}",
            display(&materialization(&tree, "pathogen"))
        )),
        "-v did not report the remote unchanged:\n{}",
        stderr_of(&verbose)
    );
}

#[test]
fn a_changed_declaration_is_fetched_again() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_a_file(&server, "/pathogen.vim", ""));
    tree.batfiles().arg("sync").assert().success();

    tree.write_manifest(&linking_a_file(&server, "/pathogen-2.vim", ""));
    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(materialization(&tree, "pathogen")).expect("the materialization"),
        PATHOGEN_2
    );
    assert!(
        stderr_of(&assertion).contains(&format!(
            "refetched {} from {}/pathogen-2.vim",
            display(&materialization(&tree, "pathogen")),
            server.address()
        )),
        "the refetch was not reported:\n{}",
        stderr_of(&assertion)
    );
    assert!(
        !tree
            .path("repo")
            .join("remotes/pathogen.batfiles-old")
            .exists(),
        "what was replaced was left behind"
    );
}

#[test]
fn refresh_remotes_fetches_every_file_and_archive_remote_again() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "{}\n{}",
        linking_a_file(&server, "/pathogen.vim", ""),
        linking_an_archive(&server)
    ));
    tree.batfiles().arg("sync").assert().success();
    assert_eq!(server.requests(), 2);
    fs::write(materialization(&tree, "pathogen"), "edited by hand\n").expect("an edit");

    let assertion = tree
        .batfiles()
        .args(["sync", "--refresh-remotes"])
        .assert()
        .success();
    let stderr = stderr_of(&assertion);

    assert_eq!(server.requests(), 4, "not every remote was fetched again");
    assert_eq!(
        fs::read_to_string(materialization(&tree, "pathogen")).expect("the materialization"),
        PATHOGEN
    );
    for id in ["pathogen", "fzf"] {
        assert!(
            stderr.contains(&format!(
                "refetched {}",
                display(&materialization(&tree, id))
            )),
            "`{id}` was not reported refetched:\n{stderr}"
        );
    }
}

#[test]
fn refresh_remotes_still_refuses_what_batfiles_did_not_fetch() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_a_file(&server, "/pathogen.vim", ""));
    fs::create_dir_all(tree.path("repo/remotes")).expect("the remotes directory");
    fs::write(materialization(&tree, "pathogen"), "mine\n").expect("a file of the user's");

    let assertion = tree
        .batfiles()
        .args(["sync", "--refresh-remotes"])
        .assert()
        .failure();

    assert!(
        stderr_of(&assertion).contains("remove it and run sync again"),
        "{}",
        stderr_of(&assertion)
    );
    assert_eq!(
        fs::read_to_string(materialization(&tree, "pathogen")).expect("the user's file"),
        "mine\n"
    );
    assert_eq!(server.requests(), 0, "something was fetched");
}

#[test]
fn refresh_remotes_and_dry_run_do_not_go_together() {
    let assertion = Tree::new()
        .batfiles()
        .args(["sync", "--dry-run", "--refresh-remotes"])
        .assert()
        .failure()
        .code(2);
    assert!(
        stderr_of(&assertion).contains("cannot be used with"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_remote_that_changes_type_replaces_what_batfiles_fetched_for_it() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[remotes.fzf]\ntype = \"file\"\nurl = \"{}/pathogen.vim\"\n",
        server.address()
    ));
    tree.batfiles().arg("sync").assert().success();

    tree.write_manifest(&linking_an_archive(&server));
    tree.batfiles().arg("sync").assert().success();

    assert!(materialization(&tree, "fzf").join("bin/fzf").is_file());
    assert!(tree.home("bin/fzf").exists(), "the link was not made");
}

#[test]
fn a_failed_refetch_leaves_the_earlier_materialization_in_place() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_a_file(&server, "/pathogen.vim", ""));
    tree.batfiles().arg("sync").assert().success();
    let recorded = fs::read_to_string(stamp(&tree, "pathogen")).expect("the stamp");

    tree.write_manifest(&linking_a_file(
        &server,
        "/pathogen-2.vim",
        "sha256 = \"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"\n",
    ));
    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("does not match the declared sha256"),
        "{}",
        stderr_of(&assertion)
    );
    assert_eq!(
        fs::read_to_string(materialization(&tree, "pathogen")).expect("the materialization"),
        PATHOGEN,
        "the earlier fetch was not kept"
    );
    assert_eq!(
        fs::read_to_string(stamp(&tree, "pathogen")).expect("the stamp"),
        recorded,
        "the stamp no longer says what is there"
    );
}

#[test]
fn what_batfiles_did_not_fetch_is_refused_and_left_alone() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_an_archive(&server));
    let left = materialization(&tree, "fzf");
    fs::create_dir_all(&left).expect("the leftover directory");
    fs::write(left.join("NOTES.md"), "unpushed work\n").expect("the leftover file");

    let assertion = tree.batfiles().arg("sync").assert().failure().code(1);
    let stderr = stderr_of(&assertion);

    for expected in [
        "cannot materialize remote `fzf`",
        &format!("{} is a directory", display(&left)),
        "remove it and run sync again",
    ] {
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
    assert_eq!(
        fs::read_to_string(left.join("NOTES.md")).expect("the leftover file"),
        "unpushed work\n"
    );
    assert_eq!(server.requests(), 0, "something was fetched");
}

#[test]
fn a_symlink_is_refused_whatever_the_stamp_beside_it_says() {
    for (reaching, target) in [
        ("the declared content", "../vendored.vim"),
        ("nothing", "../nowhere"),
    ] {
        for args in [&["sync"][..], &["sync", "--dry-run"]] {
            let case = format!("a link reaching {reaching}, under `{}`", args.join(" "));
            let server = server();
            let tree = Tree::new();
            tree.write_manifest(&linking_a_file(&server, "/pathogen.vim", ""));
            tree.batfiles().arg("sync").assert().success();
            tree.repo_file("vendored.vim", PATHOGEN);
            let link = materialization(&tree, "pathogen");
            fs::remove_file(&link).expect("the fetched file");
            std::os::unix::fs::symlink(target, &link).expect("the link");
            let before = snapshot(&tree.path("repo"));

            let assertion = tree.batfiles().args(args).assert().failure().code(1);
            let stderr = stderr_of(&assertion);

            assert!(
                stderr.contains(&format!(
                    "{} is a symlink that batfiles did not fetch",
                    display(&link)
                )),
                "{case} was not refused:\n{stderr}"
            );
            assert_eq!(snapshot(&tree.path("repo")), before, "{case} was changed");
            assert_eq!(server.requests(), 1, "{case} was fetched over");
        }
    }
}

#[test]
fn a_dry_run_says_what_it_would_fetch_and_fetches_nothing() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_a_file(&server, "/pathogen.vim", ""));
    let before = snapshot(tree.root());

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .failure();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains(&format!(
            "would fetch {} from {}/pathogen.vim",
            display(&materialization(&tree, "pathogen")),
            server.address()
        )),
        "{stderr}"
    );
    assert!(
        stderr.contains("remote `pathogen` is not materialized"),
        "{stderr}"
    );
    assert_eq!(server.requests(), 0, "a dry run fetched");
    assert_eq!(snapshot(tree.root()), before, "a dry run wrote");
}

#[test]
fn a_dry_run_says_it_would_fetch_a_changed_declaration_again() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_a_file(&server, "/pathogen.vim", ""));
    tree.batfiles().arg("sync").assert().success();
    tree.write_manifest(&linking_a_file(&server, "/pathogen-2.vim", ""));
    let before = snapshot(tree.root());

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert!(
        stderr_of(&assertion).contains(&format!(
            "would refetch {}",
            display(&materialization(&tree, "pathogen"))
        )),
        "{}",
        stderr_of(&assertion)
    );
    assert_eq!(server.requests(), 1, "a dry run fetched");
    assert_eq!(snapshot(tree.root()), before, "a dry run wrote");
}

#[test]
fn an_excluded_file_remote_is_not_fetched_or_read() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_a_file(
        &server,
        "/pathogen.vim",
        "when = \"false\"\n",
    ));

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("remote `pathogen` is excluded on this machine"),
        "{}",
        stderr_of(&assertion)
    );
    assert_eq!(server.requests(), 0, "an excluded remote was fetched");
    assert!(!materialization(&tree, "pathogen").exists());
}

#[test]
fn an_apply_command_fetches_nothing() {
    let server = server();
    let tree = Tree::new();
    tree.write_manifest(&linking_a_file(&server, "/pathogen.vim", "").replace(
        "type = \"symlink\"\n",
        "type = \"symlink\"\nid = \"pathogen\"\n",
    ));

    let assertion = tree
        .batfiles()
        .args(["apply-action", "--id", "pathogen"])
        .assert()
        .failure()
        .code(1);

    assert!(
        stderr_of(&assertion).contains("remote `pathogen` is not materialized"),
        "{}",
        stderr_of(&assertion)
    );
    assert_eq!(server.requests(), 0, "an apply command fetched");
}

#[test]
fn a_file_remote_is_named_whole_and_never_as_a_directory() {
    for (action, expected) in [
        (
            one_symlink("@pathogen/pathogen.vim", "~/.vimrc"),
            "which is a single file; write `@pathogen`",
        ),
        (
            one_symlink_dir("@pathogen", "~/.vim", false),
            "this action installs from a directory",
        ),
    ] {
        let stderr = rejected(&format!(
            "[remotes.pathogen]\ntype = \"file\"\nurl = \"https://e.example/p.vim\"\n\n{action}"
        ));
        assert!(stderr.contains(expected), "no `{expected}` in:\n{stderr}");
    }
}

#[test]
fn an_inclusion_names_a_git_remote() {
    let stderr = rejected(
        r#"[remotes.fzf]
type = "archive"
url = "https://e.example/fzf.tar.gz"

[[actions]]
type = "include-remote"
id = "tools"
remote = "fzf"
"#,
    );
    assert!(
        stderr.contains("remote `fzf` is of type `archive`"),
        "{stderr}"
    );
}

#[test]
fn an_archive_remote_is_unpacked_through_its_filters() {
    let server = Server::new(&[("/fzf.tar.gz", Reply::Bytes(tarball(FZF_WITH_DOCS)))]);
    let tree = Tree::new();
    tree.write_manifest(&linking_a_filtered_archive(&server, "exclude = \"*.md\"\n"));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(entries(&materialization(&tree, "fzf")), ["bin"]);
    assert!(tree.home("bin/fzf").exists());
    let stamp = fs::read_to_string(stamp(&tree, "fzf")).expect("the stamp");
    assert!(stamp.contains("exclude = [\"*.md\"]"), "{stamp}");
}

#[test]
fn a_changed_filter_is_a_changed_declaration() {
    let server = Server::new(&[("/fzf.tar.gz", Reply::Bytes(tarball(FZF_WITH_DOCS)))]);
    let tree = Tree::new();
    tree.write_manifest(&linking_a_filtered_archive(&server, "exclude = \"*.md\"\n"));
    tree.batfiles().arg("sync").assert().success();

    tree.write_manifest(&linking_a_filtered_archive(
        &server,
        "exclude = [\"*.md\"]\n",
    ));
    tree.batfiles().arg("sync").assert().success();
    assert_eq!(server.requests(), 1, "one spelling of a filter for another");

    tree.write_manifest(&linking_a_filtered_archive(&server, ""));
    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .success();
    assert_eq!(server.requests(), 2);
    assert_eq!(
        entries(&materialization(&tree, "fzf")),
        ["README.md", "bin"]
    );
    assert!(
        stderr_of(&assertion).contains("refetched"),
        "{}",
        stderr_of(&assertion)
    );
}

/// [`FZF`] with documentation beside the program.
const FZF_WITH_DOCS: &[Member] = &[
    Member::Directory("fzf-0.1.0", 0o755),
    Member::Directory("fzf-0.1.0/bin", 0o755),
    Member::File("fzf-0.1.0/bin/fzf", 0o755, "#!/bin/sh\necho fzf\n"),
    Member::File("fzf-0.1.0/README.md", 0o644, "# fzf\n"),
];

/// A manifest declaring the archive remote `fzf` with `filters`, and linking its executable.
fn linking_a_filtered_archive(server: &Server, filters: &str) -> String {
    linking_an_archive(server).replacen(
        "archive-root = \"*\"\n",
        &format!("archive-root = \"*\"\n{filters}"),
        1,
    )
}
