//! The two fetching actions: what a download installs, what an archive unpacks,
//! what each refuses to install, and what a dry run does instead of either.
//!
//! Every test answers from a local server (`guidance.md`, "Test environments").
//! Several of them assert on [`Server::requests`] as well as on the tree,
//! because "nothing was fetched" and "nothing was written" are different
//! claims and the first is the one `--dry-run` makes.

use std::fs;

use crate::support::*;

/// The pathogen body, which the fixture fetches without a digest.
const PATHOGEN: &str =
    "\" pathogen.vim, as the vim setup script fetches it\ncall pathogen#infect()\n";

/// The starship body. The digest in the fixture manifest is this text's.
const STARSHIP: &str = "add_newline = false\n";

/// What the `fetching` fixture asks for, and the three bodies behind it.
///
/// A function rather than a constant because one of the three is an archive
/// built at run time, and it is the whole set rather than a base to add to:
/// every action in the fixture runs on every test that drives it, so a route
/// missing here fails a test that is about something else.
fn routes() -> Vec<(&'static str, Reply)> {
    vec![
        ("/pathogen.vim", Reply::Body(PATHOGEN)),
        ("/starship.toml", Reply::Body(STARSHIP)),
        ("/fzf.tar.gz", Reply::Bytes(tarball(FZF))),
    ]
}

/// The fixture's routes with some of them answered differently.
///
/// The overrides go first, and the server answers with the first route that
/// matches, so an entry here replaces the standard one for that path.
fn routes_but(overrides: &[(&'static str, Reply)]) -> Vec<(&'static str, Reply)> {
    let mut routes = overrides.to_vec();
    routes.extend(self::routes());
    routes
}

/// A tool published as a release tarball: one versioned directory, an
/// executable under it, and a file that is not one.
const FZF: &[Member] = &[
    Member::Directory("fzf-0.1.0", 0o755),
    Member::Directory("fzf-0.1.0/bin", 0o755),
    Member::File("fzf-0.1.0/bin/fzf", 0o755, "#!/bin/sh\necho fzf\n"),
    Member::File("fzf-0.1.0/README.md", 0o644, "# fzf\n"),
];

/// The fixture pointed at a running server, which is how every test here starts.
fn fetching(server: &Server) -> Tree {
    let tree = Tree::fixture("fetching");
    tree.point_at(server);
    tree
}

/// A tree whose manifest declares one `fetch-archive` and nothing else, for the
/// archives that are about what extraction refuses.
fn one_archive(server: &Server, extra: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[[actions]]\n\
         type = \"fetch-archive\"\n\
         source = \"{}/tool.tar.gz\"\n\
         dest = \"~/.local/tool\"\n\
         {extra}",
        server.address()
    ));
    tree
}

/// A server answering `/tool.tar.gz` with exactly this archive.
fn serving(members: &[Member]) -> Server {
    Server::new(&[("/tool.tar.gz", Reply::Bytes(tarball(members)))])
}

#[test]
fn a_fetch_installs_a_file_the_home_does_not_have() {
    let server = Server::new(&routes());
    let tree = fetching(&server);

    let assertion = tree.batfiles().arg("sync").assert().success();

    // The parents are made on the way, which is the `mkdir -p` the shell
    // script does before its `curl`.
    assert_eq!(
        fs::read_to_string(tree.home(".vim/autoload/pathogen.vim")).expect("the fetched file"),
        PATHOGEN
    );
    assert_eq!(
        fs::read_to_string(tree.home(".config/starship.toml")).expect("the pinned file"),
        STARSHIP
    );
    assert!(stderr_of(&assertion).contains(&format!(
        "fetched {} from {}/pathogen.vim",
        display(&tree.home(".vim/autoload/pathogen.vim")),
        server.address()
    )));
    assert_eq!(server.requests(), 3);
}

