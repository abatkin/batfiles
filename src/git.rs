//! Clone repositories and update existing clones conservatively.
//! Dry runs report intent without running Git. Git configuration and credentials
//! are inherited; repository redirects are cleared.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use thiserror::Error as ThisError;

use crate::directory;
use crate::error::Error;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths::{self, ExistingNode, Occupancy, RepositoryRoot};

/// What can go wrong reaching a clone, as one enum a caller can match on.
#[derive(Debug, ThisError)]
pub(crate) enum Failure {
    /// `git` could not be run at all, most often because it is not on `PATH`.
    #[error(
        "could not run git: {source}. batfiles runs the `git` on your PATH so that your \
         gitconfig, credential helpers, and SSH agent apply"
    )]
    Unavailable { source: io::Error },

    /// A `git` command ran and failed, carrying git's own diagnostic.
    #[error("git {command} failed in {}: {message}", .path.display())]
    Failed {
        command: &'static str,
        path: PathBuf,
        message: String,
    },

    /// A clone destination holding a directory with no `.git` in it.
    #[error(
        "cannot update {}: it is a directory, and not a git clone; \
         move it aside and run sync again",
        .path.display()
    )]
    NotAClone { path: PathBuf },

    /// A destination whose git directory is not its own: a `.git` that is a symlink or
    /// a file rather than the directory `git clone` makes, or a real one whose
    /// configured worktree is somewhere else.
    #[error(
        "cannot update {}: its .git belongs to a checkout somewhere else, so updating it \
         would change that one; move it aside and run sync again",
        .path.display()
    )]
    CloneElsewhere { path: PathBuf },

    /// A destination holding a `.git` whose `HEAD` names no commit: what an interrupted
    /// clone leaves, and what a damaged one looks like.
    #[error(
        "cannot update {}: it has a .git but nothing checked out, so it is an incomplete \
         or damaged clone ({message}); move it aside and run sync again",
        .path.display()
    )]
    CloneIncomplete { path: PathBuf, message: String },

    /// A declared `ref` that names nothing in the clone: a branch nobody publishes, a
    /// tag spelled wrong, a commit that was rebased away.
    #[error(
        "cannot follow `{git_ref}` in {}: no branch, tag, or commit of that name is in the clone",
        .path.display()
    )]
    RefUnresolvable { path: PathBuf, git_ref: String },
}

/// Clone into a vacant or replaceable destination; update an existing clone.
/// Dry runs inspect occupancy and report intent without invoking Git.
/// Existing clones retain their configured remotes, regardless of `url`.
pub(crate) fn clone_or_update(
    url: &str,
    dest: &Path,
    git_ref: Option<&str>,
    repository: &RepositoryRoot,
    mode: RunMode,
    reporter: &Reporter,
) -> Result<(), Error> {
    match Occupancy::at(dest, repository)? {
        // The only thing that can be a clone. Whether it *is* one takes `git`,
        // so a dry run does not find out.
        Occupancy::Unmanaged(ExistingNode::Directory) => {
            if mode.writes() {
                return update(dest, git_ref, reporter);
            }
            reporter.info(&format!("{} {}", Verb::Update.say(mode), dest.display()));
            Ok(())
        }
        Occupancy::Unmanaged(found) => Err(Error::DestinationExists {
            path: dest.to_path_buf(),
            found,
        }),
        Occupancy::Replaceable { written, .. } => {
            if mode.writes() {
                remove(dest)?;
            }
            reporter.info(&format!(
                "{} the symlink at {} to {} to make room for the clone",
                Verb::Remove.say(mode),
                dest.display(),
                written.display()
            ));
            clone(url, dest, git_ref, mode, reporter)
        }
        Occupancy::Vacant => clone(url, dest, git_ref, mode, reporter),
    }
}

