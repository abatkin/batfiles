//! `init`: lay the conventional leaf-repository skeleton into the current
//! directory.
//!
//! The command resolves no roots. It works on the current directory alone, and
//! the only home it consults is the invoking user's OS home — not the selected
//! destination home — so that `init` in a fresh shell cannot turn `$HOME` itself
//! into the dotfiles repository.
//!
//! Nothing existing is replaced. Every rule is checked before the first path is
//! created, so a refused `init` leaves the directory exactly as it found it, and
//! a path that already has the kind `init` wants is simply left alone.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use crate::cli::InitArgs;
use crate::config::{ConfigError, detect_os_home};
use crate::output::Reporter;
use crate::repo::{BatfilesConfig, Remote};

/// The bootstrap entry point of a leaf repository.
const INSTALL_SCRIPT: &str = "install.sh";

/// Git's exclusion list, which is where the tool-owned `remotes/` tree belongs.
const GITIGNORE: &str = ".gitignore";

/// Initialize the current directory.
pub(crate) fn run(args: &InitArgs, reporter: &Reporter) -> Result<(), Error> {
    let dir = std::env::current_dir().map_err(Error::CurrentDirectory)?;

    // Validation is what protects a directory from being half-initialized:
    // creation itself is not transactional, so everything that can be checked is
    // checked before the first write.
    let missing = validate(&dir, detect_os_home)?;
    create(&dir, &missing)?;

    reporter.info(&format!(
        "initialized batfiles repository in {}",
        dir.display()
    ));
    reporter.info(&created_line(&missing));
    warn_unignored_remotes(&dir, &missing, reporter);

    if !args.no_git_init {
        reporter.info(if git_init(&dir)? {
            "initialized a Git repository"
        } else {
            "a Git repository already covers this directory"
        });
    }

    reporter.info("add files under files/ or local-files/, then edit batfiles.toml");
    Ok(())
}

/// One conventional path `init` lays down.
///
/// The skeleton is a table rather than a sequence of statements because three
/// separate rules read it: what kind an existing path must be, what to create,
/// and what to report having created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    File {
        name: &'static str,
        content: &'static str,
    },
    Directory {
        name: &'static str,
    },
}

/// The skeleton, in creation order, so a run interrupted by an I/O error leaves
/// a predictable partial result.
///
/// `remotes/` is deliberately absent: it is generated, tool-owned
/// materialization data, and should appear only once something materializes a
/// remote. `.gitignore` excludes it instead.
const SKELETON: [Entry; 6] = [
    Entry::File {
        name: BatfilesConfig::FILE_NAME,
        content: BATFILES_TOML,
    },
    Entry::File {
        name: INSTALL_SCRIPT,
        content: INSTALL_SH,
    },
    Entry::File {
        name: GITIGNORE,
        content: GITIGNORE_CONTENT,
    },
    Entry::Directory { name: "bin" },
    Entry::Directory { name: "files" },
    Entry::Directory {
        name: "local-files",
    },
];

impl Entry {
    fn name(self) -> &'static str {
        match self {
            Self::File { name, .. } | Self::Directory { name } => name,
        }
    }

    fn kind(self) -> Kind {
        match self {
            Self::File { .. } => Kind::File,
            Self::Directory { .. } => Kind::Directory,
        }
    }

    /// How the entry is named in the report: a directory carries its trailing
    /// slash so the two kinds are told apart at a glance.
    fn label(self) -> String {
        match self {
            Self::File { name, .. } => name.to_owned(),
            Self::Directory { name } => format!("{name}/"),
        }
    }

    /// Whether a newly created file must be executable.
    ///
    /// Only the bootstrap script is, and its mode is set explicitly rather than
    /// left to the umask, so a fresh repository is the same everywhere.
    #[cfg(unix)]
    fn executable(self) -> bool {
        matches!(self, Self::File { name, .. } if name == INSTALL_SCRIPT)
    }
}

/// What a skeleton path must be when something already occupies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    File,
    Directory,
}

impl Kind {
    fn matches(self, metadata: &fs::Metadata) -> bool {
        match self {
            Self::File => metadata.is_file(),
            Self::Directory => metadata.is_dir(),
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Self::File => "a regular file",
            Self::Directory => "a directory",
        }
    }
}

