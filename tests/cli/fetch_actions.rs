//! CLI tests for file downloads, archive extraction, validation failures, and dry runs. Use
//! local-server request counts to detect unwanted downloads.

use std::fs;

use crate::support::*;

/// The pathogen body, which the fixture fetches without a digest.
const PATHOGEN: &str =
    "\" pathogen.vim, as the vim setup script fetches it\ncall pathogen#infect()\n";

/// The starship body. The digest in the fixture manifest is this text's.
const STARSHIP: &str = "add_newline = false\n";

/// [`STARSHIP`]'s digest, as the fixture manifest writes it.
const STARSHIP_SHA256: &str = "0fbf196b3612d0bafdaaa79b45efb1d03c813bedc53bbe3e6e0f5550ec14683f";

/// Return all three fixture routes, including the generated release archive.
fn routes() -> Vec<(&'static str, Reply)> {
    vec![
        ("/pathogen.vim", Reply::Body(PATHOGEN)),
        ("/starship.toml", Reply::Body(STARSHIP)),
        ("/fzf.tar.gz", Reply::Bytes(tarball(FZF))),
    ]
}

/// Return fixture routes with supplied responses taking precedence.
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
        r#"[[actions]]
type = "fetch-archive"
source = "{}/tool.tar.gz"
dest = "~/.local/tool"
{extra}"#,
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
    // The other actions account for the two requests.
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
    assert!(
        said.contains(&format!(
            "would extract {} from {}/fzf.tar.gz",
            display(&tree.home(".local/fzf")),
            server.address()
        )),
        "{said}"
    );
    assert_eq!(server.requests(), 0);
    assert_eq!(snapshot(&tree.path("home")), before);
}

#[test]
fn a_digest_that_does_not_match_installs_nothing() {
    let server = Server::new(&routes_but(&[
        ("/pathogen.vim", Reply::Body("fine\n")),
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
        r#"[[actions]]
type = "fetch-file"
source = "{address}/a"
dest = "~/.vimrc"
"#
    ));

    let assertion = tree.batfiles().arg("sync").assert().failure();

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

/// Reject partial or empty HTTP responses without installing a destination.
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
            r#"[[actions]]
type = "fetch-file"
source = "{}/pathogen.vim"
dest = "~/.vimrc"
"#,
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

/// Publish fetched files with readable permissions.
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
    assert_eq!(
        mode & 0o777,
        0o644,
        "the fetched file kept its staging mode"
    );
}

#[test]
#[cfg(unix)]
fn a_fetched_file_declared_executable_arrives_executable() {
    use std::os::unix::fs::PermissionsExt;

    let server = Server::new(&[("/tool", Reply::Body("#!/bin/sh\necho tool\n"))]);
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "fetch-file"
source = "{}/tool"
dest = "~/bin/tool"
executable = true
"#,
        server.address()
    ));

    tree.batfiles().arg("sync").assert().success();

    let mode = fs::metadata(tree.home("bin/tool"))
        .expect("the fetched file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
}

#[test]
fn a_source_that_is_not_a_url_is_refused_before_anything_runs() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "fetch-file"
source = "files/ackrc"
dest = "~/.ackrc"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("is not an http://, https://, or file:// URL"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_file_url_fetches_a_file_on_this_machine() {
    let tree = Tree::new();
    let published = tree.path("published");
    fs::create_dir_all(&published).expect("the published directory");
    fs::write(published.join("starship.toml"), STARSHIP).expect("the published file");
    fs::write(published.join("fzf.tar.gz"), tarball(FZF)).expect("the published archive");
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "fetch-file"
source = "{}"
sha256 = "{STARSHIP_SHA256}"
dest = "~/.config/starship.toml"

[[actions]]
type = "fetch-archive"
source = "{}"
dest = "~/.local/fzf"
archive-root = "*"
"#,
        file_url(&published.join("starship.toml")),
        file_url(&published.join("fzf.tar.gz")),
    ));

    let assertion = tree.batfiles().arg("sync").assert().success();

    assert_eq!(
        fs::read_to_string(tree.home(".config/starship.toml")).expect("the fetched file"),
        STARSHIP,
        "{}",
        stderr_of(&assertion)
    );
    assert!(
        tree.home(".local/fzf/bin/fzf").is_file(),
        "the archive was not unpacked:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_file_url_naming_nothing_fails_naming_the_path() {
    let tree = Tree::new();
    let missing = tree.path("nowhere/pathogen.vim");
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "fetch-file"
source = "{}"
dest = "~/.vim/autoload/pathogen.vim"
"#,
        file_url(&missing)
    ));

    let assertion = tree.batfiles().arg("sync").assert().failure();
    let stderr = stderr_of(&assertion);

    // The path the URL names, in its separators.
    assert!(
        stderr.contains(&format!("could not read {}", written(&missing))),
        "{stderr}"
    );
    assert!(
        !tree.home(".vim/autoload/pathogen.vim").exists(),
        "nothing should be installed"
    );
}