/// Carry out an action's clone into a destination nothing is at: the parents it
/// needs, the clone itself, the declared ref, and the line reporting all of it.
fn clone(
    url: &str,
    dest: &Path,
    git_ref: Option<&str>,
    mode: RunMode,
    reporter: &Reporter,
) -> Result<(), Error> {
    directory::create_parents(dest, mode)?.report_removals(mode, reporter);
    if mode.writes() {
        clone_repository(url, dest)?;
        if let Some(git_ref) = git_ref {
            // The clone line below already reports where the repository is.
            follow(dest, git_ref, reporter)?;
        }
    }
    reporter.info(&format!(
        "{} {} from {url}{}",
        Verb::Clone.say(mode),
        dest.display(),
        at(git_ref)
    ));
    Ok(())
}

/// Clone into a vacant destination without reporting it; the caller reports.
/// Git creates missing parent directories. Takes no [`RunMode`]: the `clone`
/// command calls this directly.
///
/// The `--` stops a URL beginning with a dash from being read as an option.
pub(crate) fn clone_repository(url: &str, dest: &Path) -> Result<(), Error> {
    run(
        None,
        "clone",
        &[
            OsStr::new("clone"),
            OsStr::new("--"),
            OsStr::new(url),
            dest.as_os_str(),
        ],
        dest,
    )?;
    Ok(())
}

/// ` at <ref>`, or nothing where none was declared.
fn at(git_ref: Option<&str>) -> String {
    git_ref.map(|it| format!(" at {it}")).unwrap_or_default()
}

/// Validate the clone and leave dirty worktrees unchanged with a warning.
/// Follow the declared ref, or update the current branch from its upstream.
fn update(dest: &Path, git_ref: Option<&str>, reporter: &Reporter) -> Result<(), Error> {
    validate_clone(dest)?;

    if is_dirty(dest)? {
        warn_skipped_update(reporter, dest, "it has uncommitted changes");
        return Ok(());
    }
    match git_ref {
        Some(git_ref) => update_to_ref(dest, git_ref, reporter),
        None => update_tracking(dest, reporter),
    }
}

/// Bring a clone to the `ref` its record declares, and say what that took.
fn update_to_ref(dest: &Path, git_ref: &str, reporter: &Reporter) -> Result<(), Error> {
    run(Some(dest), "fetch", &["fetch", "--all"], dest)?;

    report_update(dest, follow(dest, git_ref, reporter)?, git_ref, reporter);
    Ok(())
}

/// Report a completed update or checkout at normal verbosity; unchanged clones at `-v`.
fn report_update(dest: &Path, outcome: UpdateOutcome, target: &str, reporter: &Reporter) {
    match outcome {
        UpdateOutcome::Unchanged => reporter.detail(1, &format!("unchanged {}", dest.display())),
        UpdateOutcome::Advanced => reporter.info(&format!(
            "{} {}",
            Verb::Update.say(RunMode::Perform),
            dest.display()
        )),
        UpdateOutcome::Switched => reporter.info(&format!(
            "{} {} to {target}",
            Verb::SwitchRef.say(RunMode::Perform),
            dest.display()
        )),
        UpdateOutcome::Skipped => {}
    }
}

/// What following a declared `ref` came to.
enum UpdateOutcome {
    /// The checkout was already at what the ref resolved to.
    Unchanged,
    /// The branch the checkout was already on moved forward.
    Advanced,
    /// The checkout itself changed: another branch, or a detached object.
    Switched,
    /// Left alone, with a warning already reported.
    Skipped,
}

/// Put the worktree on whatever `git_ref` names, having resolved it once.
fn follow(dest: &Path, git_ref: &str, reporter: &Reporter) -> Result<UpdateOutcome, Error> {
    match resolve(dest, git_ref)? {
        Target::Branch { remote_ref } => branch(dest, git_ref, &remote_ref, reporter),
        Target::Object { commit } => detach(dest, &commit),
    }
}