/// Check every rule, and answer with the entries that still have to be created.
///
/// `os_home` supplies the invoking user's OS home, matching
/// [`detect_os_home`]'s signature so tests can decide what the home is.
fn validate(
    dir: &Path,
    os_home: impl FnOnce() -> Result<PathBuf, ConfigError>,
) -> Result<Vec<Entry>, Error> {
    // Presence, not kind: anything named `batfiles.toml` — a directory or a
    // dangling symlink included — means this directory already claims to be a
    // batfiles repository, and `init` is not a repair path for one.
    let manifest = dir.join(BatfilesConfig::FILE_NAME);
    match fs::symlink_metadata(&manifest) {
        Ok(_) => return Err(Error::AlreadyInitialized(manifest)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Inspect {
                path: manifest,
                source,
            });
        }
    }

    if is_os_home(dir, os_home) {
        return Err(Error::HomeDirectory(dir.to_path_buf()));
    }

    let mut missing = Vec::new();
    for entry in SKELETON {
        let path = dir.join(entry.name());
        match occupant(&path, entry.kind())? {
            Occupant::Absent => missing.push(entry),
            Occupant::Matching => {}
            Occupant::Wrong => {
                return Err(Error::WrongKind {
                    path,
                    expected: entry.kind().describe(),
                });
            }
        }
    }
    Ok(missing)
}

/// What already sits at a skeleton path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Occupant {
    Absent,
    Matching,
    Wrong,
}

/// Classify what occupies `path`, if anything.
///
/// Symlinks are followed here, unlike the `batfiles.toml` check above: a
/// `files -> /elsewhere` link pointing at a directory is a deliberate
/// arrangement, and `init` neither replaces it nor writes through it. A link to
/// the wrong kind, or one pointing at nothing, is a wrong kind like any other.
fn occupant(path: &Path, kind: Kind) -> Result<Occupant, Error> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Occupant::Absent),
        Err(source) => Err(Error::Inspect {
            path: path.to_path_buf(),
            source,
        }),
        Ok(_) => match fs::metadata(path) {
            Ok(metadata) if kind.matches(&metadata) => Ok(Occupant::Matching),
            _ => Ok(Occupant::Wrong),
        },
    }
}

/// Whether `dir` is the invoking user's OS home directory.
///
/// The comparison is between canonical paths: `current_dir` is symlink-resolved
/// on Unix while `$HOME` frequently is not, so the two only line up once both
/// have been resolved.
///
/// A home that cannot be determined — or cannot be canonicalized, which is what
/// a `$HOME` pointing at nothing gives — is not an answer of "yes". `init` needs
/// no home of its own; it needs one only to refuse this single directory, so
/// without one the check simply does not apply.
fn is_os_home(dir: &Path, os_home: impl FnOnce() -> Result<PathBuf, ConfigError>) -> bool {
    let Ok(home) = os_home() else {
        return false;
    };
    match (dir.canonicalize(), home.canonicalize()) {
        (Ok(dir), Ok(home)) => dir == home,
        _ => false,
    }
}

/// Create the missing entries, in order.
fn create(dir: &Path, missing: &[Entry]) -> Result<(), Error> {
    for entry in missing {
        let path = dir.join(entry.name());
        let failed = |source| Error::Create {
            path: path.clone(),
            source,
        };
        match entry {
            Entry::Directory { .. } => fs::create_dir(&path).map_err(failed)?,
            Entry::File { content, .. } => fs::write(&path, content).map_err(failed)?,
        }
        #[cfg(unix)]
        if entry.executable() {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).map_err(failed)?;
        }
    }
    Ok(())
}

/// The line naming what was created. Pre-existing paths are not listed: `init`
/// reports what it did, not what it found.
fn created_line(missing: &[Entry]) -> String {
    let names: Vec<String> = missing.iter().map(|entry| entry.label()).collect();
    format!("created {}", names.join(", "))
}

