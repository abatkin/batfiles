//! Running the scripts, and the stand-in binaries and release trees they run against.

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;

pub(crate) const LINUX: &str = "x86_64-unknown-linux-musl";
pub(crate) const WINDOWS: &str = "x86_64-pc-windows-msvc";

/// Every release target, as `install.sh` might ask for one.
pub(crate) const TARGETS: [&str; 5] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
    "x86_64-pc-windows-msvc",
];

/// The path of `dist/<script>`.
pub(crate) fn dist(script: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("dist")
        .join(script)
}

/// Run `dist/<script>` with `args`.
pub(crate) fn script(script: &str, args: &[&str]) -> Command {
    let mut command = Command::new("sh");
    command
        .arg(dist(script))
        .args(args)
        .env_remove("CARGO_TARGET_DIR");
    command
}

pub(crate) fn stderr_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stderr).into_owned()
}

pub(crate) fn stdout_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stdout).into_owned()
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn utf8(path: &Path) -> &str {
    path.to_str().expect("fixture paths are UTF-8")
}

/// Write an executable shell script.
pub(crate) fn executable(path: &Path, body: &str) {
    fs::write(path, format!("#!/bin/sh\n{body}")).expect("a script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("its mode");
}

/// Write a stand-in for batfiles `version` of `target`: it reports that version, and otherwise
/// prints its target, version, and arguments.
pub(crate) fn stand_in(path: &Path, target: &str, version: &str) {
    executable(
        path,
        &format!(
            "if [ \"$1\" = version ]; then echo \"batfiles {version}\"; exit 0; fi\n\
             echo \"{target} {version} ran: $*\"\n"
        ),
    );
}

/// The entries of `dir`, sorted, with hidden ones included.
pub(crate) fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(dir)
        .expect("a directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("UTF-8")
        })
        .collect();
    names.sort();
    names
}

/// A release tree on disk, served as `file://` URLs or over loopback HTTP.
pub(crate) struct Tree {
    root: PathBuf,
}

impl Tree {
    pub(crate) fn new(root: PathBuf) -> Self {
        fs::create_dir_all(&root).expect("a release tree");
        Self { root }
    }

    pub(crate) fn url(&self) -> String {
        format!("file://{}", self.root.display())
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// Assemble stand-ins for every target at `version`, stamped with this tree's URL, and serve
    /// them as that release and, when `latest`, as the latest one.
    pub(crate) fn release(&self, version: &str, latest: bool) {
        let staging = self.root.with_extension(format!("staging-{version}"));
        let bin = staging.join("bin");
        fs::create_dir_all(&bin).expect("a staging directory");
        for target in TARGETS {
            let exe = if target.contains("windows") {
                ".exe"
            } else {
                ""
            };
            stand_in(
                &bin.join(format!("batfiles-{target}{exe}")),
                target,
                version,
            );
        }
        let out = staging.join("out");
        script(
            "assemble.sh",
            &[version, &self.url(), utf8(&out), utf8(&bin)],
        )
        .assert()
        .success();

        let mut dirs = vec![format!("download/v{version}")];
        if latest {
            dirs.push("latest/download".into());
        }
        for dir in dirs {
            let dest = self.root.join(dir);
            fs::create_dir_all(&dest).expect("a release directory");
            for name in entries(&out) {
                fs::copy(out.join(&name), dest.join(&name)).expect("a release asset");
            }
        }
    }
}

/// A machine with no batfiles, a release tree whose latest release is 1.2.3, and `uname` and
/// `sysctl` answering as the test says.
pub(crate) struct Machine {
    dir: TempDir,
    pub(crate) tree: Tree,
}

impl Machine {
    pub(crate) fn new() -> Self {
        let dir = TempDir::new().expect("a scratch directory");
        let tree = Tree::new(dir.path().join("tree"));
        tree.release("1.2.3", true);
        for sub in ["home", "fakes", "on-path"] {
            fs::create_dir(dir.path().join(sub)).expect("a directory");
        }
        executable(
            &dir.path().join("fakes/uname"),
            "case $1 in -s) echo \"$FAKE_OS\" ;; -m) echo \"$FAKE_ARCH\" ;; *) exit 1 ;; esac\n",
        );
        executable(
            &dir.path().join("fakes/sysctl"),
            "[ -n \"${FAKE_ARM64:-}\" ] || exit 1\necho \"$FAKE_ARM64\"\n",
        );
        Self { dir, tree }
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    pub(crate) fn install_dir(&self) -> PathBuf {
        self.path("home/.local/bin")
    }

    /// The directory on `PATH` where a test can put a batfiles of its own.
    pub(crate) fn on_path(&self) -> PathBuf {
        self.path("on-path")
    }

    /// The installer as the one-liner runs it: piped into `sh -s --`, which takes any further
    /// arguments the command is given. The environment is only what a fresh shell would have.
    pub(crate) fn installer(&self) -> Command {
        self.piped(&fs::read(self.tree.path("latest/download/install.sh")).expect("installer"))
    }

    pub(crate) fn piped(&self, script: &[u8]) -> Command {
        // By absolute path, since a test may leave no `sh` on PATH.
        self.piped_into(Path::new("/bin/sh"), script)
    }

    /// The installer piped into `shell` rather than `/bin/sh`.
    pub(crate) fn piped_into(&self, shell: &Path, script: &[u8]) -> Command {
        let mut command = self.shell(shell);
        command.args(["-s", "--"]).write_stdin(script.to_vec());
        command
    }

    /// `shell`, in only the environment a fresh shell on this machine would have.
    pub(crate) fn shell(&self, shell: &Path) -> Command {
        let mut command = Command::new(shell);
        command
            .env_clear()
            .env(
                "PATH",
                format!(
                    "{}:{}:/usr/bin:/bin",
                    self.path("fakes").display(),
                    self.on_path().display()
                ),
            )
            .env("HOME", self.path("home"))
            .env("FAKE_OS", "Linux")
            .env("FAKE_ARCH", "x86_64");
        command
    }

    /// What the installer left in its install directory.
    pub(crate) fn installed(&self) -> Vec<String> {
        if self.install_dir().exists() {
            entries(&self.install_dir())
        } else {
            Vec::new()
        }
    }
}

/// Whether `path` is the stand-in for `target` at `version`.
pub(crate) fn is_stand_in(path: &Path, target: &str, version: &str) -> bool {
    let output = std::process::Command::new(path)
        .arg("probe")
        .output()
        .expect("the stand-in runs");
    String::from_utf8_lossy(&output.stdout) == format!("{target} {version} ran: probe\n")
}

#[cfg(target_os = "linux")]
pub(crate) use file_server::FileServer;

/// Serving a release tree over HTTP, which only the Linux-only wget test needs.
#[cfg(target_os = "linux")]
mod file_server {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::thread::JoinHandle;

