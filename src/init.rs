//! Initialize the conventional leaf-repository layout in the current directory, ending with the
//! stubs that install a checkout of it, or add only the stubs to an existing repository. Reject
//! the invoking user's OS home and validate existing paths before writing. Keep existing entries
//! of the expected kind.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error as ThisError;

use crate::cli::InitArgs;
use crate::env::Environment;
use crate::error::Error;
use crate::git;
use crate::manifest::Manifest;
use crate::output::Reporter;
use crate::paths;
use crate::release::ReleaseBase;
use crate::remotes;

/// Git's exclusion list, which is where the tool-owned `remotes/` tree belongs.
const GITIGNORE: &str = ".gitignore";

/// What [`Stub::render`] replaces with the release base.
const STUB_BASE: &str = "@BATFILES_BASE@";

/// Each stub's second line, which identifies its format.
const STUB_MARKER: &str = "# batfiles-stub 1";

/// A stub that installs a checkout, last in the skeleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stub {
    /// `install.sh`, for a Unix shell.
    Shell,
    /// `install.ps1`, for PowerShell on Windows.
    PowerShell,
}

impl Stub {
    /// Every stub, in creation order.
    const ALL: [Self; 2] = [Self::Shell, Self::PowerShell];

    fn name(self) -> &'static str {
        match self {
            Self::Shell => "install.sh",
            Self::PowerShell => "install.ps1",
        }
    }

    /// The template, whose base line holds [`STUB_BASE`].
    fn template(self) -> &'static str {
        match self {
            Self::Shell => include_str!("stub.sh"),
            Self::PowerShell => include_str!("stub.ps1"),
        }
    }

    /// The stub, stamped with `base`.
    fn render(self, base: &ReleaseBase) -> String {
        self.template().replacen(STUB_BASE, base.as_str(), 1)
    }
}

/// Initialize the current directory, stamping the stubs with the release base `env` selects.
/// With `--stubs`, add only the stubs a repository there lacks.
pub(crate) fn run(args: &InitArgs, env: &Environment, reporter: &Reporter) -> Result<(), Error> {
    let base = ReleaseBase::from_env(env)?;
    let dir = std::env::current_dir().map_err(|source| Error::WorkingDirectory { source })?;
    if args.stubs {
        return add_stubs(&dir, &base, reporter);
    }

    // Validate before writing; layout creation is not transactional.
    let missing = validate(&dir, crate::location::detect_os_home)?;
    create(&dir, &missing, &base)?;

    reporter.info(&format!(
        "initialized batfiles repository in {}",
        dir.display()
    ));
    reporter.info(&created_line(&missing));
    warn_unignored_remotes(&dir, &missing, reporter);
    warn_foreign_stubs(&dir, &missing, reporter);

    if !args.no_git_init {
        reporter.info(if git::init_repository(&dir).map_err(InitError::from)? {
            "initialized a Git repository"
        } else {
            "a Git repository already covers this directory"
        });
    }

    reporter.info("add files under files/ or bin/, then edit batfiles.toml");
    Ok(())
}

/// Write the stubs a repository in `dir` lacks, leaving everything else as it is.
fn add_stubs(dir: &Path, base: &ReleaseBase, reporter: &Reporter) -> Result<(), Error> {
    let manifest = dir.join(Manifest::FILE_NAME);
    if occupant(&manifest, SkeletonKind::File)? != Occupant::Matching {
        return Err(InitError::NotARepository { path: manifest }.into());
    }
    let stubs = Stub::ALL.map(SkeletonEntry::Stub);
    let missing = missing_from(dir, &stubs)?;
    create(dir, &missing, base)?;

    if missing.is_empty() {
        reporter.info(&format!(
            "{} already has every stub; nothing was added",
            dir.display()
        ));
    } else {
        reporter.info(&format!("in {}, {}", dir.display(), created_line(&missing)));
    }
    warn_foreign_stubs(dir, &missing, reporter);
    Ok(())
}

/// A file or directory in the initial repository layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkeletonEntry {
    File {
        name: &'static str,
        content: &'static str,
    },
    Directory {
        name: &'static str,
    },
    /// A stub, whose content carries the release base.
    Stub(Stub),
}