/// What a declared `ref` turned out to name.
enum Target {
    /// A branch on a remote, held here as its full `refs/remotes/…` name. A
    /// local branch of the declared name follows it.
    Branch { remote_ref: String },
    /// Anything else that resolves — a tag, a commit, a full ref name — held as
    /// the commit it resolved to.
    Object { commit: String },
}

/// Resolve a declared ref to a remote branch or commit. Prefer remote branches
/// with `origin` first; otherwise resolve the input as a commit expression.
fn resolve(dest: &Path, git_ref: &str) -> Result<Target, Error> {
    if names_a_branch(dest, git_ref)? {
        for remote in remotes(dest)? {
            let candidate = format!("refs/remotes/{remote}/{git_ref}");
            if has_ref(dest, &candidate)? {
                return Ok(Target::Branch {
                    remote_ref: candidate,
                });
            }
        }
    }
    // `^{commit}` peels an annotated tag, so what comes back is comparable with
    // `HEAD` and is something a detached checkout can sit on.
    match verify(dest, &format!("{git_ref}^{{commit}}"))? {
        Some(commit) => Ok(Target::Object { commit }),
        None => Err(Failure::RefUnresolvable {
            path: dest.to_path_buf(),
            git_ref: git_ref.to_owned(),
        }
        .into()),
    }
}

/// The clone's remotes, `origin` first where it has one.
fn remotes(dest: &Path) -> Result<Vec<String>, Error> {
    let listed = run(Some(dest), "remote", &["remote"], dest)?;
    let mut names: Vec<String> = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    // A stable sort on one bit, so `origin` comes first and the rest keep the
    // order git listed them in.
    names.sort_by_key(|name| name != "origin");
    Ok(names)
}

/// Check out the declared branch and attempt a conservative fast-forward.
/// If checkout succeeds but advancement is skipped, return Switched.
fn branch(
    dest: &Path,
    git_ref: &str,
    remote: &str,
    reporter: &Reporter,
) -> Result<UpdateOutcome, Error> {
    let switching = head_branch(dest)?.as_deref() != Some(git_ref);
    if switching {
        if has_ref(dest, &format!("refs/heads/{git_ref}"))? {
            // Spelled with a trailing `--` so a file of the same name in the
            // worktree cannot be read as a path to check out instead.
            run(
                Some(dest),
                "checkout",
                &["checkout", KEEP_IGNORED, git_ref, "--"],
                dest,
            )?;
        } else {
            run(
                Some(dest),
                "checkout",
                &["checkout", KEEP_IGNORED, "-b", git_ref, "--track", remote],
                dest,
            )?;
        }
    }

    let advanced = advance(dest, remote, reporter)?;
    // A switch is reported; any following fast-forward is implied.
    Ok(if switching {
        UpdateOutcome::Switched
    } else {
        advanced
    })
}

/// Fast-forward to the target ref. Equal commits are unchanged; local commits
/// or files that would be overwritten produce a warning and a skipped outcome.
fn advance(dest: &Path, remote: &str, reporter: &Reporter) -> Result<UpdateOutcome, Error> {
    if commit(dest, "HEAD")? == commit(dest, remote)? {
        return Ok(UpdateOutcome::Unchanged);
    }
    if !ancestor(dest, "HEAD", remote)? {
        warn_skipped_update(
            reporter,
            dest,
            &format!("it has commits that {} does not", short_remote_ref(remote)),
        );
        return Ok(UpdateOutcome::Skipped);
    }
    if let Some(path) = overwritten_by(dest, remote)? {
        warn_skipped_update(reporter, dest, &in_the_way(&path, short_remote_ref(remote)));
        return Ok(UpdateOutcome::Skipped);
    }
    run(Some(dest), "merge", &["merge", "--ff-only", remote], dest)?;
    Ok(UpdateOutcome::Advanced)
}

