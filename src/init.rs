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

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error as ThisError;

use crate::cli::InitArgs;
use crate::error::Error;
use crate::git;
use crate::manifest::Manifest;
use crate::output::Reporter;
use crate::paths;
use crate::remotes;

/// Git's exclusion list, which is where the tool-owned `remotes/` tree belongs.
const GITIGNORE: &str = ".gitignore";

/// Initialize the current directory.
pub(crate) fn run(args: &InitArgs, reporter: &Reporter) -> Result<(), Error> {
    let dir = std::env::current_dir().map_err(|source| Error::WorkingDirectory { source })?;

    // Validation is what protects a directory from being half-initialized:
    // creation itself is not transactional, so everything that can be checked is
    // checked before the first write.
    let missing = validate(&dir, crate::location::detect_os_home)?;
    create(&dir, &missing)?;

    reporter.info(&format!(
        "initialized batfiles repository in {}",
        dir.display()
    ));
    reporter.info(&created_line(&missing));
    warn_unignored_remotes(&dir, &missing, reporter);

    if !args.no_git_init {
        reporter.info(if git::init_repository(&dir).map_err(Failure::from)? {
            "initialized a Git repository"
        } else {
            "a Git repository already covers this directory"
        });
    }

    reporter.info("add files under files/ or bin/, then edit batfiles.toml");
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
const SKELETON: [Entry; 4] = [
    Entry::File {
        name: Manifest::FILE_NAME,
        content: BATFILES_TOML,
    },
    Entry::File {
        name: GITIGNORE,
        content: GITIGNORE_CONTENT,
    },
    Entry::Directory { name: "bin" },
    Entry::Directory { name: "files" },
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
/// [`detect_os_home`](crate::location::detect_os_home)'s signature so tests can
/// decide what the home is.
fn validate(
    dir: &Path,
    os_home: impl FnOnce() -> Result<PathBuf, Error>,
) -> Result<Vec<Entry>, Error> {
    // Presence, not kind: anything named `batfiles.toml` — a directory or a
    // dangling symlink included — means this directory already claims to be a
    // batfiles repository, and `init` is not a repair path for one.
    let manifest = dir.join(Manifest::FILE_NAME);
    if paths::occupied(&manifest)? {
        return Err(Failure::AlreadyInitialized { path: manifest }.into());
    }

    if is_os_home(dir, os_home) {
        return Err(Failure::HomeDirectory {
            path: dir.to_path_buf(),
        }
        .into());
    }

    let mut missing = Vec::new();
    for entry in SKELETON {
        let path = dir.join(entry.name());
        match occupant(&path, entry.kind())? {
            Occupant::Absent => missing.push(entry),
            Occupant::Matching => {}
            Occupant::Wrong => {
                return Err(Failure::WrongKind {
                    path,
                    expected: entry.kind().describe(),
                }
                .into());
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
    if !paths::occupied(path)? {
        return Ok(Occupant::Absent);
    }
    Ok(match fs::metadata(path) {
        Ok(metadata) if kind.matches(&metadata) => Occupant::Matching,
        _ => Occupant::Wrong,
    })
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
fn is_os_home(dir: &Path, os_home: impl FnOnce() -> Result<PathBuf, Error>) -> bool {
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
        let failed = |source| Error::Write {
            path: path.clone(),
            source,
        };
        match entry {
            Entry::Directory { .. } => fs::create_dir(&path).map_err(failed)?,
            Entry::File { content, .. } => fs::write(&path, content).map_err(failed)?,
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
            "{GITIGNORE} does not ignore the tool-owned `{tree}/` tree; consider adding \
             `/{tree}/` to it",
            tree = remotes::DIRECTORY
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
        .any(|line| line.contains(remotes::DIRECTORY))
}

/// Why an `init` failed, for the faults that are `init`'s own. Inspecting and
/// creating a path fail as the crate's shared read and write errors, which say
/// the same thing here as anywhere else.
#[derive(Debug, ThisError)]
pub(crate) enum Failure {
    /// Something named `batfiles.toml` is already here.
    #[error("{} already exists; this is already a batfiles repository", .path.display())]
    AlreadyInitialized { path: PathBuf },

    /// The current directory is the invoking user's OS home.
    #[error(
        "{} is your home directory; initialize a repository below it instead, such as ~/dotfiles",
        .path.display()
    )]
    HomeDirectory { path: PathBuf },

    /// A skeleton path exists as the wrong kind of filesystem node. Carries the
    /// kind it should have had, already phrased for the message.
    #[error(
        "{} exists and is not {expected}, which `init` needs it to be",
        .path.display()
    )]
    WrongKind {
        path: PathBuf,
        expected: &'static str,
    },

    /// Git would not run, or `git init` ran and failed. The hint rides along
    /// here rather than in `git.rs`, because the rest of `init` needs no Git at
    /// all and every other caller of that module does.
    #[error("{source} (use `--no-git-init` to skip Git initialization)")]
    Git {
        #[from]
        source: git::Failure,
    },
}

/// The starter manifest: a valid document that installs nothing, with one
/// commented sample per conventional directory so neither is left unexplained,
/// and one carrying a condition so that machine-dependent installation is
/// discoverable from the file itself.
const BATFILES_TOML: &str = r#"# Batfiles configuration.
#
# Uncomment and adjust the examples as you add files to this repository.

# [vars]
# profile = "personal"

# [[actions]]
# type = "symlink"
# source = "files/zshrc"
# dest = "~/.zshrc"

# Every direct child of `bin/`, linked into a directory on PATH.
# [[actions]]
# type = "symlink-dir"
# source-dir = "bin"
# dest-dir = "~/.local/bin"

# A starting point batfiles writes once and then leaves alone.
# [[actions]]
# type = "copy"
# source = "files/gitconfig"
# dest = "~/.gitconfig"

# [[actions]]
# type = "create-dir"
# dest = "~/.config"

# A condition decides whether an action applies to this machine.
# [[actions]]
# type = "symlink"
# source = "files/gitconfig.work"
# dest = "~/.gitconfig-work"
# when = "profile == 'work'"
"#;

/// `remotes/` is materialization output, regenerated from the manifest, so it
/// does not belong in history.
///
/// [`remotes::DIRECTORY`] owns the name, but `SKELETON` is a `const` and
/// `concat!` will not take a const path, so the tree is spelled out here. A unit
/// test pins the two together, which is what keeps this from being the drift the
/// single owner exists to prevent.
const GITIGNORE_CONTENT: &str = "/remotes/\n";

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in OS home no temporary directory can equal.
    fn os_home() -> Result<PathBuf, Error> {
        Ok(PathBuf::from("/os-home"))
    }

    /// Stand in for a user whose home cannot be determined.
    fn unavailable() -> Result<PathBuf, Error> {
        Err(Error::HomeUnavailable)
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
        fs::write(dir.path().join(GITIGNORE), "*.swp\nremotes/\n").expect("fixture");

        let missing = validate(dir.path(), os_home).expect("valid");
        assert_eq!(names(&missing), [Manifest::FILE_NAME, "bin"]);
    }

    #[test]
    fn a_manifest_of_any_kind_refuses_the_command() {
        for directory in [false, true] {
            let dir = temp();
            let manifest = dir.path().join(Manifest::FILE_NAME);
            if directory {
                fs::create_dir(&manifest).expect("fixture");
            } else {
                fs::write(&manifest, "").expect("fixture");
            }

            let error = validate(dir.path(), os_home).expect_err("already a repository");
            assert!(
                matches!(error, Error::Init(Failure::AlreadyInitialized { .. })),
                "unexpected error: {error}"
            );
            assert!(error.to_string().contains(Manifest::FILE_NAME), "{error}");
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
            matches!(error, Error::Init(Failure::HomeDirectory { .. })),
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
                name: Manifest::FILE_NAME,
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
        // any depth, directories included. Matching on `remotes/` would nag the
        // user who had written either of these.
        assert!(ignores_remotes("/remotes\n"));
        assert!(ignores_remotes("remotes\n"));

        assert!(!ignores_remotes(""));
        assert!(!ignores_remotes("*.swp\ntarget\n"));
    }

    #[test]
    fn the_starter_exclusion_list_still_names_the_tree_it_excludes() {
        // `SKELETON` is a `const`, so the starter `.gitignore` spells the tree
        // out instead of deriving it from `remotes::DIRECTORY`. This is what
        // makes that duplication safe: renaming the tree without editing the
        // literal fails here rather than silently shipping a `.gitignore` that
        // excludes a directory nothing writes to.
        assert_eq!(GITIGNORE_CONTENT, format!("/{}/\n", remotes::DIRECTORY));
        assert!(ignores_remotes(GITIGNORE_CONTENT));
    }

    #[test]
    fn the_starter_manifest_declares_nothing_at_all() {
        // Every sample is commented out, so a fresh repository installs nothing
        // until its owner uncomments one. That it *loads* — validation included
        // — is settled by the CLI test that syncs a freshly initialized
        // repository.
        let manifest: Manifest =
            toml::from_str(BATFILES_TOML).expect("the starter manifest should parse");
        assert!(manifest.actions.is_empty());
        assert!(manifest.remotes.is_empty());
        assert!(manifest.vars.is_empty());
    }

    #[test]
    fn every_git_failure_points_at_the_flag_that_avoids_git() {
        let unavailable = Failure::from(git::Failure::Unavailable {
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        });
        let message = unavailable.to_string();
        assert!(message.contains("--no-git-init"), "{message}");
        assert!(message.contains("could not run git"), "{message}");

        let failed = Failure::from(git::Failure::Failed {
            command: "init",
            path: PathBuf::from("/repo"),
            message: "fatal: cannot mkdir".to_owned(),
        });
        assert_eq!(
            failed.to_string(),
            "git init failed in /repo: fatal: cannot mkdir \
             (use `--no-git-init` to skip Git initialization)"
        );
    }
}
