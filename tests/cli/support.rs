//! The fixtures every group of tests is written against: a throwaway tree
//! standing in for the four location roots, the local HTTP server the fetching
//! tests answer from, the manifests that declare one action, and the assertions
//! that read a tree back.

use std::fs;
use std::io::{Cursor, Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

use assert_cmd::Command;
use tempfile::TempDir;

/// A command with nothing selected, for the cases that resolve no roots:
/// `version`, `--help`, and anything clap rejects before dispatch.
///
/// It inherits the developer's location variables, which is harmless only
/// because nothing it runs consults them. Anything that resolves a root goes
/// through [`Tree`].
pub(crate) fn batfiles() -> Command {
    let mut command = Command::cargo_bin("batfiles").expect("the batfiles binary should be built");
    // The tests must not inherit the developer's own color environment.
    command.env_remove("BATFILES_COLOR").env_remove("NO_COLOR");
    // Nor their proxy. `fetch-url` honors these, so a developer or a runner
    // that sets one would send the fetching tests' loopback requests to it —
    // reaching a network the suite promises never to reach, and failing with
    // the fixture server untouched (`guidance.md`, "Test environments").
    for proxy in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env_remove(proxy);
    }
    command
}

/// A throwaway tree standing in for the four location roots, with an empty leaf
/// manifest in place.
///
/// `sync` opens the repository root, so tests point at directories that exist
/// rather than at fixed absolute paths. A path that is never opened — an
/// alternative home, an `$XDG_*` base — can still be written inline.
pub(crate) struct Tree {
    dir: TempDir,
}

impl Tree {
    /// The four roots as sibling directories, with `repo/batfiles.toml` empty
    /// but present, which is what a command needs to get past reading it.
    pub(crate) fn new() -> Self {
        let tree = Self::roots();
        tree.repository("repo");
        tree
    }

    /// The same roots, with the leaf repository copied from
    /// `tests/fixtures/<name>` rather than holding a manifest written inline.
    ///
    /// One directory per repository shape, and copied rather than pointed at:
    /// the suite writes only inside the temporary tree, so a fixture is never
    /// mutated in place by a run.
    pub(crate) fn fixture(name: &str) -> Self {
        let tree = Self::roots();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        copy_tree(&source, &tree.path("repo"));
        tree
    }