/// The first path a fast-forward would create that something is already at, if
/// there is one.
fn overwritten_by(dest: &Path, target: &str) -> Result<Option<PathBuf>, Error> {
    let listed = run(
        Some(dest),
        "diff",
        &[
            "diff",
            "--no-renames",
            "--name-only",
            "--diff-filter=A",
            "-z",
            "HEAD",
            target,
        ],
        dest,
    )?;
    for name in listed
        .stdout
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let Some(relative) = as_path(name) else {
            continue;
        };
        if fs::symlink_metadata(dest.join(&relative)).is_ok() {
            return Ok(Some(relative));
        }
    }
    Ok(None)
}

/// Why a clone holding its own copy of a file is left alone.
fn in_the_way(path: &Path, publisher: &str) -> String {
    format!(
        "it has a file of its own at {}, which {publisher} now tracks",
        path.display()
    )
}

/// A remote-tracking ref as a reader writes it: `origin/main`, not
/// `refs/remotes/origin/main`.
fn short_remote_ref(remote: &str) -> &str {
    remote.strip_prefix("refs/remotes/").unwrap_or(remote)
}

/// Sit the worktree on one object, detached.
fn detach(dest: &Path, commit_id: &str) -> Result<UpdateOutcome, Error> {
    if head_branch(dest)?.is_none() && commit(dest, "HEAD")? == commit_id {
        return Ok(UpdateOutcome::Unchanged);
    }
    run(
        Some(dest),
        "checkout",
        &["checkout", KEEP_IGNORED, "--detach", commit_id, "--"],
        dest,
    )?;
    Ok(UpdateOutcome::Switched)
}

/// What stops a checkout replacing a file the user keeps and git ignores.
const KEEP_IGNORED: &str = "--no-overwrite-ignore";

/// Resolve a revision expression. Returns None for an unresolved expression;
/// other Git failures propagate. Use `has_ref` for exact ref-name lookups.
fn verify(dest: &Path, revision: &str) -> Result<Option<String>, Error> {
    ask(
        dest,
        "rev-parse",
        &["rev-parse", "--verify", "--quiet", revision],
    )
}

/// Whether a ref exists under exactly that name.
fn has_ref(dest: &Path, name: &str) -> Result<bool, Error> {
    Ok(ask(dest, "show-ref", &["show-ref", "--verify", "--quiet", name])?.is_some())
}

/// Whether a declared `ref` is a name a local branch could have.
fn names_a_branch(dest: &Path, git_ref: &str) -> Result<bool, Error> {
    let output = git(Some(dest), &["check-ref-format", "--branch", git_ref])?;
    Ok(output.status.success())
}

/// The branch `HEAD` is on, or `None` where it is detached.
fn head_branch(dest: &Path) -> Result<Option<String>, Error> {
    // Exit 1 is a detached HEAD, which is not on a branch. Anything else
    // non-zero is trouble reading the repository.
    ask(
        dest,
        "symbolic-ref",
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
    )
}

/// Bring an existing clone up to date on the branch it is already on, or say
/// why it was left alone.
fn update_tracking(dest: &Path, reporter: &Reporter) -> Result<(), Error> {
    let Some(upstream) = upstream(dest)? else {
        warn_skipped_update(reporter, dest, "it is not on a branch that tracks a remote");
        return Ok(());
    };
    run(Some(dest), "fetch", &["fetch"], dest)?;
    report_update(
        dest,
        advance(dest, &upstream, reporter)?,
        &upstream,
        reporter,
    );
    Ok(())
}

/// Leave a clone as it is, and say why at a volume that is not hidden by
/// default.
fn warn_skipped_update(reporter: &Reporter, dest: &Path, because: &str) {
    reporter.warn(&format!("not updating {}: {because}", dest.display()));
}