#[test]
fn a_file_url_naming_another_host_is_refused_as_the_manifest_is_read() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "fetch-file"
source = "file://fileserver/share/hosts"
dest = "~/.hosts"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("names a host other than `localhost`"),
        "{}",
        stderr_of(&assertion)
    );
}

#[test]
fn a_digest_that_is_not_one_is_refused_before_anything_is_fetched() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "fetch-file"
source = "https://e.example/a"
sha256 = "abc123"
dest = "~/.a"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("is not 64 hexadecimal digits"),
        "{}",
        stderr_of(&assertion)
    );
}

/// Reject archive-specific fields on `fetch-file` actions.
#[test]
fn the_archive_fields_are_not_fields_of_a_plain_download() {
    let tree = Tree::new();
    for field in [
        "archive-root = \"*\"",
        "include = [\"bin/*\"]",
        "extract = true",
    ] {
        tree.write_manifest(&format!(
            r#"[[actions]]
type = "fetch-file"
source = "https://e.example/a.tar.gz"
dest = "~/.local/tool"
{field}
"#,
        ));

        tree.batfiles().arg("sync").assert().failure();
    }
}

#[test]
fn an_archive_is_unpacked_where_nothing_is() {
    let server = Server::new(&routes());
    let tree = fetching(&server);

    let assertion = tree.batfiles().arg("sync").assert().success();

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
    assert_eq!(entries(&tree.home(".local")), ["fzf"]);
}