    /// The three roots that are directories in their own right, with nothing in
    /// the repository yet.
    pub(crate) fn roots() -> Self {
        let dir = tempfile::tempdir().expect("a temporary directory");
        for name in ["home", "config", "cache"] {
            fs::create_dir(dir.path().join(name)).expect("a root directory");
        }
        Self { dir }
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    /// The tree the four roots sit in, for the cases that run from inside it.
    #[cfg(unix)]
    pub(crate) fn root(&self) -> &Path {
        self.dir.path()
    }

    /// Create a repository directory holding an empty manifest, and return it.
    pub(crate) fn repository(&self, relative: &str) -> PathBuf {
        let repo = self.path(relative);
        fs::create_dir_all(&repo).expect("a repository directory");
        fs::write(repo.join("batfiles.toml"), "").expect("a manifest");
        repo
    }

    pub(crate) fn manifest(&self) -> PathBuf {
        self.path("repo").join("batfiles.toml")
    }

    /// Replace the leaf manifest.
    pub(crate) fn write_manifest(&self, contents: &str) {
        fs::write(self.manifest(), contents).expect("a manifest");
    }

    /// Point a copied fixture's `{server}` placeholders at a running server.
    ///
    /// A fixture cannot know which port one binds, and rewriting the manifest
    /// after the copy keeps the URL in the repository — where a reader of the
    /// fixture sees it — rather than in the test that drives it.
    pub(crate) fn point_at(&self, server: &Server) {
        let manifest = fs::read_to_string(self.manifest()).expect("the fixture manifest");
        assert!(
            manifest.contains("{server}"),
            "the fixture has no `{{server}}` to point at a server"
        );
        self.write_manifest(&manifest.replace("{server}", server.address()));
    }

    /// Put a file in the leaf repository, and return where it landed.
    pub(crate) fn repo_file(&self, relative: &str, contents: &str) -> PathBuf {
        let path = self.path("repo").join(relative);
        fs::create_dir_all(path.parent().expect("a parent")).expect("a source directory");
        fs::write(&path, contents).expect("a source file");
        path
    }

    /// A path inside the selected home, which need not exist.
    pub(crate) fn home(&self, relative: &str) -> PathBuf {
        self.path("home").join(relative)
    }

    /// The machine-local disabled lists, which need not exist.
    pub(crate) fn disabled(&self) -> PathBuf {
        self.path("config").join("disabled.toml")
    }

    /// `disabled.toml` as it stands, which a command must have written.
    pub(crate) fn disabled_document(&self) -> String {
        fs::read_to_string(self.disabled()).expect("disabled.toml should exist")
    }

    /// Put a `disabled.toml` in place verbatim, including shapes batfiles would
    /// never write itself.
    pub(crate) fn write_disabled(&self, document: &str) {
        fs::write(self.disabled(), document).expect("a disabled document");
    }

    /// A command with all four roots selected inside this tree.
    pub(crate) fn batfiles(&self) -> Command {
        let mut command = batfiles();
        command
            .env("BATFILES_HOME", self.path("home"))
            .env("BATFILES_DIR", self.path("repo"))
            .env("BATFILES_CONFIG_DIR", self.path("config"))
            .env("BATFILES_CACHE_DIR", self.path("cache"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_CACHE_HOME");
        command
    }
}

/// What the local server answers one path with.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Reply {
    /// The file itself.
    Body(&'static str),
    /// A path the server does not have, answered 404.
    Missing,
    /// An answer that is not a refusal and not a whole file either: a 204 with
    /// nothing in it, or a 206 holding one range of one.
    NotAWholeFile { status: u16, body: &'static str },
    /// A permanent move to another path on the same server, which is what a
    /// release URL does before it hands over a file.
    RedirectTo(&'static str),
}

/// A local HTTP server, so no test reaches the network (`guidance.md`, "Test
/// environments").
///
/// It binds an ephemeral port and counts what it was asked for, which is how a
/// dry-run test asserts the stronger thing: not that the tree is unchanged, but
/// that nothing was requested at all.
pub(crate) struct Server {
    server: Arc<tiny_http::Server>,
    address: String,
    requests: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
}

impl Server {
    /// Start a server answering each named path with the reply beside it.
    ///
    /// The routes are copied rather than borrowed, so a test can build one from
    /// a value it is looping over.
    pub(crate) fn new(routes: &[(&'static str, Reply)]) -> Self {
        let routes = routes.to_vec();
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("a local HTTP server"));
        let address = format!("http://{}", server.server_addr());
        let requests = Arc::new(AtomicUsize::new(0));

        let worker = {
            let server = Arc::clone(&server);
            let requests = Arc::clone(&requests);
            std::thread::spawn(move || {
                // Ends when `unblock` is called from `drop`, which is what
                // stops the thread outliving the test that started it.
                for request in server.incoming_requests() {
                    requests.fetch_add(1, Ordering::SeqCst);
                    let reply = routes
                        .iter()
                        .find(|(path, _)| *path == request.url())
                        .map_or(Reply::Missing, |(_, reply)| *reply);
                    let _ = request.respond(response(reply));
                }
            })
        };

        Self {
            server,
            address,
            requests,
            worker: Some(worker),
        }
    }

    /// Where the server is, as a manifest writes it: `http://127.0.0.1:<port>`.
    pub(crate) fn address(&self) -> &str {
        &self.address
    }

    /// How many requests have reached it.
    pub(crate) fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// One reply, as tiny_http sends it.
fn response(reply: Reply) -> tiny_http::Response<Cursor<Vec<u8>>> {
    match reply {
        Reply::Body(body) => tiny_http::Response::from_string(body),
        Reply::Missing => tiny_http::Response::from_string("no such file\n").with_status_code(404),
        Reply::NotAWholeFile { status, body } => {
            tiny_http::Response::from_string(body).with_status_code(status)
        }
        Reply::RedirectTo(path) => tiny_http::Response::from_string("moved\n")
            .with_status_code(301)
            .with_header(
                tiny_http::Header::from_bytes("Location", path).expect("a location header"),
            ),
    }
}

/// A server that promises a length, sends less than it, and hangs up, for the
/// one test about a transfer that does not finish.
///
/// Raw rather than a [`Server`] route: the length a `tiny_http` response sends
/// is the length it declares, and the connection outlives the response, so
/// neither half of a cut-short transfer can be expressed through it. Answers
/// exactly one request and then ends, which is all the test makes.
pub(crate) fn server_that_hangs_up(body: &'static str, promised: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a local socket");
    let address = format!("http://{}", listener.local_addr().expect("its address"));
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("a connection");
        // Enough of the request to have read it; what it asks for does not
        // change the answer.
        let _ = socket.read(&mut [0u8; 1024]);
        let _ = write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {promised}\r\nConnection: close\r\n\r\n{body}"
        );
        let _ = socket.flush();
    });
    address
}

pub(crate) fn display(path: &Path) -> String {
    path.display().to_string()
}

/// Copy a directory tree, creating `to` and everything beneath it.
pub(crate) fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("a destination directory");
    for entry in fs::read_dir(from).expect("a fixture directory") {
        let entry = entry.expect("a directory entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("a file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("a fixture file");
        }
    }
}

/// The names directly inside a directory, sorted, for asserting that a run
/// installed everything it should have and nothing else.
pub(crate) fn entries(dir: &Path) -> Vec<String> {
    let mut found: Vec<String> = fs::read_dir(dir)
        .expect("a readable directory")
        .map(|entry| {
            entry
                .expect("a directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    found.sort();
    found
}

/// Everything under a directory: names, types, symlink targets as written, and
/// file contents, sorted.
///
/// What a dry run is checked against, rather than the destinations a manifest
/// names — a `.batfiles-incomplete` staging node, a parent directory created on
/// the way, and a broken symlink cleared at an ancestor are none of them.
/// `Tree` gives the four roots as siblings, so this needs no exclusions.
///
/// Not gated: reading a tree back is portable, and only the dry-run tests that
/// drive symlink actions are not.
pub(crate) fn snapshot(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    record_into(root, root, &mut found);
    found.sort();
    found
}

fn record_into(root: &Path, dir: &Path, found: &mut Vec<String>) {
    for entry in fs::read_dir(dir).expect("a readable directory") {
        let entry = entry.expect("a directory entry");
        let path = entry.path();
        let name = display(path.strip_prefix(root).expect("a path under the root"));
        // Never followed: a symlink is a thing that is there, and what it
        // reaches is somebody else's part of the tree.
        let kind = entry.file_type().expect("a file type");
        if kind.is_symlink() {
            found.push(format!("{name} -> {}", display(&link_target(&path))));
        } else if kind.is_dir() {
            found.push(format!("{name}/"));
            record_into(root, &path, found);
        } else {
            let contents = fs::read(&path).expect("a readable file");
            found.push(format!("{name} = {}", String::from_utf8_lossy(&contents)));
        }
    }
}

/// A manifest declaring one symlink and nothing else.
pub(crate) fn one_symlink(source: &str, dest: &str) -> String {
    format!("[[actions]]\ntype = \"symlink\"\nsource = \"{source}\"\ndest = \"{dest}\"\n")
}

/// A manifest declaring one `symlink-dir` and nothing else.
pub(crate) fn one_symlink_dir(source_dir: &str, dest_dir: &str, dot_prefix: bool) -> String {
    format!(
        "[[actions]]\n\
         type = \"symlink-dir\"\n\
         source-dir = \"{source_dir}\"\n\
         dest-dir = \"{dest_dir}\"\n\
         dot-prefix = {dot_prefix}\n"
    )
}

/// A manifest declaring one `create-dir` and nothing else.
pub(crate) fn one_create_dir(dest: &str) -> String {
    format!("[[actions]]\ntype = \"create-dir\"\ndest = \"{dest}\"\n")
}

/// A manifest declaring one `copy` and nothing else.
pub(crate) fn one_copy(source: &str, dest: &str) -> String {
    format!("[[actions]]\ntype = \"copy\"\nsource = \"{source}\"\ndest = \"{dest}\"\n")
}

/// A manifest declaring one `copy-dir` and nothing else.
pub(crate) fn one_copy_dir(source_dir: &str, dest_dir: &str, dot_prefix: bool) -> String {
    format!(
        "[[actions]]\n\
         type = \"copy-dir\"\n\
         source-dir = \"{source_dir}\"\n\
         dest-dir = \"{dest_dir}\"\n\
         dot-prefix = {dot_prefix}\n"
    )
}

/// Where a symlink points, without following it.
pub(crate) fn link_target(path: &Path) -> PathBuf {
    fs::read_link(path)
        .unwrap_or_else(|error| panic!("{} is not a symlink: {error}", path.display()))
}

pub(crate) fn stderr_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stderr).into_owned()
}

/// A repository at `~/dotfiles`, which is where batfiles looks by default and
/// the layout in which a destination can reach the repository through `~`.
pub(crate) fn seeded_repository_in_the_home(tree: &Tree) -> PathBuf {
    let repo = tree.repository("home/dotfiles");
    fs::create_dir(repo.join("seed")).expect("a source directory");
    fs::write(repo.join("seed/a"), "x\n").expect("something to copy");
    repo
}

// The portable half of the `leaf` fixture: the actions that need no symlink,
// which the manifest declares first so that a platform which cannot make one
// still runs them before the refusal stops the list.

/// Every directory `tests/fixtures/leaf` creates outright, as opposed to the
/// ones made on the way to a destination.
pub(crate) const LEAF_DIRS: [&str; 1] = [".cache/zsh"];

/// Every file it seeds, and the repository file each one is a copy of, in the
/// order it seeds them.
///
/// Written out rather than read back from the manifest: a test that derives its
/// expectations from the file under test asserts nothing. The last two are the
/// one `copy-dir` action expanded — one entry per child, in sorted order,
/// because that is what the run produces.
///
/// `profile.zsh` is the one entry whose *source* is an assertion rather than a
/// restatement. Two actions seed that destination and a seed does not replace,
/// so the machine-specific file is there only because it was declared first;
/// naming it here makes every caller of [`assert_leaf_portable_actions`] a check
/// on declaration order, on every platform. [`LEAF_ORDERED_PAIR`] is the
/// dedicated version, and says what a failure here means.
pub(crate) const LEAF_SEEDS: [(&str, &str); 4] = [
    ("templates/gitconfig.local", ".config/git/local"),
    ("templates/profile.machine.zsh", ".config/zsh/profile.zsh"),
    ("zsh-local/env.zsh", ".config/zsh/local/env.zsh"),
    ("zsh-local/prompt.zsh", ".config/zsh/local/prompt.zsh"),
];

/// The two `leaf` actions that name one destination, as `(winner, loser)`
/// repository paths, and the destination they contend for.
///
/// The whole of what declaration order decides in the fixture: the first
/// declared lands, the second keeps what it finds.
pub(crate) const LEAF_ORDERED_PAIR: (&str, &str, &str) = (
    "templates/profile.machine.zsh",
    "templates/profile.zsh",
    ".config/zsh/profile.zsh",
);

/// Assert that every action in [`LEAF_DIRS`] and [`LEAF_SEEDS`] has been
/// carried out, which every platform can do.
pub(crate) fn assert_leaf_portable_actions(tree: &Tree) {
    for dest in LEAF_DIRS {
        assert!(
            tree.home(dest).is_dir(),
            "`{dest}` is not a directory that exists"
        );
    }
    for (source, dest) in LEAF_SEEDS {
        let installed = tree.home(dest);
        // A seed is the user's copy, not a view of the repository's file: what
        // it holds is what an editor would write to, and nothing links back.
        assert!(
            !installed.is_symlink(),
            "`{dest}` was linked rather than seeded"
        );
        assert_eq!(
            fs::read_to_string(&installed).unwrap_or_else(|error| panic!("`{dest}`: {error}")),
            fs::read_to_string(tree.path("repo").join(source)).expect("the repository file"),
            "`{dest}` does not hold what `{source}` holds"
        );
    }
}