    /// A loopback HTTP server for the files under a directory.
    pub(crate) struct FileServer {
        server: Arc<tiny_http::Server>,
        address: String,
        worker: Option<JoinHandle<()>>,
    }

    impl FileServer {
        pub(crate) fn new(root: PathBuf) -> Self {
            let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("a local server"));
            let address = format!("http://{}", server.server_addr());
            let worker = {
                let server = Arc::clone(&server);
                std::thread::spawn(move || {
                    for request in server.incoming_requests() {
                        let path = root.join(request.url().trim_start_matches('/'));
                        let response = match fs::read(&path) {
                            Ok(body) => tiny_http::Response::from_data(body),
                            Err(_) => tiny_http::Response::from_data(b"no such file\n".to_vec())
                                .with_status_code(404),
                        };
                        let _ = request.respond(response);
                    }
                })
            };
            Self {
                server,
                address,
                worker: Some(worker),
            }
        }

        pub(crate) fn address(&self) -> &str {
            &self.address
        }
    }

    impl Drop for FileServer {
        fn drop(&mut self) {
            self.server.unblock();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
}

/// A scratch directory holding stand-in binaries in `bin/`.
pub(crate) struct Scratch {
    dir: TempDir,
}

impl Scratch {
    /// Stand-ins for the Linux and Windows binaries.
    pub(crate) fn new() -> Self {
        let scratch = Self {
            dir: TempDir::new().expect("a scratch directory"),
        };
        fs::create_dir(scratch.bin()).expect("a binaries directory");
        scratch.binary(&format!("batfiles-{LINUX}"));
        scratch.binary(&format!("batfiles-{WINDOWS}.exe"));
        scratch
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    pub(crate) fn bin(&self) -> PathBuf {
        self.path("bin")
    }

    pub(crate) fn binary(&self, name: &str) {
        fs::write(self.bin().join(name), format!("stand-in for {name}\n")).expect("a binary");
    }

    /// `dist/assemble.sh` for version 1.2.3 into `out/`, requiring `targets`.
    pub(crate) fn assemble(&self, base: &str, targets: &str) -> Command {
        let out = self.path("out");
        let bin = self.bin();
        script(
            "assemble.sh",
            &[
                "1.2.3",
                base,
                out.to_str().expect("UTF-8"),
                bin.to_str().expect("UTF-8"),
                targets,
            ],
        )
    }

    pub(crate) fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.path(relative)).expect("an assembled file")
    }

    /// Serve `out/` as release 1.2.3 and as the latest release, returning the tree's base URL.
    pub(crate) fn publish(&self) -> String {
        let tree = self.path("tree");
        for dir in ["download/v1.2.3", "latest/download"] {
            fs::create_dir_all(tree.join(dir)).expect("a release directory");
            for entry in fs::read_dir(self.path("out")).expect("the assembled release") {
                let entry = entry.expect("an assembled file");
                fs::copy(entry.path(), tree.join(dir).join(entry.file_name())).expect("a copy");
            }
        }
        format!("file://{}", tree.display())
    }
}