/// An archive with no `archive-root` is unpacked exactly as it is written, and
/// a plain or bzipped `.tar` is the same archive by another wrapping.
#[test]
fn an_archive_without_a_root_keeps_the_paths_it_was_written_with() {
    let members = &[
        Member::Directory("bin", 0o755),
        Member::File("bin/tool", 0o755, "run\n"),
    ];
    for (name, body) in [
        ("gzipped", tarball(members)),
        ("plain", plain_tarball(members)),
        ("bzipped", bzipped_tarball(members)),
        // A V7 archive has no ustar magic.
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

/// Ignore leading `./` components when extracting and detecting the archive root.
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

/// Read all concatenated gzip members or bzip2 streams before publishing the extracted tree.
#[test]
fn a_compressed_stream_of_several_members_is_read_to_the_end() {
    let members = &[
        Member::File("bin/first", 0o755, "one\n"),
        Member::File("bin/second", 0o755, "two\n"),
        Member::File("bin/third", 0o755, "three\n"),
    ];
    for stream in [Stream::Gzip, Stream::Bzip2] {
        let body = multi_member_tarball(members, stream);
        let server = Server::new(&[("/tool.tar.gz", Reply::Bytes(body))]);
        let tree = one_archive(&server, "");

        tree.batfiles().arg("sync").assert().success();

        assert_eq!(
            entries(&tree.home(".local/tool/bin")),
            ["first", "second", "third"],
            "{stream:?}"
        );
    }
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

/// A release tarball with more in it than a tool: documentation beside the
/// program, and a library directory with a backup in it.
const RELEASE: &[Member] = &[
    Member::Directory("tool-1.0", 0o755),
    Member::File("tool-1.0/README.md", 0o644, "# tool\n"),
    Member::Directory("tool-1.0/bin", 0o750),
    Member::File("tool-1.0/bin/tool", 0o755, "run\n"),
    Member::Directory("tool-1.0/lib", 0o755),
    Member::File("tool-1.0/lib/core", 0o644, "core\n"),
    Member::File("tool-1.0/lib/core.bak", 0o644, "old\n"),
];

#[test]
fn filters_match_what_is_left_once_the_root_is_stripped() {
    let server = serving(RELEASE);
    let tree = one_archive(
        &server,
        "archive-root = \"*\"\ninclude = [\"bin\", \"lib\"]\nexclude = \"**/*.bak\"\n",
    );

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(entries(&tree.home(".local/tool")), ["bin", "lib"]);
    assert_eq!(entries(&tree.home(".local/tool/lib")), ["core"]);
    assert_eq!(
        fs::read_to_string(tree.home(".local/tool/bin/tool")).expect("the program"),
        "run\n"
    );
}

#[cfg(unix)]
#[test]
fn a_directory_entry_holding_what_is_selected_keeps_its_own_mode() {
    use std::os::unix::fs::PermissionsExt;

    let server = serving(RELEASE);
    let tree = one_archive(&server, "archive-root = \"*\"\ninclude = \"bin/tool\"\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(entries(&tree.home(".local/tool")), ["bin"]);
    let mode = fs::metadata(tree.home(".local/tool/bin"))
        .expect("the directory")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o750);
}

#[test]
fn filters_that_leave_nothing_install_nothing() {
    let server = serving(RELEASE);
    let tree = one_archive(&server, "archive-root = \"*\"\ninclude = \"share\"\n");

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .failure();

    assert!(
        stderr_of(&assertion).contains("has nothing that `include` and `exclude` select"),
        "{}",
        stderr_of(&assertion)
    );
    assert!(!tree.home(".local/tool").exists());
}

#[test]
fn a_hard_link_to_an_entry_the_filters_leave_out_installs_nothing() {
    let server = serving(&[
        Member::File("lib/core", 0o644, "core\n"),
        Member::Hardlink("bin/core", "lib/core"),
    ]);
    let tree = one_archive(&server, "include = \"bin\"\n");

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "sync"])
        .assert()
        .failure();

    assert!(
        stderr_of(&assertion).contains(
            "has the hard link `bin/core` to `lib/core`, which `include` and `exclude` leave out"
        ),
        "{}",
        stderr_of(&assertion)
    );
    assert!(!tree.home(".local/tool").exists());
}

#[test]
fn a_hard_link_to_an_entry_the_filters_keep_is_installed() {
    let server = serving(&[
        Member::File("lib/core", 0o644, "core\n"),
        Member::Hardlink("lib/alias", "lib/core"),
        Member::File("doc/core.md", 0o644, "# core\n"),
    ]);
    let tree = one_archive(&server, "exclude = \"doc\"\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(entries(&tree.home(".local/tool")), ["lib"]);
    assert_eq!(
        fs::read_to_string(tree.home(".local/tool/lib/alias")).expect("the link"),
        "core\n"
    );
}

#[test]
fn an_entry_a_filter_leaves_out_must_still_have_a_safe_path() {
    let server = serving(&[
        Member::File("bin/tool", 0o755, "run\n"),
        Member::File("../escape", 0o644, "x\n"),
    ]);
    let tree = one_archive(&server, "include = \"bin\"\n");

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("would be written outside it"),
        "{}",
        stderr_of(&assertion)
    );
    assert!(!tree.home(".local/tool").exists());
}

/// `executable` adds execute permission to the files it or a directory holding them matches,
/// and leaves directories and the files it does not match as the archive gives them.
#[test]
#[cfg(unix)]
fn executable_marks_the_files_it_matches_whatever_their_recorded_modes() {
    use std::os::unix::fs::PermissionsExt;

    let server = serving(RELEASE);
    let tree = one_archive(
        &server,
        "archive-root = \"*\"\nexecutable = [\"lib\", \"*.md\"]\n",
    );

    tree.batfiles().arg("sync").assert().success();

    let mode = |path: &str| {
        fs::metadata(tree.home(&format!(".local/tool/{path}")))
            .expect("an unpacked entry")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode("lib/core"), 0o755);
    assert_eq!(mode("lib/core.bak"), 0o755);
    assert_eq!(mode("README.md"), 0o755);
    assert_eq!(mode("bin/tool"), 0o755);
    assert_eq!(mode("bin"), 0o750, "a directory is not marked");
    assert_eq!(mode("lib"), 0o755);
}

#[test]
fn an_executable_pattern_that_marked_no_file_is_said_at_verbose() {
    let server = serving(RELEASE);
    let tree = one_archive(
        &server,
        "archive-root = \"*\"\nexecutable = [\"bin\", \"share\"]\n",
    );

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "-v", "sync"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "executable pattern `share` matched no file in {}/tool.tar.gz",
            server.address()
        )),
        "{stderr}"
    );
    assert!(!stderr.contains("`bin` matched no file"), "{stderr}");
}

