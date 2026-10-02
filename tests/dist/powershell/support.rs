//! A Windows machine for the PowerShell scripts: a scratch `LOCALAPPDATA`, a directory on `PATH`
//! for a batfiles of its own, and a release tree of compiled stand-ins served over loopback HTTP,
//! since PowerShell's web cmdlets read no `file://` URL.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

use assert_cmd::Command;
use sha2::{Digest as _, Sha256};
use tempfile::TempDir;

#[path = "../../common/stand_in.rs"]
mod stand_in;

pub(crate) use stand_in::stand_in;

/// The target every Windows architecture installs.
pub(crate) const WINDOWS: &str = "x86_64-pc-windows-msvc";

/// Its asset.
pub(crate) const ASSET: &str = "batfiles-x86_64-pc-windows-msvc.exe";

pub(crate) fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(crate) fn stdout_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stdout).replace("\r\n", "\n")
}

pub(crate) fn stderr_of(assertion: &assert_cmd::assert::Assert) -> String {
    String::from_utf8_lossy(&assertion.get_output().stderr).replace("\r\n", "\n")
}

/// The entries of `dir`, sorted, with hidden ones included.
pub(crate) fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
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

/// `path` in its long form, as `$PSScriptRoot` names it, without the `\\?\` prefix
/// canonicalizing gives it.
pub(crate) fn long(path: &Path) -> PathBuf {
    let resolved = fs::canonicalize(path).expect("a canonical path");
    match resolved
        .to_str()
        .and_then(|text| text.strip_prefix(r"\\?\"))
    {
        Some(plain) => PathBuf::from(plain),
        None => resolved,
    }
}

/// Write a stand-in batfiles at `path` that reports `version`.
pub(crate) fn put_stand_in(path: &Path, version: &str) {
    fs::create_dir_all(path.parent().expect("a parent")).expect("its directory");
    fs::write(path, stand_in(WINDOWS, version)).expect("a stand-in");
}

/// The `pwsh` this machine runs, found on the test's own `PATH`.
fn pwsh() -> PathBuf {
    let path = std::env::var_os("PATH").expect("a PATH");
    std::env::split_paths(&path)
        .map(|dir| dir.join("pwsh.exe"))
        .find(|candidate| candidate.is_file())
        .expect("pwsh on PATH")
}

/// A PowerShell single-quoted string holding `text`.
pub(crate) fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// A directory served over loopback HTTP, path for path, counting requests.
pub(crate) struct FileServer {
    server: Arc<tiny_http::Server>,
    url: String,
    requests: Arc<AtomicUsize>,
    worker: Option<JoinHandle<()>>,
}

impl FileServer {
    fn new(root: PathBuf) -> Self {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("a local server"));
        let url = format!("http://{}", server.server_addr());
        let requests = Arc::new(AtomicUsize::new(0));
        let worker = {
            let server = Arc::clone(&server);
            let requests = Arc::clone(&requests);
            std::thread::spawn(move || {
                for request in server.incoming_requests() {
                    requests.fetch_add(1, Ordering::SeqCst);
                    let relative = Path::new(request.url().trim_start_matches('/'));
                    let inside = relative
                        .components()
                        .all(|part| matches!(part, Component::Normal(_)));
                    let body = inside.then(|| fs::read(root.join(relative)).ok()).flatten();
                    let response = match body {
                        Some(body) => tiny_http::Response::from_data(body),
                        None => {
                            tiny_http::Response::from_string("no such file\n").with_status_code(404)
                        }
                    };
                    let _ = request.respond(response);
                }
            })
        };
        Self {
            server,
            url,
            requests,
            worker: Some(worker),
        }
    }

    pub(crate) fn requests(&self) -> usize {
        self.requests.load(Ordering::SeqCst)
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

/// A release tree on disk, served over loopback HTTP.
pub(crate) struct Tree {
    root: PathBuf,
    pub(crate) server: FileServer,
}

impl Tree {
    fn new(root: PathBuf) -> Self {
        fs::create_dir_all(&root).expect("a release tree");
        Self {
            server: FileServer::new(root.clone()),
            root,
        }
    }

    /// The tree's release base.
    pub(crate) fn url(&self) -> &str {
        &self.server.url
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// Publish `version`: a stand-in for Windows, its `SHA256SUMS` and `VERSION`, and the
    /// installer stamped with this tree's base, served as that release and, when `latest`, as
    /// the latest one.
    pub(crate) fn release(&self, version: &str, latest: bool) {
        let binary = stand_in(WINDOWS, version);
        let installer =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("dist/install.ps1"))
                .expect("the installer")
                .replacen(
                    "$BatfilesStampedBase = 'unstamped'",
                    &format!("$BatfilesStampedBase = '{}'", self.url()),
                    1,
                );
        let mut dirs = vec![format!("download/v{version}")];
        if latest {
            dirs.push("latest/download".into());
        }
        for dir in dirs {
            let dir = self.root.join(dir);
            fs::create_dir_all(&dir).expect("a release directory");
            fs::write(dir.join(ASSET), &binary).expect("the binary");
            fs::write(
                dir.join("SHA256SUMS"),
                format!("{}  {ASSET}\n", sha256(&binary)),
            )
            .expect("its checksums");
            fs::write(dir.join("VERSION"), format!("{version}\n")).expect("its version");
            fs::write(dir.join("install.ps1"), &installer).expect("its installer");
        }
    }
}

/// A Windows machine with no batfiles, and a release tree whose latest release is 1.2.3.
pub(crate) struct Machine {
    dir: TempDir,
    pub(crate) tree: Tree,
}

impl Machine {
    pub(crate) fn new() -> Self {
        let dir = TempDir::new().expect("a scratch directory");
        let tree = Tree::new(dir.path().join("tree"));
        tree.release("1.2.3", true);
        for sub in ["local", "on-path", "work"] {
            fs::create_dir(dir.path().join(sub)).expect("a directory");
        }
        Self { dir, tree }
    }

    pub(crate) fn path(&self, relative: &str) -> PathBuf {
        self.dir.path().join(relative)
    }

    pub(crate) fn install_dir(&self) -> PathBuf {
        self.path(r"local\Programs\batfiles")
    }

    /// Where the installer puts batfiles by default.
    pub(crate) fn install_location(&self) -> PathBuf {
        self.install_dir().join("batfiles.exe")
    }

    /// The directory on `PATH` where a test can put a batfiles of its own.
    pub(crate) fn on_path(&self) -> PathBuf {
        self.path("on-path")
    }

    /// What the installer left in its install directory.
    pub(crate) fn installed(&self) -> Vec<String> {
        if self.install_dir().exists() {
            entries(&self.install_dir())
        } else {
            Vec::new()
        }
    }

    /// `pwsh` running `command`, in the machine's environment: its own `LOCALAPPDATA`, a `PATH`
    /// of the machine's directory and Windows' own, an x86_64 processor, and none of the
    /// batfiles inputs.
    pub(crate) fn pwsh(&self, command: &str) -> Command {
        self.pwsh_with(&["-Command", command])
    }

    /// `pwsh` running the script at `script` with `args`, as `pwsh -File` does, in the
    /// machine's environment.
    pub(crate) fn pwsh_file(&self, script: &Path, args: &[&str]) -> Command {
        let script = script.display().to_string();
        let mut all = vec!["-File", script.as_str()];
        all.extend_from_slice(args);
        self.pwsh_with(&all)
    }

    fn pwsh_with(&self, args: &[&str]) -> Command {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let mut pwsh = Command::new(pwsh());
        pwsh.args(["-NoLogo", "-NoProfile", "-NonInteractive"])
            .args(args)
            .current_dir(self.path("work"))
            .env("LOCALAPPDATA", self.path("local"))
            .env(
                "PATH",
                format!("{};{root}\\System32;{root}", self.on_path().display()),
            )
            .env("PROCESSOR_ARCHITECTURE", "AMD64");
        for name in [
            "BATFILES_BASE",
            "BATFILES_VERSION",
            "BATFILES_BIN",
            "PROCESSOR_ARCHITEW6432",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
        ] {
            pwsh.env_remove(name);
        }
        pwsh
    }

    /// The installer as its `clone` one-liner runs it, with `args`, leaving batfiles' status as
    /// the command's.
    pub(crate) fn installer(&self, args: &[&str]) -> Command {
        self.installer_from(
            &format!("{}/latest/download/install.ps1", self.tree.url()),
            args,
        )
    }

    /// The installer at `url`, run as [`Self::installer`] runs it.
    pub(crate) fn installer_from(&self, url: &str, args: &[&str]) -> Command {
        let args: Vec<String> = args.iter().map(|arg| quoted(arg)).collect();
        let mut command = format!(
            "& ([scriptblock]::Create((Invoke-RestMethod {}))) {}",
            quoted(url),
            args.join(" ")
        );
        if !args.is_empty() {
            command.push_str("; exit $LASTEXITCODE");
        }
        self.pwsh(&command)
    }
}