/// Warn when a `.gitignore` batfiles did not write leaves the tool-owned
/// `remotes/` tree tracked.
///
/// The file is the user's, so this reports rather than edits. A matching rule
/// can be spelled several ways, which makes a substring check a hint and not an
/// authoritative answer — one more reason not to rewrite the file on its word.
fn warn_unignored_remotes(dir: &Path, missing: &[Entry], reporter: &Reporter) {
    if missing.iter().any(|entry| entry.name() == GITIGNORE) {
        // Freshly written by `init`, so it already excludes the tree.
        return;
    }
    let Ok(document) = fs::read_to_string(dir.join(GITIGNORE)) else {
        // An unreadable `.gitignore` earns no diagnostic of its own: nothing
        // here depends on reading it.
        return;
    };
    if !ignores_remotes(&document) {
        // The bare name, unlike the initialized directory reported above: a
        // `.gitignore` is understood relative to the repository root.
        reporter.warn(&format!(
            "{GITIGNORE} does not ignore the tool-owned `{tree}/` tree; consider adding `/{tree}/` to it",
            tree = Remote::TREE_NAME
        ));
    }
}

/// Whether an exclusion list appears to cover the generated `remotes/` tree.
///
/// A deliberately loose substring match on the bare name, so `/remotes`,
/// `remotes/`, and `dotfiles/remotes/**` all count. This only decides whether to
/// emit a warning, and a false negative — nagging someone who already excluded
/// the tree — is the worse of the two errors.
fn ignores_remotes(document: &str) -> bool {
    document
        .lines()
        .any(|line| line.contains(Remote::TREE_NAME))
}

/// Initialize a Git repository unless one already covers `dir`, answering
/// whether one was created.
fn git_init(dir: &Path) -> Result<bool, Error> {
    if inside_work_tree(dir)? {
        return Ok(false);
    }

    let output = git(dir, &["init"])?;
    if !output.status.success() {
        return Err(Error::GitInit(message(&output)));
    }
    Ok(true)
}

/// Whether `dir` already sits inside a Git work tree, parent repositories
/// included.
///
/// A successful exit is not enough on its own: inside a bare repository's `.git`
/// directory `rev-parse` succeeds and prints `false`.
fn inside_work_tree(dir: &Path) -> Result<bool, Error> {
    let output = git(dir, &["rev-parse", "--is-inside-work-tree"])?;
    Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true")
}

/// Run `git` in `dir`, capturing both of its streams.
///
/// Neither stream may inherit batfiles' own: outside a repository `rev-parse`
/// prints `fatal: not a git repository`, which is an expected answer here rather
/// than something to show. Captured standard error is reported only when a
/// command actually fails.
fn git(dir: &Path, args: &[&str]) -> Result<Output, Error> {
    Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(Error::GitUnavailable)
}

/// What `git` said about a failure, if anything.
fn message(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

/// Why an `init` failed.
#[derive(Debug)]
pub(crate) enum Error {
    /// The current directory could not be determined, so there is nothing to
    /// initialize.
    CurrentDirectory(io::Error),
    /// Something named `batfiles.toml` is already here.
    AlreadyInitialized(PathBuf),
    /// The current directory is the invoking user's OS home.
    HomeDirectory(PathBuf),
    /// A skeleton path exists as the wrong kind of filesystem node. Carries the
    /// kind it should have had, already phrased for the message.
    WrongKind {
        path: PathBuf,
        expected: &'static str,
    },
    /// A path could not be examined during validation.
    Inspect { path: PathBuf, source: io::Error },
    /// A path could not be created. Whatever came before it stays.
    Create { path: PathBuf, source: io::Error },
    /// `git` could not be executed at all, typically because it is not on
    /// `PATH`.
    GitUnavailable(io::Error),
    /// `git init` ran and failed. Carries `git`'s own diagnostic.
    GitInit(String),
}

/// The hint that accompanies every Git failure: the rest of `init` does not need
/// Git at all.
const SKIP_GIT: &str = "use `--no-git-init` to skip Git initialization";

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentDirectory(source) => {
                write!(f, "could not determine the current directory: {source}")
            }
            Self::AlreadyInitialized(path) => write!(
                f,
                "{} already exists; this is already a batfiles repository",
                path.display()
            ),
            Self::HomeDirectory(path) => write!(
                f,
                "{} is your home directory; initialize a repository below it instead, such as ~/dotfiles",
                path.display()
            ),
            Self::WrongKind { path, expected } => write!(
                f,
                "{} exists and is not {expected}, which `init` needs it to be",
                path.display()
            ),
            Self::Inspect { path, source } => {
                write!(f, "could not inspect {}: {source}", path.display())
            }
            Self::Create { path, source } => {
                write!(f, "could not create {}: {source}", path.display())
            }
            Self::GitUnavailable(source) => {
                write!(f, "could not run `git`: {source} ({SKIP_GIT})")
            }
            Self::GitInit(message) if message.is_empty() => {
                write!(f, "`git init` failed ({SKIP_GIT})")
            }
            Self::GitInit(message) => write!(f, "`git init` failed: {message} ({SKIP_GIT})"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CurrentDirectory(source)
            | Self::Inspect { source, .. }
            | Self::Create { source, .. }
            | Self::GitUnavailable(source) => Some(source),
            Self::AlreadyInitialized(_)
            | Self::HomeDirectory(_)
            | Self::WrongKind { .. }
            | Self::GitInit(_) => None,
        }
    }
}