#[test]
fn a_destination_that_is_already_there_is_kept_without_asking_the_server() {
    let server = Server::new(&routes());
    let tree = fetching(&server);
    fs::create_dir_all(tree.home(".vim/autoload")).expect("a vim directory");
    fs::write(tree.home(".vim/autoload/pathogen.vim"), "mine\n").expect("a file of the user's own");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".vim/autoload/pathogen.vim")).expect("the file"),
        "mine\n",
        "a seed replaced what was there"
    );
    assert!(stderr_of(&assertion).contains(&format!(
        "kept {}",
        display(&tree.home(".vim/autoload/pathogen.vim"))
    )));
    // The occupancy check comes first, so the requests are the *other* actions':
    // a destination that is taken costs no transfer at all.
    assert_eq!(server.requests(), 2);
}

#[test]
fn a_dry_run_says_what_it_would_fetch_and_fetches_nothing() {
    let server = Server::new(&routes());
    let tree = fetching(&server);
    let before = snapshot(&tree.path("home"));

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();
    let said = stderr_of(&assertion);

    assert!(said.contains(&format!(
        "would fetch {} from {}/pathogen.vim",
        display(&tree.home(".vim/autoload/pathogen.vim")),
        server.address()
    )));
    // An archive is reported at the granularity a whole directory is copied at:
    // what it would install and where it came from, and no list of entries,
    // which is the thing a dry run could not know without unpacking one.
    assert!(
        said.contains(&format!(
            "would extract {} from {}/fzf.tar.gz",
            display(&tree.home(".local/fzf")),
            server.address()
        )),
        "{said}"
    );
    // The stronger half of the promise: not merely that nothing was written,
    // but that the network was never reached to find out what to write.
    assert_eq!(server.requests(), 0);
    assert_eq!(snapshot(&tree.path("home")), before);
}

#[test]
fn a_digest_that_does_not_match_installs_nothing() {
    let server = Server::new(&routes_but(&[
        ("/pathogen.vim", Reply::Body("fine\n")),
        // The right shape, the wrong bytes: what an upstream file changing
        // under a pinned digest looks like.
        ("/starship.toml", Reply::Body("add_newline = true\n")),
    ]));
    let tree = fetching(&server);

    let assertion = tree.batfiles().arg("sync").assert().failure();
    let said = stderr_of(&assertion);

    assert!(
        !tree.home(".config/starship.toml").exists(),
        "the destination holds bytes that failed their digest"
    );
    assert!(
        said.contains("does not match the declared sha256"),
        "{said}"
    );
    // Both digests, so the manifest can be corrected from the diagnostic when
    // the change upstream was the expected one.
    assert!(said.contains("0fbf196b3612d0ba"), "{said}");
    assert!(
        !tree
            .home(".config/starship.toml.batfiles-incomplete")
            .exists(),
        "a staging node was left beside the destination"
    );
}

#[test]
fn a_transfer_that_stops_early_installs_nothing() {
    let address = server_that_hangs_up("half a file\n", 4096);
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[[actions]]\ntype = \"fetch-file\"\nsource = \"{address}/a\"\ndest = \"~/.vimrc\"\n"
    ));

    let assertion = tree.batfiles().arg("sync").assert().failure();

    // The case rule 15 is about: a short file published here would occupy the
    // destination, and every later run would find it there and call the work
    // done. The client is what notices — a body that ends before its
    // `Content-Length` never becomes bytes batfiles could publish — and what
    // this asserts is that batfiles then installs nothing.
    assert!(
        !tree.home(".vimrc").exists(),
        "half a download was installed"
    );
    assert!(
        !tree.home(".vimrc.batfiles-incomplete").exists(),
        "a staging node was left beside the destination"
    );
    assert!(
        stderr_of(&assertion).contains("could not fetch"),
        "{}",
        stderr_of(&assertion)
    );
}

