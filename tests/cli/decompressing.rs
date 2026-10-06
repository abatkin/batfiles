//! CLI tests for `fetch-file` with `decompress`: a compressed download installed as the file it
//! holds, verified as it was downloaded, and refused where it holds no such file.

use std::fs;

use crate::support::*;

/// The program a release publishes compressed.
const TOOL: &str = "#!/bin/sh\necho tool\n";

/// A tree whose manifest fetches `/tool.gz` into `~/bin/tool`, decompressing it, with `extra`
/// fields added.
fn decompressing(server: &Server, extra: &str) -> Tree {
    let tree = Tree::new();
    tree.write_manifest(&format!(
        r#"[[actions]]
type = "fetch-file"
source = "{}/tool.gz"
dest = "~/bin/tool"
decompress = true
{extra}"#,
        server.address()
    ));
    tree
}

/// A server answering `/tool.gz` with exactly these bytes.
fn serving(body: Vec<u8>) -> Server {
    Server::new(&[("/tool.gz", Reply::Bytes(body))])
}

/// Run `sync` expecting failure, and return what it said; nothing may be installed.
fn refused(tree: &Tree) -> String {
    let assertion = tree.batfiles().arg("sync").assert().failure();
    assert!(!tree.home("bin/tool").exists());
    stderr_of(&assertion)
}

#[test]
fn a_gzip_or_bzip2_download_is_installed_as_the_file_it_holds() {
    for body in [gzipped(TOOL.as_bytes()), bzipped(TOOL.as_bytes())] {
        let server = serving(body);
        let tree = decompressing(&server, "");

        tree.batfiles().arg("sync").assert().success();

        assert_eq!(
            fs::read_to_string(tree.home("bin/tool")).expect("the decompressed file"),
            TOOL
        );
    }
}

#[test]
#[cfg(unix)]
fn a_decompressed_file_declared_executable_arrives_executable() {
    use std::os::unix::fs::PermissionsExt;

    let server = serving(gzipped(TOOL.as_bytes()));
    let tree = decompressing(&server, "executable = true\n");

    tree.batfiles().arg("sync").assert().success();

    let mode = fs::metadata(tree.home("bin/tool"))
        .expect("the decompressed file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o755);
}

/// `sha256` is the digest a release publishes beside the compressed file it publishes.
#[test]
fn the_digest_is_of_the_download_rather_than_of_what_it_decompresses_to() {
    let body = gzipped(TOOL.as_bytes());

    let server = serving(body.clone());
    let tree = decompressing(&server, &format!("sha256 = \"{}\"\n", sha256(&body)));
    tree.batfiles().arg("sync").assert().success();
    assert_eq!(
        fs::read_to_string(tree.home("bin/tool")).expect("the decompressed file"),
        TOOL
    );

    let server = serving(body);
    let tree = decompressing(
        &server,
        &format!("sha256 = \"{}\"\n", sha256(TOOL.as_bytes())),
    );
    let said = refused(&tree);
    assert!(
        said.contains("does not match the declared sha256"),
        "{said}"
    );
}

#[test]
fn a_download_that_is_not_compressed_is_refused() {
    let server = serving(TOOL.as_bytes().to_vec());
    let tree = decompressing(&server, "");

    let said = refused(&tree);

    assert!(
        said.contains(&format!(
            "the file from {}/tool.gz is not compressed, and `decompress` reads a file \
             compressed with gzip or bzip2",
            server.address()
        )),
        "{said}"
    );
}

#[test]
fn a_compressed_tar_is_refused_for_fetch_archive_to_unpack() {
    let server = serving(tarball(&[Member::File("bin/tool", 0o755, TOOL)]));
    let tree = decompressing(&server, "");

    let said = refused(&tree);

    assert!(
        said.contains("is a gzip-compressed tar archive, which `fetch-archive` unpacks"),
        "{said}"
    );
}

#[test]
fn a_compressed_stream_cut_short_cannot_be_decompressed() {
    let mut body = gzipped(TOOL.repeat(64).as_bytes());
    body.truncate(body.len() / 2);
    let server = serving(body);
    let tree = decompressing(&server, "");

    let said = refused(&tree);

    assert!(said.contains("could not be decompressed"), "{said}");
}

#[test]
fn without_decompress_a_compressed_download_is_installed_as_it_arrived() {
    let body = gzipped(TOOL.as_bytes());
    let server = serving(body.clone());
    let tree = Tree::new();
    tree.write_manifest(&format!(
        "[[actions]]\ntype = \"fetch-file\"\nsource = \"{}/tool.gz\"\ndest = \"~/tool.gz\"\n",
        server.address()
    ));

    tree.batfiles().arg("sync").assert().success();

    assert_eq!(fs::read(tree.home("tool.gz")).expect("the download"), body);
}
