//! CLI tests for `fetch-archive` over zip archives: what they unpack, the modes they record or
//! leave out, and the entries refused before anything is installed.

use std::fs;

use zip::CompressionMethod;

use crate::support::*;

/// A tool published as a release zip: one versioned directory, an executable under it, a
/// file that is not one, and a link to the executable.
const TOOL: &[Member] = &[
    Member::Directory("tool-1.0/", 0o755),
    Member::Directory("tool-1.0/bin/", 0o750),
    Member::File("tool-1.0/bin/tool", 0o755, "#!/bin/sh\necho tool\n"),
    Member::File("tool-1.0/README.md", 0o644, "# tool\n"),
];

/// A tree whose manifest declares one `fetch-archive` of `/tool.zip`, with `extra` added.
fn one_zip(server: &Server, extra: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "fetch-archive"
source = "{}/tool.zip"
dest = "~/.local/tool"
{extra}"#,
        server.address()
    ));
    tree
}

/// A server answering `/tool.zip` with exactly these bytes.
fn serving(zip: Vec<u8>) -> Server {
    Server::new(&[("/tool.zip", Reply::Bytes(zip))])
}

/// Run `sync` expecting failure, and return what it said; nothing may be installed.
fn refused(tree: &Tree) -> String {
    let assertion = tree.batfiles().arg("sync").assert().failure();
    assert!(!tree.home(".local/tool").exists());
    stderr_of(&assertion)
}

#[test]
fn a_zip_is_unpacked_whichever_way_its_entries_are_compressed() {
    for method in [
        CompressionMethod::Stored,
        CompressionMethod::Deflated,
        CompressionMethod::Bzip2,
    ] {
        let server = serving(zip_archive(TOOL, method));
        let tree = one_zip(&server, "archive-root = \"*\"\n");

        let assertion = tree.batfiles().arg("sync").assert().success();

        assert_eq!(entries(&tree.home(".local/tool")), ["README.md", "bin"]);
        assert_eq!(
            fs::read_to_string(tree.home(".local/tool/bin/tool")).expect("the unpacked program"),
            "#!/bin/sh\necho tool\n",
            "{method}"
        );
        assert!(stderr_of(&assertion).contains(&format!(
            "extracted {} from {}/tool.zip",
            display(&tree.home(".local/tool")),
            server.address()
        )));
    }
}

#[test]
#[cfg(unix)]
fn a_zip_made_on_unix_keeps_the_modes_and_links_it_records() {
    use std::os::unix::fs::PermissionsExt;

    let mut members = TOOL.to_vec();
    members.push(Member::Symlink("tool-1.0/bin/alias", "tool"));
    let server = serving(zip_archive(&members, CompressionMethod::Deflated));
    let tree = one_zip(&server, "archive-root = \"*\"\n");

    tree.batfiles().arg("sync").assert().success();

    let mode = |path: &str| {
        fs::metadata(tree.home(path))
            .expect("an unpacked entry")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(".local/tool/bin/tool"), 0o755);
    assert_eq!(mode(".local/tool/README.md"), 0o644);
    assert_eq!(mode(".local/tool/bin"), 0o750);
    assert_eq!(
        link_target(&tree.home(".local/tool/bin/alias")),
        std::path::Path::new("tool")
    );
}

#[test]
fn a_zip_made_on_windows_takes_the_modes_an_unstated_one_defaults_to() {
    let zip = as_if_made_on_windows(zip_archive(TOOL, CompressionMethod::Deflated));
    let server = serving(zip);
    let tree = one_zip(&server, "archive-root = \"*\"\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(entries(&tree.home(".local/tool/bin")), ["tool"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mode = |path: &str| {
            fs::metadata(tree.home(path))
                .expect("an unpacked entry")
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode(".local/tool/bin/tool"), 0o644);
        assert_eq!(mode(".local/tool/bin"), 0o755);
        assert_eq!(mode(".local/tool"), 0o755);
    }
}

