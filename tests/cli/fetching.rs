//! `fetch-url`: what a download installs, what it refuses to install, and what
//! a dry run does instead of one.
//!
//! Every test answers from a local server (`guidance.md`, "Test environments").
//! Several of them assert on [`Server::requests`] as well as on the tree,
//! because "nothing was fetched" and "nothing was written" are different
//! claims and the first is the one `--dry-run` makes.

use std::fs;

use crate::support::*;

/// What the `fetching` fixture asks for, and the two bodies behind it. The
/// digest in the fixture manifest is `starship.toml`'s.
const ROUTES: &[(&str, Reply)] = &[
    (
        "/pathogen.vim",
        Reply::Body(
            "\" pathogen.vim, as the vim setup script fetches it\ncall pathogen#infect()\n",
        ),
    ),
    ("/starship.toml", Reply::Body("add_newline = false\n")),
];

/// The fixture pointed at a running server, which is how every test here starts.
fn fetching(server: &Server) -> Tree {
    let tree = Tree::fixture("fetching");
    tree.point_at(server);
    tree
}

#[test]
fn a_fetch_installs_a_file_the_home_does_not_have() {
    let server = Server::new(ROUTES);
    let tree = fetching(&server);

    let assertion = tree.batfiles().arg("sync").assert().success();

    // The parents are made on the way, which is the `mkdir -p` the shell
    // script does before its `curl`.
    assert_eq!(
        fs::read_to_string(tree.home(".vim/autoload/pathogen.vim")).expect("the fetched file"),
        "\" pathogen.vim, as the vim setup script fetches it\ncall pathogen#infect()\n"
    );
    assert_eq!(
        fs::read_to_string(tree.home(".config/starship.toml")).expect("the pinned file"),
        "add_newline = false\n"
    );
    assert!(stderr_of(&assertion).contains(&format!(
        "fetched {} from {}/pathogen.vim",
        display(&tree.home(".vim/autoload/pathogen.vim")),
        server.address()
    )));
    assert_eq!(server.requests(), 2);
}

#[test]
fn a_destination_that_is_already_there_is_kept_without_asking_the_server() {
    let server = Server::new(ROUTES);
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
    // The occupancy check comes first, so the one request is the *other*
    // action's: a destination that is taken costs no transfer at all.
    assert_eq!(server.requests(), 1);
}

#[test]
fn a_dry_run_says_what_it_would_fetch_and_fetches_nothing() {
    let server = Server::new(ROUTES);
    let tree = fetching(&server);
    let before = snapshot(&tree.path("home"));

    let assertion = tree
        .batfiles()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert!(stderr_of(&assertion).contains(&format!(
        "would fetch {} from {}/pathogen.vim",
        display(&tree.home(".vim/autoload/pathogen.vim")),
        server.address()
    )));
    // The stronger half of the promise: not merely that nothing was written,
    // but that the network was never reached to find out what to write.
    assert_eq!(server.requests(), 0);
    assert_eq!(snapshot(&tree.path("home")), before);
}

#[test]
fn a_digest_that_does_not_match_installs_nothing() {
    let server = Server::new(&[
        ("/pathogen.vim", Reply::Body("fine\n")),
        // The right shape, the wrong bytes: what an upstream file changing
        // under a pinned digest looks like.
        ("/starship.toml", Reply::Body("add_newline = true\n")),
    ]);
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
        "[[actions]]\ntype = \"fetch-url\"\nsource = \"{address}/a\"\ndest = \"~/.vimrc\"\n"
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
/// `fetch-url` without a digest is a truncated file that never gets noticed.
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
            "[[actions]]\ntype = \"fetch-url\"\nsource = \"{}/pathogen.vim\"\ndest = \"~/.vimrc\"\n",
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
    let server = Server::new(&[("/pathogen.vim", Reply::Missing)]);
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
    let server = Server::new(&[
        ("/pathogen.vim", Reply::RedirectTo("/elsewhere.vim")),
        ("/elsewhere.vim", Reply::Body("redirected\n")),
        ("/starship.toml", Reply::Body("add_newline = false\n")),
    ]);
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

    let server = Server::new(ROUTES);
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
        "[[actions]]\ntype = \"fetch-url\"\nsource = \"files/ackrc\"\ndest = \"~/.ackrc\"\n",
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
        "[[actions]]\ntype = \"fetch-url\"\nsource = \"file:///etc/hosts\"\ndest = \"~/.hosts\"\n",
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
         type = \"fetch-url\"\n\
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

#[test]
fn the_archive_fields_are_refused_until_the_step_that_extracts() {
    let tree = Tree::new();
    tree.write_manifest(
        "[[actions]]\n\
         type = \"fetch-url\"\n\
         source = \"https://example.com/a.tar.gz\"\n\
         dest = \"~/.local/tool\"\n\
         extract = true\n",
    );

    // Silently ignoring `extract` would fetch the tarball and install it as a
    // file, which is the failure rule 12 is written about.
    tree.batfiles().arg("sync").assert().failure();
}