/// An answer that is not a refusal is not therefore a file.
///
/// Batfiles asks for neither a range nor a conditional response, so these come
/// from an endpoint or a proxy doing something it was not asked to. Installing
/// one would put a fragment, or nothing at all, where the file goes — and a
/// destination that is occupied is one every later run calls done, which for a
/// `fetch-file` without a digest is a truncated file that never gets noticed.
#[test]
fn an_answer_that_is_not_a_whole_file_installs_nothing() {
    for reply in [
        Reply::NotAWholeFile {
            status: 206,
            body: "one range of a fi",
        },
        Reply::NotAWholeFile {
            status: 204,
            body: "",
        },
        Reply::NotAWholeFile {
            status: 304,
            body: "",
        },
    ] {
        let Reply::NotAWholeFile { status, .. } = reply else {
            unreachable!("every reply in this list is one")
        };
        let server = Server::new(&[("/pathogen.vim", reply)]);
        let tree = Tree::new();
        tree.write_manifest(&format!(
            "[[actions]]\ntype = \"fetch-file\"\nsource = \"{}/pathogen.vim\"\ndest = \"~/.vimrc\"\n",
            server.address()
        ));

        let assertion = tree.batfiles().arg("sync").assert().failure();

        assert!(
            !tree.home(".vimrc").exists(),
            "a {status} answer was installed as though it were the file"
        );
        assert!(
            stderr_of(&assertion).contains(&format!("the server answered {status}")),
            "{}",
            stderr_of(&assertion)
        );
    }
}

#[test]
fn a_server_that_does_not_have_the_file_says_so() {
    let server = Server::new(&routes_but(&[("/pathogen.vim", Reply::Missing)]));
    let tree = fetching(&server);

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(!tree.home(".vim/autoload/pathogen.vim").exists());
    assert!(
        stderr_of(&assertion).contains("the server answered 404"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_redirect_is_followed() {
    let server = Server::new(&routes_but(&[
        ("/pathogen.vim", Reply::RedirectTo("/elsewhere.vim")),
        ("/elsewhere.vim", Reply::Body("redirected\n")),
    ]));
    let tree = fetching(&server);

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".vim/autoload/pathogen.vim")).expect("the fetched file"),
        "redirected\n"
    );
}

/// A fetched file is readable, the way the `curl -o` this replaces left one.
#[cfg(unix)]
#[test]
fn a_fetched_file_arrives_readable_rather_than_staying_private() {
    use std::os::unix::fs::PermissionsExt;

    let server = Server::new(&routes());
    let tree = fetching(&server);

    tree.batfiles().arg("sync").assert().success();

    let mode = fs::metadata(tree.home(".vim/autoload/pathogen.vim"))
        .expect("the fetched file")
        .permissions()
        .mode();
    // The staging node is created at 0600 so an interrupted run leaves nothing
    // readable behind; publication is where it widens.
    assert_eq!(
        mode & 0o777,
        0o644,
        "the fetched file kept its staging mode"
    );
}

// What a manifest may say. Each of these is refused as the document is read,
// so nothing is fetched and no destination is touched.

