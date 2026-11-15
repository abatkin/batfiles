//! Launch Git with repository redirects cleared and decode its output.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::GitError;
use crate::error::Error;

/// Git environment variables cleared before subprocess execution.
/// The supported environment contract is in `docs/environment.md`.
const REDIRECTS: [&str; 12] = [
    // Where the repository is.
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_PREFIX",
    // Which parts of it a command reads and writes.
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    // Command-local configuration overrides, which are not redirects but are
    // cleared with them.
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
];

/// Run Git and capture both output streams.
pub(super) fn launch<S: AsRef<OsStr>>(dir: Option<&Path>, args: &[S]) -> Result<Output, GitError> {
    capture(command(dir, args))
}

/// A `git` command with the [`REDIRECTS`] cleared, run in `dir` where one is
/// given.
fn command<S: AsRef<OsStr>>(dir: Option<&Path>, args: &[S]) -> Command {
    let mut command = Command::new("git");
    command.args(args);
    for redirect in REDIRECTS {
        command.env_remove(redirect);
    }
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command
}

/// Run a `git` command, capturing both of its streams.
fn capture(mut command: Command) -> Result<Output, GitError> {
    command
        .output()
        .map_err(|source| GitError::Unavailable { source })
}

/// What git said about a failure, or the status it exited with when it said
/// nothing.
pub(super) fn stderr_summary(output: &Output) -> String {
    let said = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if said.is_empty() {
        format!("git exited with {}", output.status)
    } else {
        said
    }
}

/// Run a `git` command that has to succeed, and turn a failure into one error
/// carrying git's own diagnostic.
pub(super) fn run_git<S: AsRef<OsStr>>(
    dir: Option<&Path>,
    command: &'static str,
    args: &[S],
    dest: &Path,
) -> Result<Output, Error> {
    let output = launch(dir, args)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(GitError::Failed {
            command,
            path: dest.to_path_buf(),
            message: stderr_summary(&output),
        }
        .into())
    }
}

/// Run a Git query: exit 0 returns trimmed stdout, exit 1 returns None, and
/// other statuses fail with Git diagnostics. Only use for queries with this convention.
pub(super) fn query(
    dest: &Path,
    command: &'static str,
    args: &[&str],
) -> Result<Option<String>, Error> {
    let output = launch(Some(dest), args)?;
    match output.status.code() {
        Some(0) => Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        )),
        Some(1) => Ok(None),
        _ => Err(GitError::Failed {
            command,
            path: dest.to_path_buf(),
            message: stderr_summary(&output),
        }
        .into()),
    }
}

/// Ask Git whether `git_dir` is a repository directory. Ignore global and system configuration.
/// Return `false` for an unrecognized repository and propagate other Git failures.
pub(super) fn is_repository_directory(git_dir: &Path, dest: &Path) -> Result<bool, Error> {
    let mut probe = command(
        Some(dest),
        &[
            OsStr::new("rev-parse"),
            OsStr::new("--resolve-git-dir"),
            git_dir.as_os_str(),
        ],
    );
    probe
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        // The "not a gitdir" match below needs Git's untranslated messages.
        .env("LC_ALL", "C")
        .env_remove("LANGUAGE");
    let output = capture(probe)?;
    if output.status.success() {
        return Ok(true);
    }
    let said = String::from_utf8_lossy(&output.stderr);
    if output.status.code() == Some(128) && said.trim_start().starts_with("fatal: not a gitdir") {
        return Ok(false);
    }
    Err(GitError::Failed {
        command: "rev-parse",
        path: dest.to_path_buf(),
        message: stderr_summary(&output),
    }
    .into())
}

/// Decode a Git path, removing one line terminator. Preserve trailing spaces
/// and, on Unix, non-UTF-8 bytes.
pub(super) fn line_as_path(printed: &[u8]) -> Option<PathBuf> {
    let line = printed.strip_suffix(b"\n").unwrap_or(printed);
    as_path(line.strip_suffix(b"\r").unwrap_or(line))
}

/// One path git printed, as a path.
pub(super) fn as_path(printed: &[u8]) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(OsStr::from_bytes(printed)))
    }
    // On non-Unix platforms, reject paths that cannot be decoded as UTF-8.
    #[cfg(not(unix))]
    {
        std::str::from_utf8(printed).ok().map(PathBuf::from)
    }
}