/// What makes a Windows release zip usable on Unix: marking its programs executable.
#[test]
#[cfg(unix)]
fn executable_marks_what_a_zip_made_on_windows_could_not() {
    use std::os::unix::fs::PermissionsExt;

    let zip = as_if_made_on_windows(zip_archive(TOOL, CompressionMethod::Deflated));
    let server = serving(zip);
    let tree = one_zip(&server, "archive-root = \"*\"\nexecutable = \"bin/*\"\n");

    tree.batfiles().arg("sync").assert().success();

    let mode = |path: &str| {
        fs::metadata(tree.home(path))
            .expect("an unpacked entry")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(".local/tool/bin/tool"), 0o755);
    assert_eq!(mode(".local/tool/README.md"), 0o644);
}

#[test]
fn filters_choose_what_a_zip_installs() {
    let server = serving(zip_archive(TOOL, CompressionMethod::Deflated));
    let tree = one_zip(&server, "archive-root = \"*\"\ninclude = \"bin\"\n");

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(entries(&tree.home(".local/tool")), ["bin"]);
    assert_eq!(entries(&tree.home(".local/tool/bin")), ["tool"]);
}

#[test]
#[cfg(unix)]
fn a_zip_symlink_pointing_out_of_the_tree_is_refused() {
    let server = serving(zip_archive(
        &[Member::Symlink("tool-1.0/escape", "../../outside")],
        CompressionMethod::Deflated,
    ));
    let tree = one_zip(&server, "");

    let said = refused(&tree);

    assert!(
        said.contains("would be written outside it: `tool-1.0/escape`"),
        "{said}"
    );
}

#[test]
fn a_zip_entry_climbing_out_of_the_tree_is_refused() {
    let server = serving(zip_archive(
        &[Member::File("../outside", 0o644, "escaped\n")],
        CompressionMethod::Deflated,
    ));
    let tree = one_zip(&server, "");

    let said = refused(&tree);

    assert!(
        said.contains("would be written outside it: `../outside`"),
        "{said}"
    );
}

#[test]
fn a_zip_entry_named_with_a_backslash_is_refused() {
    let server = serving(zip_archive(
        &[Member::File("bin\\tool", 0o755, "run\n")],
        CompressionMethod::Deflated,
    ));
    let tree = one_zip(&server, "");

    let said = refused(&tree);

    assert!(said.contains("holds a `\\`"), "{said}");
    assert!(said.contains("`bin\\\\tool`"), "{said}");
}

#[test]
fn an_encrypted_zip_entry_is_refused_by_name() {
    let zip = with_entry_encrypted(
        zip_archive(TOOL, CompressionMethod::Deflated),
        "tool-1.0/bin/tool",
    );
    let server = serving(zip);
    let tree = one_zip(&server, "");

    let said = refused(&tree);

    assert!(
        said.contains("has the encrypted entry `tool-1.0/bin/tool`"),
        "{said}"
    );
}

#[test]
fn a_zip_entry_compressed_with_another_method_is_refused_naming_it() {
    // 14 is LZMA.
    let zip = with_entry_method(
        zip_archive(TOOL, CompressionMethod::Stored),
        "tool-1.0/README.md",
        14,
    );
    let server = serving(zip);
    let tree = one_zip(&server, "");

    let said = refused(&tree);

    assert!(
        said.contains("compresses `tool-1.0/README.md` with Lzma"),
        "{said}"
    );
}

#[test]
fn an_empty_zip_is_refused_as_empty() {
    let server = serving(zip_archive(&[], CompressionMethod::Deflated));
    let tree = one_zip(&server, "");

    let said = refused(&tree);

    assert!(said.contains("is empty"), "{said}");
}

#[test]
fn a_body_that_begins_like_a_zip_and_is_not_one_cannot_be_read() {
    let server = serving(b"PK\x03\x04and nothing a zip holds".to_vec());
    let tree = one_zip(&server, "");

    let said = refused(&tree);

    assert!(said.contains("could not be read"), "{said}");
}