#[test]
fn a_source_that_is_not_a_url_is_refused_before_anything_runs() {
    let tree = Tree::new();
    tree.write_manifest(
        "[[actions]]\ntype = \"fetch-file\"\nsource = \"files/ackrc\"\ndest = \"~/.ackrc\"\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("is not an http:// or https:// URL"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_file_url_names_the_step_that_makes_it_work() {
    let tree = Tree::new();
    tree.write_manifest(
        "[[actions]]\ntype = \"fetch-file\"\nsource = \"file:///etc/hosts\"\ndest = \"~/.hosts\"\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("step 9.3"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_digest_that_is_not_one_is_refused_before_anything_is_fetched() {
    let tree = Tree::new();
    tree.write_manifest(
        "[[actions]]\n\
         type = \"fetch-file\"\n\
         source = \"https://example.com/a\"\n\
         sha256 = \"abc123\"\n\
         dest = \"~/.a\"\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("is not 64 hexadecimal digits"),
        "{}",
        stderr_of(&assertion)
    );
}

/// The archive fields belong to `fetch-archive`, so they are unknown on
/// `fetch-file` for good rather than until a step.
///
/// Nothing about `fetch-file` is provisional: it fetches a response body and
/// installs it as a file, and a repository that wants an archive unpacked asks
/// for it by `type`. Accepting `archive-root` and ignoring it would install the
/// tarball itself at a destination every later run then finds occupied and
/// calls done. `extract` does not exist in the format at all — a choice between
/// two shapes is a `type`, never a boolean.
#[test]
fn the_archive_fields_are_not_fields_of_a_plain_download() {
    let tree = Tree::new();
    for field in [
        "archive-root = \"*\"",
        "include = [\"bin/*\"]",
        "extract = true",
    ] {
        tree.write_manifest(&format!(
            "[[actions]]\n\
             type = \"fetch-file\"\n\
             source = \"https://example.com/a.tar.gz\"\n\
             dest = \"~/.local/tool\"\n\
             {field}\n",
        ));

        tree.batfiles().arg("sync").assert().failure();
    }
}

/// The entry filters are specified and not built, so they are refused rather
/// than accepted and ignored — which would install more of an archive than the
/// manifest asked for, at a destination every later run then calls done.
#[test]
fn the_entry_filters_are_not_built_yet() {
    let tree = Tree::new();
    for field in ["include = [\"bin/*\"]", "exclude = [\"*.md\"]"] {
        tree.write_manifest(&format!(
            "[[actions]]\n\
             type = \"fetch-archive\"\n\
             source = \"https://example.com/a.tar.gz\"\n\
             dest = \"~/.local/tool\"\n\
             {field}\n",
        ));

        tree.batfiles().arg("sync").assert().failure();
    }
}

// `fetch-archive`: what an archive installs, and what it refuses to.

#[test]
fn an_archive_is_unpacked_where_nothing_is() {
    let server = Server::new(&routes());
    let tree = fetching(&server);

    let assertion = tree.batfiles().arg("sync").assert().success();

    // The versioned directory is gone, because `archive-root = "*"` found it
    // and stripped it; what is at `dest` is the tool rather than a directory
    // holding the tool.
    assert_eq!(
        fs::read_to_string(tree.home(".local/fzf/bin/fzf")).expect("the unpacked program"),
        "#!/bin/sh\necho fzf\n"
    );
    assert_eq!(entries(&tree.home(".local/fzf")), ["README.md", "bin"]);
    assert!(stderr_of(&assertion).contains(&format!(
        "extracted {} from {}/fzf.tar.gz",
        display(&tree.home(".local/fzf")),
        server.address()
    )));
    // Neither the staging tree nor the archive it was unpacked from survives
    // beside the destination.
    assert_eq!(entries(&tree.home(".local")), ["fzf"]);
}

/// An archive with no `archive-root` is unpacked exactly as it is written, and
/// a plain `.tar` is the same archive by another wrapping.
#[test]
fn an_archive_without_a_root_keeps_the_paths_it_was_written_with() {
    let members = &[
        Member::Directory("bin", 0o755),
        Member::File("bin/tool", 0o755, "run\n"),
    ];
    for (name, body) in [
        ("gzipped", tarball(members)),
        ("plain", plain_tarball(members)),
        // The original format, whose headers carry no `ustar` magic. A tar
        // reader takes it; a detector that looks for the magic would refuse an
        // archive it was about to read perfectly well.
        ("V7", v7_tarball(members)),
    ] {
        let server = Server::new(&[("/tool.tar.gz", Reply::Bytes(body))]);
        let tree = one_archive(&server, "");

        tree.batfiles().arg("sync").assert().success();

        assert_eq!(
            fs::read_to_string(tree.home(".local/tool/bin/tool"))
                .unwrap_or_else(|error| panic!("the {name} archive: {error}")),
            "run\n"
        );
    }
}

/// `tar czf x.tgz .` writes every entry with a leading `./`, which is how most
/// archives are made and therefore the spelling that has to work.
///
/// It is also the spelling `archive-root = "*"` is most easily wrong about: a
/// `.` left at the front of every path is a top-level component like any other,
/// and stripping it would leave the versioned directory in place.
#[test]
fn entries_written_with_a_leading_dot_slash_are_unpacked_as_though_they_were_not() {
    let server = serving(&[
        Member::Directory("./", 0o755),
        Member::Directory("./tool-1.0", 0o755),
        Member::Directory("./tool-1.0/bin", 0o755),
        Member::File("./tool-1.0/bin/tool", 0o755, "run\n"),
    ]);
    let tree = one_archive(&server, "archive-root = \"*\"\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".local/tool/bin/tool")).expect("the unpacked program"),
        "run\n"
    );
    assert_eq!(entries(&tree.home(".local/tool")), ["bin"]);
}

/// A gzip stream of several members concatenated, which `pigz` writes and `cat
/// a.gz b.gz` produces.
///
/// A decoder that read only the first member would hand back the front of the
/// tar and stop, and what makes that worth a test is that it does not look like
/// a failure: the tree would be published short, and every later run would find
/// the destination occupied and call it done.
#[test]
fn a_gzip_stream_of_several_members_is_read_to_the_end() {
    let members = &[
        Member::File("bin/first", 0o755, "one\n"),
        Member::File("bin/second", 0o755, "two\n"),
        Member::File("bin/third", 0o755, "three\n"),
    ];
    let server = Server::new(&[("/tool.tar.gz", Reply::Bytes(multi_member_tarball(members)))]);
    let tree = one_archive(&server, "");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        entries(&tree.home(".local/tool/bin")),
        ["first", "second", "third"]
    );
}

/// A named `archive-root` installs one directory out of an archive, and nothing
/// beside it.
#[test]
fn a_named_root_selects_what_is_under_it_and_leaves_the_rest() {
    let server = serving(&[
        Member::File("tool-1.0/LICENSE", 0o644, "MIT\n"),
        Member::Directory("tool-1.0/bin", 0o755),
        Member::File("tool-1.0/bin/tool", 0o755, "run\n"),
    ]);
    let tree = one_archive(&server, "archive-root = \"tool-1.0/bin\"\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(entries(&tree.home(".local/tool")), ["tool"]);
    assert_eq!(
        fs::read_to_string(tree.home(".local/tool/tool")).expect("the selected program"),
        "run\n"
    );
}

/// A destination that is already there means the action is done, whatever put it
/// there — including a `create-dir` earlier in the same manifest. `dest` is one
/// name, not a merge root.
#[test]
fn a_destination_that_is_already_a_directory_is_kept_without_asking_the_server() {
    let server = Server::new(&routes());
    let tree = fetching(&server);
    fs::create_dir_all(tree.home(".local/fzf")).expect("a directory of the user's own");

    let assertion = tree.batfiles().args(["sync", "-v"]).assert().success();

    assert_eq!(
        entries(&tree.home(".local/fzf")),
        Vec::<String>::new(),
        "an occupied destination was unpacked into"
    );
    assert!(stderr_of(&assertion).contains(&format!("kept {}", display(&tree.home(".local/fzf")))));
    // Two requests, both the other actions': an occupied destination costs no
    // transfer at all.
    assert_eq!(server.requests(), 2);
}

/// Every way an entry can name a path outside the tree it is unpacked into.
///
/// Each fails the whole action rather than being skipped: an archive carrying
/// one of these is not one to install part of, and nothing has been written when
/// it is found, because the paths are all read before any of them is created.
#[test]
fn an_entry_that_would_be_written_outside_the_destination_installs_nothing() {
    for (what, members) in [
        (
            "an absolute path",
            &[Member::File("/etc/passwd", 0o644, "root\n")][..],
        ),
        (
            "a path climbing out",
            &[Member::File("../../.ssh/authorized_keys", 0o600, "key\n")][..],
        ),
        (
            "a path climbing out from inside",
            &[Member::File("bin/../../escape", 0o644, "x\n")][..],
        ),
        (
            "a symlink to an absolute path",
            &[Member::Symlink("secrets", "/etc/shadow")][..],
        ),
        (
            "a symlink climbing out",
            &[Member::Symlink("secrets", "../../.ssh/id_rsa")][..],
        ),
        (
            "a hardlink to something outside",
            &[Member::Hardlink("secrets", "../../.netrc")][..],
        ),
    ] {
        let server = serving(members);
        let tree = one_archive(&server, "");

        let assertion = tree.batfiles().arg("sync").assert().failure();
        let said = stderr_of(&assertion);

        assert!(
            !tree.home(".local/tool").exists(),
            "{what} was installed anyway"
        );
        assert!(said.contains("would be written outside"), "{what}: {said}");
        assert!(
            !tree.home(".local").exists() || entries(&tree.home(".local")).is_empty(),
            "{what} left something beside the destination"
        );
    }
}

/// The escape that takes two entries, and that no check on a single path finds.
///
/// `a/b -> ../x` is honest and stays inside. `escape -> a/b/../../outside`
/// cancels on paper to `outside`, which is inside; the kernel resolves `a/b` to
/// `x` first, goes up twice from there, and lands beside the destination. Left
/// alone, everything under `escape/` is then written outside `dest` — through a
/// link, past every check that looked only at spellings.
///
/// The external directory exists here on purpose. Without it the write fails on
/// its own and the test would pass for the wrong reason.
#[cfg(unix)]
#[test]
fn an_entry_reached_through_the_archives_own_symlink_installs_nothing() {
    let server = serving(&[
        Member::Directory("a", 0o755),
        Member::Symlink("a/b", "../x"),
        Member::Directory("x", 0o755),
        Member::Symlink("escape", "a/b/../../outside"),
        Member::File("escape/planted", 0o644, "not yours\n"),
    ]);
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[[actions]]\n\
         type = \"fetch-archive\"\n\
         source = \"{}/tool.tar.gz\"\n\
         dest = \"~/.local/tool\"\n",
        server.address()
    ));
    // Where `escape` resolves to once the kernel has followed `a/b`: a sibling
    // of the destination, and one that already exists, so a write through the
    // link would succeed rather than failing of its own accord.
    let outside = tree.home(".local/outside");
    fs::create_dir_all(&outside).expect("a directory beside the destination");

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert_eq!(
        entries(&outside),
        Vec::<String>::new(),
        "a file was written outside the destination, through the archive's own symlink"
    );
    assert!(!tree.home(".local/tool").exists());
    assert!(
        stderr_of(&assertion).contains("would be written outside"),
        "{}",
        stderr_of(&assertion)
    );
}