#[test]
fn an_archive_filter_that_matched_nothing_is_said_at_verbose() {
    let server = serving(RELEASE);
    let tree = one_archive(
        &server,
        "archive-root = \"*\"\nexclude = [\"README.md\", \"*.txt\"]\n",
    );

    let assertion = tree
        .batfiles()
        .args(["--color", "never", "-v", "sync"])
        .assert()
        .success();

    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "exclude pattern `*.txt` matched nothing in {}/tool.tar.gz",
            server.address()
        )),
        "{stderr}"
    );
    assert!(!stderr.contains("`README.md` matched nothing"), "{stderr}");
    assert_eq!(entries(&tree.home(".local/tool")), ["bin", "lib"]);
}

/// Keep an existing destination directory without fetching or merging archive content.
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
    // The other actions account for the two requests.
    assert_eq!(server.requests(), 2);
}

/// Reject archives with escaping entry paths before installing any content.
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
        // Windows refuses an archive symlink before asking where it points.
        let unsupported = cfg!(windows)
            && matches!(members[0], Member::Symlink(..))
            && said.contains("symlinks are not supported on this platform");
        assert!(
            said.contains("would be written outside") || unsupported,
            "{what}: {said}"
        );
        assert!(
            !tree.home(".local").exists() || entries(&tree.home(".local")).is_empty(),
            "{what} left something beside the destination"
        );
    }
}

/// Reject link targets that escape after traversing another archive symlink. The external
/// destination exists so a missed validation cannot pass merely because the write fails.
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
        r#"[[actions]]
type = "fetch-archive"
source = "{}/tool.tar.gz"
dest = "~/.local/tool"
"#,
        server.address()
    ));
    // Create the outside directory so an unsafe write succeeds instead of failing for a missing
    // parent.
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

/// Reject entries nested under archive symlinks, even when the links remain inside the tree.
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

/// Preserve symlinks whose targets stay inside the extracted tree.
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

/// Install hardlinks to other archive entries.
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

/// Reject unsupported archive entry kinds without publishing a partial tree.
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
fn a_body_that_is_not_an_archive_batfiles_unpacks_says_what_it_is() {
    for (body, said) in [
        (
            &b"\x28\xb5\x2f\xfdand the rest of a zstd"[..],
            "a zstd archive",
        ),
        (&b"\xfd7zXZ\x00and the rest of an xz"[..], "an xz archive"),
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
        // The empty-body digest cannot match this archive.
        "sha256 = \"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\"\n",
    );

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(!tree.home(".local/tool").exists());
    assert!(
        stderr_of(&assertion).contains("does not match the declared sha256"),
        "{}",
        stderr_of(&assertion)
    );
    assert_eq!(entries(&tree.home(".local")), Vec::<String>::new());
}

/// An archive-root that could match no entry batfiles would unpack is refused as
/// the manifest is read, before anything is fetched.
#[test]
fn an_archive_root_that_names_nothing_inside_is_refused_before_anything_is_fetched() {
    let tree = Tree::new();
    for root in ["/tool", "../tool", ""] {
        tree.write_manifest(&format!(
            r#"[[actions]]
type = "fetch-archive"
source = "https://e.example/a.tar.gz"
dest = "~/.local/tool"
archive-root = "{root}"
"#,
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
    assert_eq!(mode(".local/tool/tool"), 0o750);
}

/// Apply the stripped root directory's permissions to the extraction destination.
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

/// Use readable default permissions when no archive entry specifies the root mode.
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

/// Reject `..` in archive roots during manifest validation.
#[test]
fn an_archive_root_that_climbs_is_refused_before_anything_is_fetched() {
    let tree = Tree::new();
    tree.write_manifest(
        r#"[[actions]]
type = "fetch-archive"
source = "https://e.example/a.tar.gz"
dest = "~/.local/tool"
archive-root = "releases/../tool"
"#,
    );

    let assertion = tree.batfiles().arg("sync").assert().failure();

    assert!(
        stderr_of(&assertion).contains("is not a path inside the archive"),
        "{}",
        stderr_of(&assertion)
    );
}

/// Populate read-only archive directories before applying their final permissions.
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