/// Initial repository entries in creation order. `remotes/` is created on materialization and
/// excluded by `.gitignore`.
const SKELETON: [SkeletonEntry; 6] = [
    SkeletonEntry::File {
        name: Manifest::FILE_NAME,
        content: BATFILES_TOML,
    },
    SkeletonEntry::File {
        name: GITIGNORE,
        content: GITIGNORE_CONTENT,
    },
    SkeletonEntry::Directory { name: "bin" },
    SkeletonEntry::Directory { name: "files" },
    SkeletonEntry::Stub(Stub::Shell),
    SkeletonEntry::Stub(Stub::PowerShell),
];

impl SkeletonEntry {
    fn name(self) -> &'static str {
        match self {
            Self::File { name, .. } | Self::Directory { name } => name,
            Self::Stub(stub) => stub.name(),
        }
    }

    fn kind(self) -> SkeletonKind {
        match self {
            Self::File { .. } | Self::Stub(_) => SkeletonKind::File,
            Self::Directory { .. } => SkeletonKind::Directory,
        }
    }

    /// Return the entry name, with a trailing slash for directories.
    fn label(self) -> String {
        match self {
            Self::File { .. } | Self::Stub(_) => self.name().to_owned(),
            Self::Directory { name } => format!("{name}/"),
        }
    }
}

/// What a skeleton path must be when something already occupies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkeletonKind {
    File,
    Directory,
}