/// Nothing is created under a symlink the archive declares, even where the link
/// stays inside: the kernel follows it before creating what is below it, so the
/// entry does not land where the archive says it does.
#[cfg(unix)]
#[test]
fn an_entry_written_under_the_archives_own_symlink_installs_nothing() {
    let server = serving(&[
        Member::Directory("real", 0o755),
        Member::Symlink("link", "real"),
        Member::File("link/inside", 0o644, "misplaced\n"),
    ]);
    let tree = one_archive(&server, "");

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(!tree.home(".local/tool").exists());
    assert!(
        stderr_of(&assertion).contains("would be written outside"),
        "{}",
        stderr_of(&assertion)
    );
}

/// A symlink that stays inside the tree is installed as a symlink, target and
/// all: an archive that ships one is describing its own layout.
#[cfg(unix)]
#[test]
fn a_symlink_that_stays_inside_the_tree_is_installed() {
    let server = serving(&[
        Member::Directory("bin", 0o755),
        Member::File("bin/tool-1.0", 0o755, "run\n"),
        Member::Symlink("bin/tool", "tool-1.0"),
        Member::Symlink("current", "bin/tool-1.0"),
    ]);
    let tree = one_archive(&server, "");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        link_target(&tree.home(".local/tool/bin/tool")),
        std::path::PathBuf::from("tool-1.0")
    );
    assert_eq!(
        fs::read_to_string(tree.home(".local/tool/current")).expect("the link resolves"),
        "run\n"
    );
}