/// Require a real `.git` directory, a worktree rooted at `dest`, and a resolvable
/// HEAD. Refuse indirect, misplaced, incomplete, or damaged clones.
fn validate_clone(dest: &Path) -> Result<(), Error> {
    match own_git_directory(dest)? {
        GitDirectory::Missing => {
            return Err(Failure::NotAClone {
                path: dest.to_path_buf(),
            }
            .into());
        }
        GitDirectory::Indirect => {
            return Err(Failure::CloneElsewhere {
                path: dest.to_path_buf(),
            }
            .into());
        }
        GitDirectory::Own => {}
    }

    let toplevel = run(
        Some(dest),
        "rev-parse",
        &["rev-parse", "--show-toplevel"],
        dest,
    )?;
    match line_as_path(&toplevel.stdout) {
        Some(root)
            if paths::canonicalize_or_normalize(&root)
                == paths::canonicalize_or_normalize(dest) => {}
        _ => {
            return Err(Failure::CloneElsewhere {
                path: dest.to_path_buf(),
            }
            .into());
        }
    }

    let output = git(Some(dest), &["rev-parse", "--verify", "HEAD"])?;
    if !output.status.success() {
        return Err(Failure::CloneIncomplete {
            path: dest.to_path_buf(),
            message: complaint(&output),
        }
        .into());
    }
    Ok(())
}

/// What sits at `<dest>/.git`, judged without following it.
enum GitDirectory {
    /// Nothing: a directory that is not a clone at all.
    Missing,
    /// A real directory, which is what `git clone` makes.
    Own,
    /// A symlink or a file standing in for one, either of which can put git's
    /// refs somewhere batfiles did not install.
    Indirect,
}

fn own_git_directory(dest: &Path) -> Result<GitDirectory, Error> {
    let path = dest.join(".git");
    match fs::symlink_metadata(&path) {
        // `symlink_metadata` does not follow, so `is_dir` is false for a
        // symlink however it resolves. That is the whole check.
        Ok(found) if found.is_dir() => Ok(GitDirectory::Own),
        Ok(_) => Ok(GitDirectory::Indirect),
        Err(error) if paths::reaches_nothing(&error) => Ok(GitDirectory::Missing),
        Err(source) => Err(Error::Read { path, source }),
    }
}

/// Decode a Git path, removing one line terminator. Preserve trailing spaces
/// and, on Unix, non-UTF-8 bytes.
fn line_as_path(printed: &[u8]) -> Option<PathBuf> {
    let line = printed.strip_suffix(b"\n").unwrap_or(printed);
    as_path(line.strip_suffix(b"\r").unwrap_or(line))
}

/// One path git printed, as a path.
fn as_path(printed: &[u8]) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(PathBuf::from(OsStr::from_bytes(printed)))
    }
    // Where a path is not bytes, git prints UTF-8; anything else is not a path
    // this can compare, and a comparison it cannot make must not pass.
    #[cfg(not(unix))]
    {
        std::str::from_utf8(printed).ok().map(PathBuf::from)
    }
}

/// Check staged, unstaged, and untracked content, regardless of Git display
/// preferences. Ignored content does not make a worktree dirty.
fn is_dirty(dest: &Path) -> Result<bool, Error> {
    let status = run(
        Some(dest),
        "status",
        &["status", "--porcelain", "--untracked-files=normal"],
        dest,
    )?;
    Ok(!status.stdout.iter().all(u8::is_ascii_whitespace))
}

/// Return the checked-out branch's upstream. Detached HEAD or absent tracking
/// configuration returns None; malformed configuration and Git failures propagate.
fn upstream(dest: &Path) -> Result<Option<String>, Error> {
    // A detached HEAD is not on a branch and so follows nothing.
    let Some(branch) = head_branch(dest)? else {
        return Ok(None);
    };

    // Exit 1 is the key not being there: a branch nobody set an upstream on.
    if ask(
        dest,
        "config",
        &["config", "--get", &format!("branch.{branch}.merge")],
    )?
    .is_none()
    {
        return Ok(None);
    }

    let found = run(
        Some(dest),
        "rev-parse",
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        dest,
    )?;
    Ok(Some(
        String::from_utf8_lossy(&found.stdout).trim().to_owned(),
    ))
}