impl SkeletonKind {
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

/// Validate the target directory and return missing layout entries. `os_home` supplies the
/// invoking user's OS home.
fn validate(
    dir: &Path,
    os_home: impl FnOnce() -> Result<PathBuf, Error>,
) -> Result<Vec<SkeletonEntry>, Error> {
    // Refuse any existing manifest node, including a directory or broken symlink.
    let manifest = dir.join(Manifest::FILE_NAME);
    if paths::occupied(&manifest)? {
        return Err(InitError::AlreadyInitialized { path: manifest }.into());
    }

    if is_os_home(dir, os_home) {
        return Err(InitError::HomeDirectory {
            path: dir.to_path_buf(),
        }
        .into());
    }

    missing_from(dir, &SKELETON)
}

/// The `entries` nothing occupies in `dir`, failing on one occupied by the wrong kind of node.
fn missing_from(dir: &Path, entries: &[SkeletonEntry]) -> Result<Vec<SkeletonEntry>, Error> {
    let mut missing = Vec::new();
    for &entry in entries {
        let path = dir.join(entry.name());
        match occupant(&path, entry.kind())? {
            Occupant::Absent => missing.push(entry),
            Occupant::Matching => {}
            Occupant::WrongKind => {
                return Err(InitError::WrongKind {
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
    WrongKind,
}

/// Classify `path` against the expected kind, following symlinks. Broken symlinks and targets
/// of another kind are mismatches.
fn occupant(path: &Path, kind: SkeletonKind) -> Result<Occupant, Error> {
    if !paths::occupied(path)? {
        return Ok(Occupant::Absent);
    }
    Ok(match fs::metadata(path) {
        Ok(metadata) if kind.matches(&metadata) => Occupant::Matching,
        _ => Occupant::WrongKind,
    })
}

/// Compare `dir` with the invoking user's OS home using canonical paths. Return `false` if the
/// home cannot be determined or either path cannot be canonicalized.
fn is_os_home(dir: &Path, os_home: impl FnOnce() -> Result<PathBuf, Error>) -> bool {
    let Ok(home) = os_home() else {
        return false;
    };
    match (dir.canonicalize(), home.canonicalize()) {
        (Ok(dir), Ok(home)) => dir == home,
        _ => false,
    }
}

/// Create the missing entries, in order, stamping each stub with `base`.
fn create(dir: &Path, missing: &[SkeletonEntry], base: &ReleaseBase) -> Result<(), Error> {
    for entry in missing {
        let path = dir.join(entry.name());
        let failed = |source| Error::Write {
            path: path.clone(),
            source,
        };
        match entry {
            SkeletonEntry::Directory { .. } => fs::create_dir(&path).map_err(failed)?,
            SkeletonEntry::File { content, .. } => fs::write(&path, content).map_err(failed)?,
            SkeletonEntry::Stub(stub) => {
                fs::write(&path, stub.render(base)).map_err(failed)?;
                if *stub == Stub::Shell {
                    make_executable(&path).map_err(failed)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

/// Windows has no executable bit to set.
#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Warn about each existing stub that is not a batfiles stub. Leave the files unchanged.
fn warn_foreign_stubs(dir: &Path, missing: &[SkeletonEntry], reporter: &Reporter) {
    for stub in Stub::ALL {
        if missing.contains(&SkeletonEntry::Stub(stub)) {
            continue;
        }
        let Ok(document) = fs::read_to_string(dir.join(stub.name())) else {
            // An unreadable file is reported nowhere else either; it does not block
            // initialization.
            continue;
        };
        if document.lines().nth(1) != Some(STUB_MARKER) {
            reporter.warn(&format!(
                "{} is not a batfiles stub, so it is left as it is; a checkout of this \
                 repository is installed with `batfiles sync --bootstrap` instead",
                stub.name()
            ));
        }
    }
}

/// Format a summary listing only newly created entries.
fn created_line(missing: &[SkeletonEntry]) -> String {
    let names: Vec<String> = missing.iter().map(|entry| entry.label()).collect();
    format!("created {}", names.join(", "))
}

/// Warn if an existing `.gitignore` does not appear to exclude `remotes/`. Leave the file
/// unchanged.
fn warn_unignored_remotes(dir: &Path, missing: &[SkeletonEntry], reporter: &Reporter) {
    if missing.iter().any(|entry| entry.name() == GITIGNORE) {
        // Freshly written by `init`, so it already excludes the tree.
        return;
    }
    let Ok(document) = fs::read_to_string(dir.join(GITIGNORE)) else {
        // An unreadable ignore file does not block initialization.
        return;
    };
    if !ignores_remotes(&document) {
        reporter.warn(&format!(
            "{GITIGNORE} does not ignore the tool-owned `{tree}/` tree; consider adding \
             `/{tree}/` to it",
            tree = remotes::DIRECTORY
        ));
    }
}

/// Return whether any line contains `remotes`. This is a warning heuristic, not a Git pattern
/// parser.
fn ignores_remotes(document: &str) -> bool {
    document
        .lines()
        .any(|line| line.contains(remotes::DIRECTORY))
}

/// `init`'s own failures. Filesystem failures use the crate's shared read and
/// write errors.
#[derive(Debug, ThisError)]
pub(crate) enum InitError {
    /// Something named `batfiles.toml` is already here.
    #[error("{} already exists; this is already a batfiles repository", .path.display())]
    AlreadyInitialized { path: PathBuf },

    /// `--stubs` was given somewhere with no repository to add them to.
    #[error(
        "{} is not a file; `init --stubs` adds the stubs to an existing batfiles repository, \
         and plain `init` creates one",
        .path.display()
    )]
    NotARepository { path: PathBuf },

    /// The current directory is the invoking user's OS home.
    #[error(
        "{} is your home directory; initialize a repository below it instead, such as ~/dotfiles",
        .path.display()
    )]
    HomeDirectory { path: PathBuf },

    /// An existing layout path has the wrong filesystem kind; `expected` describes the required
    /// kind.
    #[error(
        "{} exists and is not {expected}, which `init` needs it to be",
        .path.display()
    )]
    WrongKind {
        path: PathBuf,
        expected: &'static str,
    },

    /// Git would not run, or `git init` failed. Adds the `--no-git-init` hint,
    /// which applies only to `init`.
    #[error("{source} (use `--no-git-init` to skip Git initialization)")]
    Git {
        #[from]
        source: git::GitError,
    },
}

/// An empty starter manifest with commented action and condition examples.
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

/// Initial `.gitignore` contents excluding generated remote materializations.
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

    fn names(entries: &[SkeletonEntry]) -> Vec<&'static str> {
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
        assert_eq!(
            names(&missing),
            [Manifest::FILE_NAME, "bin", "install.sh", "install.ps1"]
        );
    }

    #[test]
    fn each_stub_template_has_its_marker_and_one_base_to_stamp() {
        let base = ReleaseBase::try_from("https://example.com/batfiles").expect("valid");
        for (stub, stamped) in [
            (
                Stub::Shell,
                "\nBATFILES_BASE=${BATFILES_BASE:-'https://example.com/batfiles'}\n",
            ),
            (
                Stub::PowerShell,
                "\n$BatfilesBase = if ($env:BATFILES_BASE) { $env:BATFILES_BASE } else { \
                 'https://example.com/batfiles' }\n",
            ),
        ] {
            let template = stub.template();
            assert_eq!(template.lines().nth(1), Some(STUB_MARKER), "{stub:?}");
            // LF endings and ASCII, whatever the checkout it was built from: Windows
            // PowerShell reads a file without a byte-order mark in the system code page.
            assert!(!template.contains('\r'), "{stub:?}");
            assert!(template.is_ascii(), "{stub:?}");
            assert_eq!(template.matches(STUB_BASE).count(), 1, "{stub:?}");

            let rendered = stub.render(&base);
            assert!(rendered.contains(stamped), "{stub:?}:\n{rendered}");
            assert!(!rendered.contains(STUB_BASE), "{stub:?}");
        }
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
                matches!(error, Error::Init(InitError::AlreadyInitialized { .. })),
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
            matches!(error, Error::Init(InitError::HomeDirectory { .. })),
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
        let dir = temp();
        let absent = dir.path().join("absent");
        assert!(!is_os_home(dir.path(), || Ok(absent)));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_home_still_matches_the_current_directory() {
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
        std::os::unix::fs::symlink(dir.path().join("nowhere"), dir.path().join("bin"))
            .expect("fixture");

        assert_eq!(
            occupant(&dir.path().join("files"), SkeletonKind::Directory).expect("inspect"),
            Occupant::Matching
        );
        assert_eq!(
            occupant(&dir.path().join("bin"), SkeletonKind::Directory).expect("inspect"),
            Occupant::WrongKind
        );
    }

    #[test]
    fn the_report_marks_directories_and_lists_only_new_paths() {
        let created = [
            SkeletonEntry::File {
                name: Manifest::FILE_NAME,
                content: "",
            },
            SkeletonEntry::Directory { name: "bin" },
        ];
        assert_eq!(created_line(&created), "created batfiles.toml, bin/");
    }

    #[test]
    fn an_exclusion_list_covers_remotes_however_it_is_spelled() {
        assert!(ignores_remotes("/remotes/\n"));
        assert!(ignores_remotes("*.swp\nremotes/\n"));
        assert!(ignores_remotes("dotfiles/remotes/**\n"));
        // Slashless ignore patterns also cover the remotes directory.
        assert!(ignores_remotes("/remotes\n"));
        assert!(ignores_remotes("remotes\n"));

        assert!(!ignores_remotes(""));
        assert!(!ignores_remotes("*.swp\ntarget\n"));
    }

    #[test]
    fn the_starter_exclusion_list_still_names_the_tree_it_excludes() {
        assert_eq!(GITIGNORE_CONTENT, format!("/{}/\n", remotes::DIRECTORY));
        assert!(ignores_remotes(GITIGNORE_CONTENT));
    }

    #[test]
    fn the_starter_manifest_declares_nothing_at_all() {
        let manifest: Manifest =
            toml::from_str(BATFILES_TOML).expect("the starter manifest should parse");
        assert!(manifest.actions.is_empty());
        assert!(manifest.remotes.is_empty());
        assert!(manifest.vars.is_empty());
    }

    #[test]
    fn every_git_failure_points_at_the_flag_that_avoids_git() {
        let unavailable = InitError::from(git::GitError::Unavailable {
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        });
        let message = unavailable.to_string();
        assert!(message.contains("--no-git-init"), "{message}");
        assert!(message.contains("could not run git"), "{message}");

        let failed = InitError::from(git::GitError::Failed {
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