/// A hardlink is a second name for an entry the archive already holds, which is
/// how a tar records the same file twice.
#[test]
fn a_hardlink_to_another_entry_is_installed() {
    let server = serving(&[
        Member::File("tool-1.0/bin/tool", 0o755, "run\n"),
        Member::Hardlink("tool-1.0/bin/also-tool", "tool-1.0/bin/tool"),
    ]);
    let tree = one_archive(&server, "archive-root = \"*\"\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".local/tool/bin/also-tool")).expect("the second name"),
        "run\n"
    );
}

/// Anything that is not a file, a directory, or a link. Skipping it would
/// publish an incomplete tree at a destination every later run calls finished.
#[test]
fn an_entry_that_is_not_a_file_a_directory_or_a_link_installs_nothing() {
    let server = serving(&[
        Member::File("bin/tool", 0o755, "run\n"),
        Member::Fifo("bin/pipe"),
    ]);
    let tree = one_archive(&server, "");

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(!tree.home(".local/tool").exists());
    assert!(
        stderr_of(&assertion).contains("neither a file, a directory, nor a link"),
        "{}",
        stderr_of(&assertion)
    );
}

/// A body that is not an archive batfiles unpacks, named by what it is.
#[test]
fn a_body_that_is_not_a_tar_archive_says_what_it_is() {
    for (body, said) in [
        (&b"PK\x03\x04and the rest of a zip"[..], "a zip archive"),
        (&b"BZh9and the rest of a bzip2"[..], "a bzip2 archive"),
        (
            &b"<!DOCTYPE html>\n<title>Not found</title>\n"[..],
            "not an archive batfiles recognizes",
        ),
    ] {
        let server = Server::new(&[("/tool.tar.gz", Reply::Bytes(body.to_vec()))]);
        let tree = one_archive(&server, "");

        let assertion = tree.batfiles().arg("sync").assert().failure();

        assert!(!tree.home(".local/tool").exists());
        assert!(
            stderr_of(&assertion).contains(said),
            "{}",
            stderr_of(&assertion)
        );
    }
}