/// The commit a revision names.
fn commit(dest: &Path, revision: &str) -> Result<String, Error> {
    let found = run(Some(dest), "rev-parse", &["rev-parse", revision], dest)?;
    Ok(String::from_utf8_lossy(&found.stdout).trim().to_owned())
}

/// Whether `earlier` is reachable from `later`, which is what makes an update a
/// fast-forward.
fn ancestor(dest: &Path, earlier: &str, later: &str) -> Result<bool, Error> {
    let output = git(Some(dest), &["merge-base", "--is-ancestor", earlier, later])?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(Failure::Failed {
            command: "merge-base",
            path: dest.to_path_buf(),
            message: complaint(&output),
        }
        .into()),
    }
}

/// Run a `git` command that has to succeed, and turn a failure into one error
/// carrying git's own diagnostic.
fn run<S: AsRef<OsStr>>(
    dir: Option<&Path>,
    command: &'static str,
    args: &[S],
    dest: &Path,
) -> Result<Output, Error> {
    let output = git(dir, args)?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(Failure::Failed {
            command,
            path: dest.to_path_buf(),
            message: complaint(&output),
        }
        .into())
    }
}

/// Run a Git query: exit 0 returns trimmed stdout, exit 1 returns None, and
/// other statuses fail with Git diagnostics. Only use for queries with this convention.
fn ask(dest: &Path, command: &'static str, args: &[&str]) -> Result<Option<String>, Error> {
    let output = git(Some(dest), args)?;
    match output.status.code() {
        Some(0) => Ok(Some(
            String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        )),
        Some(1) => Ok(None),
        _ => Err(Failure::Failed {
            command,
            path: dest.to_path_buf(),
            message: complaint(&output),
        }
        .into()),
    }
}

/// Remove a symlink [`Occupancy`] classified as holding no content of its own.
fn remove(dest: &Path) -> Result<(), Error> {
    fs::remove_file(dest).map_err(|source| Error::Write {
        path: dest.to_path_buf(),
        source,
    })
}

/// Create a repository in `dir` unless one already covers it, answering whether
/// one was created.
///
/// For `init`, which has no dry run, so it takes no [`RunMode`].
pub(crate) fn init_repository(dir: &Path) -> Result<bool, Failure> {
    if inside_work_tree(dir)? {
        return Ok(false);
    }

    let output = launch(Some(dir), &["init"])?;
    if output.status.success() {
        Ok(true)
    } else {
        Err(Failure::Failed {
            command: "init",
            path: dir.to_path_buf(),
            message: complaint(&output),
        })
    }
}

/// Whether `dir` already sits inside a work tree, a parent repository included.
///
/// Checks the output too: inside a bare repository's `.git`, `rev-parse`
/// succeeds and prints `false`. Neither stream is shown; outside a repository
/// its `fatal:` line is the answer, not an error.
fn inside_work_tree(dir: &Path) -> Result<bool, Failure> {
    let output = launch(Some(dir), &["rev-parse", "--is-inside-work-tree"])?;
    Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true")
}

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

/// Run `git`, capturing both of its streams.
fn git<S: AsRef<OsStr>>(dir: Option<&Path>, args: &[S]) -> Result<Output, Error> {
    launch(dir, args).map_err(Into::into)
}

/// [`git`], returning the subsystem failure for callers that report a launch
/// failure themselves.
fn launch<S: AsRef<OsStr>>(dir: Option<&Path>, args: &[S]) -> Result<Output, Failure> {
    let mut command = Command::new("git");
    command.args(args);
    for redirect in REDIRECTS {
        command.env_remove(redirect);
    }
    if let Some(dir) = dir {
        command.current_dir(dir);
    }
    command
        .output()
        .map_err(|source| Failure::Unavailable { source })
}

/// What git said about a failure, or the status it exited with when it said
/// nothing.
fn complaint(output: &Output) -> String {
    let said = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if said.is_empty() {
        format!("git exited with {}", output.status)
    } else {
        said
    }
}