/// The starter manifest: a valid document that installs nothing, with one
/// commented sample per conventional directory so none of them is left
/// unexplained.
const BATFILES_TOML: &str = r#"# Batfiles configuration.
#
# Uncomment and adjust examples as you add files to this repository.

# [vars]
# profile = "personal"

# [[actions]]
# type = "symlink"
# source = "files/zshrc"
# dest = "~/.zshrc"

# [[actions]]
# type = "symlink"
# source-dir = "bin"
# dest-dir = "~/.local/bin"

# [[actions]]
# type = "copy"
# source = "local-files"
# dest = "~"
# dot-prefix = true

# [[actions]]
# type = "create-dir"
# dest = "~/.config"
"#;

/// A placeholder bootstrap script. A fuller one that can locate or download the
/// `batfiles` binary is a later design.
const INSTALL_SH: &str = r#"#!/bin/sh
set -eu

echo "install.sh bootstrap is not implemented yet." >&2
echo "Run: batfiles sync" >&2
exit 1
"#;

/// `remotes/` is materialization output, regenerated from the manifest, so it
/// does not belong in history.
///
/// [`Remote::TREE_NAME`] owns the name, but `SKELETON` is a `const` and
/// `concat!` will not take a const path, so the tree is spelled out here. A unit
/// test pins the two together, which is what keeps this from being the drift the
/// single owner exists to prevent.
const GITIGNORE_CONTENT: &str = "/remotes/\n";

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in OS home no temporary directory can equal.
    fn os_home() -> Result<PathBuf, ConfigError> {
        Ok(PathBuf::from("/os-home"))
    }

    /// Stand in for a user whose home cannot be determined.
    fn unavailable() -> Result<PathBuf, ConfigError> {
        Err(ConfigError::HomeUnavailable)
    }

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn names(entries: &[Entry]) -> Vec<&'static str> {
        entries.iter().map(|entry| entry.name()).collect()
    }

    #[test]
    fn an_empty_directory_needs_the_whole_skeleton() {
        let dir = temp();
        assert_eq!(
            names(&validate(dir.path(), os_home).expect("valid")),
            names(&SKELETON)
        );
    }

    #[test]
    fn a_matching_path_is_left_out_of_the_work() {
        let dir = temp();
        fs::create_dir(dir.path().join("files")).expect("fixture");
        fs::write(dir.path().join(INSTALL_SCRIPT), "mine\n").expect("fixture");

        let missing = validate(dir.path(), os_home).expect("valid");
        assert_eq!(
            names(&missing),
            [BatfilesConfig::FILE_NAME, GITIGNORE, "bin", "local-files"]
        );
    }

    #[test]
    fn a_manifest_of_any_kind_refuses_the_command() {
        for directory in [false, true] {
            let dir = temp();
            let manifest = dir.path().join(BatfilesConfig::FILE_NAME);
            if directory {
                fs::create_dir(&manifest).expect("fixture");
            } else {
                fs::write(&manifest, "").expect("fixture");
            }

            let error = validate(dir.path(), os_home).expect_err("already a repository");
            assert!(
                matches!(error, Error::AlreadyInitialized(_)),
                "unexpected error: {error}"
            );
            assert!(error.to_string().contains("batfiles.toml"), "{error}");
        }
    }

    #[test]
    fn a_wrong_kind_names_the_path_and_what_it_should_be() {
        let dir = temp();
        fs::write(dir.path().join("files"), "not a directory\n").expect("fixture");

        let error = validate(dir.path(), os_home).expect_err("wrong kind");
        let message = error.to_string();
        assert!(message.contains("files"), "{message}");
        assert!(message.contains("a directory"), "{message}");
    }

    #[test]
    fn the_home_directory_is_refused() {
        let dir = temp();
        let home = || Ok(dir.path().to_path_buf());
        let error = validate(dir.path(), home).expect_err("the home directory");
        assert!(
            matches!(error, Error::HomeDirectory(_)),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn a_home_that_cannot_be_determined_skips_the_check() {
        let dir = temp();
        assert!(validate(dir.path(), unavailable).is_ok());
    }

    #[test]
    fn a_home_that_does_not_exist_skips_the_check() {
        // Canonicalizing a `$HOME` pointing at nothing fails, which is not an
        // answer of "this is the home directory".
        let dir = temp();
        let absent = dir.path().join("absent");
        assert!(!is_os_home(dir.path(), || Ok(absent)));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_home_still_matches_the_current_directory() {
        // The case the canonicalization exists for: `$HOME` reached through a
        // symlink, which `current_dir` would have already resolved.
        let dir = temp();
        let home = dir.path().join("home");
        fs::create_dir(&home).expect("fixture");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&home, &link).expect("fixture");

        assert!(is_os_home(&home, || Ok(link)));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_judged_by_what_it_points_at() {
        let dir = temp();
        let target = dir.path().join("elsewhere");
        fs::create_dir(&target).expect("fixture");
        std::os::unix::fs::symlink(&target, dir.path().join("files")).expect("fixture");
        // A link to nothing resolves to nothing, which is not a directory.
        std::os::unix::fs::symlink(dir.path().join("nowhere"), dir.path().join("bin"))
            .expect("fixture");

        assert_eq!(
            occupant(&dir.path().join("files"), Kind::Directory).expect("inspect"),
            Occupant::Matching
        );
        assert_eq!(
            occupant(&dir.path().join("bin"), Kind::Directory).expect("inspect"),
            Occupant::Wrong
        );
    }

    #[test]
    fn the_report_marks_directories_and_lists_only_new_paths() {
        let created = [
            Entry::File {
                name: BatfilesConfig::FILE_NAME,
                content: "",
            },
            Entry::Directory { name: "bin" },
        ];
        assert_eq!(created_line(&created), "created batfiles.toml, bin/");
    }

    #[test]
    fn an_exclusion_list_covers_remotes_however_it_is_spelled() {
        assert!(ignores_remotes("/remotes/\n"));
        assert!(ignores_remotes("*.swp\nremotes/\n"));
        assert!(ignores_remotes("dotfiles/remotes/**\n"));
        // Both slashless spellings are real rules that cover the tree: a
        // gitignore pattern containing no slash matches an entry of that name at
        // any depth, directories included. Matching on `remotes/` used to nag
        // the user who had written either of these.
        assert!(ignores_remotes("/remotes\n"));
        assert!(ignores_remotes("remotes\n"));

        assert!(!ignores_remotes(""));
        assert!(!ignores_remotes("*.swp\ntarget\n"));
    }

    #[test]
    fn the_starter_exclusion_list_still_names_the_tree_it_excludes() {
        // `SKELETON` is a `const`, so the starter `.gitignore` spells the tree
        // out instead of deriving it from `Remote::TREE_NAME`. This is what
        // makes that duplication safe: renaming the tree without editing the
        // literal fails here rather than silently shipping a `.gitignore` that
        // excludes a directory nothing writes to.
        assert_eq!(GITIGNORE_CONTENT, format!("/{}/\n", Remote::TREE_NAME));
        assert!(ignores_remotes(GITIGNORE_CONTENT));
    }

    #[test]
    fn the_starter_manifest_is_a_valid_empty_configuration() {
        assert_eq!(
            BatfilesConfig::parse(BATFILES_TOML).expect("the starter manifest should parse"),
            BatfilesConfig::default()
        );
    }

    #[test]
    fn every_git_failure_points_at_the_flag_that_avoids_git() {
        let unavailable = Error::GitUnavailable(io::Error::from(io::ErrorKind::NotFound));
        assert!(unavailable.to_string().contains("--no-git-init"));
        assert!(unavailable.to_string().contains("`git`"));

        let failed = Error::GitInit("fatal: cannot mkdir".to_owned());
        assert_eq!(
            failed.to_string(),
            "`git init` failed: fatal: cannot mkdir (use `--no-git-init` to skip Git initialization)"
        );
        assert_eq!(
            Error::GitInit(String::new()).to_string(),
            "`git init` failed (use `--no-git-init` to skip Git initialization)"
        );
    }
}