/// `archive-root = "*"` over an archive with two top-level directories has no
/// answer, so it says so and names both rather than picking one.
#[test]
fn an_archive_with_no_single_top_level_directory_is_refused() {
    let server = serving(&[
        Member::File("tool-1.0/bin/tool", 0o755, "run\n"),
        Member::File("docs/README.md", 0o644, "# tool\n"),
    ]);
    let tree = one_archive(&server, "archive-root = \"*\"\n");

    let assertion = tree.batfiles().arg("sync").assert().failure();
    let said = stderr_of(&assertion);

    assert!(!tree.home(".local/tool").exists());
    assert!(said.contains("no single top-level directory"), "{said}");
    // Both of them, because what the author writes in place of the `*` is one
    // of the names in the message.
    assert!(said.contains("docs") && said.contains("tool-1.0"), "{said}");
}

/// A named root over an archive holding nothing under it, which is a manifest
/// naming a directory the upstream archive does not have.
#[test]
fn a_named_root_that_matches_nothing_is_refused() {
    let server = serving(&[Member::File("tool-2.0/bin/tool", 0o755, "run\n")]);
    let tree = one_archive(&server, "archive-root = \"tool-1.0\"\n");

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(!tree.home(".local/tool").exists());
    assert!(
        stderr_of(&assertion).contains("has nothing under `tool-1.0`"),
        "{}",
        stderr_of(&assertion)
    );
}

/// A digest is checked against the archive's own bytes, and it is checked before
/// a single entry is unpacked.
#[test]
fn an_archive_whose_digest_does_not_match_is_never_unpacked() {
    let server = serving(&[Member::File("bin/tool", 0o755, "run\n")]);
    let tree = one_archive(
        &server,
        // The empty string's digest, which no archive has.
        "sha256 = \"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(!tree.home(".local/tool").exists());
    assert!(
        stderr_of(&assertion).contains("does not match the declared sha256"),
        "{}",
        stderr_of(&assertion)
    );
    // Neither leftover survives: the archive was downloaded, refused, and taken
    // away, and the staging tree it would have been unpacked into with it.
    assert_eq!(entries(&tree.home(".local")), Vec::<String>::new());
}

/// An archive-root that could match no entry batfiles would unpack is refused as
/// the manifest is read, before anything is fetched.
#[test]
fn an_archive_root_that_names_nothing_inside_is_refused_before_anything_is_fetched() {
    let tree = Tree::new();
    for root in ["/tool", "../tool", ""] {
        tree.write_manifest(&format!(
            "[[actions]]\n\
             type = \"fetch-archive\"\n\
             source = \"https://example.com/a.tar.gz\"\n\
             dest = \"~/.local/tool\"\n\
             archive-root = \"{root}\"\n",
        ));

        let assertion = tree.batfiles().arg("sync").assert().failure();

        assert!(
            stderr_of(&assertion).contains("is not a path inside the archive"),
            "`{root}`: {}",
            stderr_of(&assertion)
        );
    }
}

/// What the archive says the modes are, minus what batfiles will not grant.
#[cfg(unix)]
#[test]
fn an_unpacked_entry_carries_the_archives_permissions_without_the_dangerous_ones() {
    use std::os::unix::fs::PermissionsExt;

    let server = serving(&[
        Member::Directory("tool", 0o750),
        // Setuid root is the whole reason this is masked rather than copied.
        Member::File("tool/setuid", 0o4755, "x\n"),
        Member::File("tool/program", 0o755, "x\n"),
        Member::File("tool/notes", 0o600, "x\n"),
    ]);
    let tree = one_archive(&server, "");

    tree.batfiles().arg("sync").assert().success();

    let mode = |relative: &str| {
        fs::metadata(tree.home(relative))
            .unwrap_or_else(|error| panic!("{relative}: {error}"))
            .permissions()
            .mode()
            & 0o7777
    };
    assert_eq!(mode(".local/tool/tool/program"), 0o755);
    assert_eq!(mode(".local/tool/tool/notes"), 0o600);
    assert_eq!(
        mode(".local/tool/tool/setuid"),
        0o755,
        "an archive handed out setuid"
    );
    // Applied last, and deepest-first, so a directory the archive marks
    // unwritable is still one batfiles could fill on the way.
    assert_eq!(mode(".local/tool/tool"), 0o750);
}

/// The destination itself carries the mode of whatever `archive-root` stripped.
///
/// The staging directory is created closed so that an interrupted run leaves
/// nothing readable behind, and the widening at the end is what the archive's own
/// root entry supplies. Without it every `archive-root = "*"` destination is
/// published at `0700` — nothing is broken, and nothing but the owner can enter
/// the tool that was just installed.
#[cfg(unix)]
#[test]
fn the_destination_carries_the_mode_of_the_directory_that_was_stripped() {
    use std::os::unix::fs::PermissionsExt;

    let server = serving(&[
        Member::Directory("tool-1.0", 0o755),
        Member::File("tool-1.0/tool", 0o755, "run\n"),
    ]);
    let tree = one_archive(&server, "archive-root = \"*\"\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::metadata(tree.home(".local/tool"))
            .expect("the destination")
            .permissions()
            .mode()
            & 0o7777,
        0o755
    );
}

/// An archive that lists only files says nothing about the mode of the tree they
/// are in, and the closed staging mode is not an answer to publish.
#[cfg(unix)]
#[test]
fn a_destination_no_entry_describes_is_still_published_readable() {
    use std::os::unix::fs::PermissionsExt;

    let server = serving(&[Member::File("bin/tool", 0o755, "run\n")]);
    let tree = one_archive(&server, "");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::metadata(tree.home(".local/tool"))
            .expect("the destination")
            .permissions()
            .mode()
            & 0o7777,
        0o755,
        "the destination kept the mode the staging directory was built under"
    );
}

/// An `archive-root` written with a `.` in it names the same directory, and is
/// matched against entries that carry no such spelling.
#[test]
fn an_archive_root_is_matched_however_it_is_spelled() {
    let server = serving(&[Member::File("tool-1.0/bin/tool", 0o755, "run\n")]);
    let tree = one_archive(&server, "archive-root = \"./tool-1.0\"\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".local/tool/bin/tool")).expect("the selected program"),
        "run\n"
    );
}

/// An `archive-root` that climbs is refused as the manifest is read, because an
/// archive entry may not climb either: a root spelled with `..` would be matched
/// against entries that carry no such spelling and would select nothing.
#[test]
fn an_archive_root_that_climbs_is_refused_before_anything_is_fetched() {
    let tree = Tree::new();
    tree.write_manifest(
        "[[actions]]\n\
         type = \"fetch-archive\"\n\
         source = \"https://example.com/a.tar.gz\"\n\
         dest = \"~/.local/tool\"\n\
         archive-root = \"releases/../tool\"\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("is not a path inside the archive"),
        "{}",
        stderr_of(&assertion)
    );
}

/// A directory the archive marks read-only still gets its children, because the
/// mode is applied after they are written rather than when it is created.
#[cfg(unix)]
#[test]
fn a_directory_the_archive_marks_unwritable_is_still_filled() {
    let server = serving(&[
        Member::Directory("locked", 0o500),
        Member::File("locked/inside", 0o644, "here\n"),
    ]);
    let tree = one_archive(&server, "");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".local/tool/locked/inside")).expect("the child"),
        "here\n"
    );
}
