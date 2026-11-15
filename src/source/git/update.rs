//! Follow declared refs and fast-forward clean checkouts when local content permits.

use std::fs;
use std::path::{Path, PathBuf};

use super::GitError;
use super::process::{as_path, launch, query, run_git, stderr_summary};
use crate::error::Error;
use crate::filesystem::mode::RunMode;
use crate::output::{Reporter, Verb};

/// Leave a validated clone's dirty worktree unchanged with a warning. Follow
/// the declared ref, or update the current branch from its upstream.
pub(super) fn update_clone(
    dest: &Path,
    git_ref: Option<&str>,
    reporter: &Reporter,
) -> Result<(), Error> {
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
    run_git(Some(dest), "fetch", &["fetch", "--all"], dest)?;

    report_update(
        dest,
        follow_ref(dest, git_ref, reporter)?,
        git_ref,
        reporter,
    );
    Ok(())
}

/// Report a completed update or checkout at normal verbosity; unchanged clones at `-v`.
fn report_update(dest: &Path, outcome: UpdateOutcome, target: &str, reporter: &Reporter) {
    match outcome {
        UpdateOutcome::Unchanged => reporter.detail(1, &format!("unchanged {}", dest.display())),
        UpdateOutcome::Advanced => reporter.info(&format!(
            "{} {}",
            Verb::Update.for_mode(RunMode::Perform),
            dest.display()
        )),
        UpdateOutcome::Switched => reporter.info(&format!(
            "{} {} to {target}",
            Verb::SwitchRef.for_mode(RunMode::Perform),
            dest.display()
        )),
        UpdateOutcome::Skipped => {}
    }
}

/// What following a declared `ref` came to.
pub(super) enum UpdateOutcome {
    /// The checkout was already at what the ref resolved to.
    Unchanged,
    /// The branch the checkout was already on moved forward.
    Advanced,
    /// The checkout itself changed: another branch, or a detached object.
    Switched,
    /// Left alone, with a warning already reported.
    Skipped,
}

/// Resolve `git_ref` and check out its branch or commit.
pub(super) fn follow_ref(
    dest: &Path,
    git_ref: &str,
    reporter: &Reporter,
) -> Result<UpdateOutcome, Error> {
    match classify_ref(dest, git_ref)? {
        RefTarget::Branch { remote_ref } => follow_branch(dest, git_ref, &remote_ref, reporter),
        RefTarget::Object { commit } => detach_at(dest, &commit),
    }
}

/// What a declared `ref` turned out to name.
enum RefTarget {
    /// A branch on a remote, held here as its full `refs/remotes/…` name. A
    /// local branch of the declared name follows it.
    Branch { remote_ref: String },
    /// Anything else that resolves — a tag, a commit, a full ref name — held as
    /// the commit it resolved to.
    Object { commit: String },
}

/// Resolve a declared ref to a remote branch or commit. Prefer remote branches
/// with `origin` first; otherwise resolve the input as a commit expression.
fn classify_ref(dest: &Path, git_ref: &str) -> Result<RefTarget, Error> {
    if names_a_branch(dest, git_ref)? {
        for remote in remote_names(dest)? {
            let candidate = format!("refs/remotes/{remote}/{git_ref}");
            if has_ref(dest, &candidate)? {
                return Ok(RefTarget::Branch {
                    remote_ref: candidate,
                });
            }
        }
    }
    match commit_named_by(dest, git_ref)? {
        Some(commit) => Ok(RefTarget::Object { commit }),
        None => Err(GitError::RefUnresolvable {
            path: dest.to_path_buf(),
            git_ref: git_ref.to_owned(),
        }
        .into()),
    }
}

/// The clone's remotes, `origin` first where it has one.
fn remote_names(dest: &Path) -> Result<Vec<String>, Error> {
    let listed = run_git(Some(dest), "remote", &["remote"], dest)?;
    let mut names: Vec<String> = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect();
    // Stable sorting keeps other remotes in Git's order.
    names.sort_by_key(|name| name != "origin");
    Ok(names)
}

/// Switch to the declared branch if needed, then attempt a fast-forward.
/// If checkout succeeds but advancement is skipped, return Switched.
fn follow_branch(
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
            run_git(
                Some(dest),
                "checkout",
                &["checkout", KEEP_IGNORED, git_ref, "--"],
                dest,
            )?;
        } else {
            run_git(
                Some(dest),
                "checkout",
                &["checkout", KEEP_IGNORED, "-b", git_ref, "--track", remote],
                dest,
            )?;
        }
    }

    let advanced = fast_forward(dest, remote, reporter)?;
    // A branch switch takes precedence over any subsequent fast-forward in the report.
    Ok(if switching {
        UpdateOutcome::Switched
    } else {
        advanced
    })
}

/// Fast-forward to the target ref. Equal commits are unchanged; local commits
/// or files that would be overwritten produce a warning and a skipped outcome.
fn fast_forward(dest: &Path, remote: &str, reporter: &Reporter) -> Result<UpdateOutcome, Error> {
    if commit_id_of(dest, "HEAD")? == commit_id_of(dest, remote)? {
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
        warn_skipped_update(
            reporter,
            dest,
            &obstruction_message(&path, short_remote_ref(remote)),
        );
        return Ok(UpdateOutcome::Skipped);
    }
    run_git(Some(dest), "merge", &["merge", "--ff-only", remote], dest)?;
    Ok(UpdateOutcome::Advanced)
}

/// Return the first occupied path that a fast-forward would create, if any.
fn overwritten_by(dest: &Path, target: &str) -> Result<Option<PathBuf>, Error> {
    let listed = run_git(
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
fn obstruction_message(path: &Path, publisher: &str) -> String {
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

/// Check out `commit_id` with a detached HEAD.
fn detach_at(dest: &Path, commit_id: &str) -> Result<UpdateOutcome, Error> {
    if head_branch(dest)?.is_none() && commit_id_of(dest, "HEAD")? == commit_id {
        return Ok(UpdateOutcome::Unchanged);
    }
    run_git(
        Some(dest),
        "checkout",
        &["checkout", KEEP_IGNORED, "--detach", commit_id, "--"],
        dest,
    )?;
    Ok(UpdateOutcome::Switched)
}

/// Checkout option that preserves ignored files.
const KEEP_IGNORED: &str = "--no-overwrite-ignore";

/// The commit `git_ref` names, or `None` where it names nothing. Annotated tags are peeled
/// so the result can be compared with `HEAD` and checked out. Other Git failures propagate.
/// Use `has_ref` for exact ref-name lookups.
fn commit_named_by(dest: &Path, git_ref: &str) -> Result<Option<String>, Error> {
    query(
        dest,
        "rev-parse",
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{git_ref}^{{commit}}"),
        ],
    )
}

/// Whether a ref exists under exactly that name.
fn has_ref(dest: &Path, name: &str) -> Result<bool, Error> {
    Ok(query(dest, "show-ref", &["show-ref", "--verify", "--quiet", name])?.is_some())
}

/// Whether a declared `ref` is a name a local branch could have.
fn names_a_branch(dest: &Path, git_ref: &str) -> Result<bool, Error> {
    let output = launch(Some(dest), &["check-ref-format", "--branch", git_ref])?;
    Ok(output.status.success())
}

/// The branch `HEAD` is on, or `None` where it is detached.
fn head_branch(dest: &Path) -> Result<Option<String>, Error> {
    // Exit 1 is a detached HEAD, which is not on a branch. Anything else
    // non-zero is trouble reading the repository.
    query(
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
    run_git(Some(dest), "fetch", &["fetch"], dest)?;
    report_update(
        dest,
        fast_forward(dest, &upstream, reporter)?,
        &upstream,
        reporter,
    );
    Ok(())
}

/// Warn that the clone was not updated, including the reason.
fn warn_skipped_update(reporter: &Reporter, dest: &Path, because: &str) {
    reporter.warn(&format!("not updating {}: {because}", dest.display()));
}

/// Check staged, unstaged, and untracked content, regardless of Git display
/// preferences. Ignored content does not make a worktree dirty.
fn is_dirty(dest: &Path) -> Result<bool, Error> {
    let status = run_git(
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
    let Some(branch) = head_branch(dest)? else {
        return Ok(None);
    };

    // Exit 1 is the key not being there: a branch nobody set an upstream on.
    if query(
        dest,
        "config",
        &["config", "--get", &format!("branch.{branch}.merge")],
    )?
    .is_none()
    {
        return Ok(None);
    }

    let found = run_git(
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
fn commit_id_of(dest: &Path, revision: &str) -> Result<String, Error> {
    let found = run_git(Some(dest), "rev-parse", &["rev-parse", revision], dest)?;
    Ok(String::from_utf8_lossy(&found.stdout).trim().to_owned())
}

/// Return whether `earlier` is an ancestor of `later`.
fn ancestor(dest: &Path, earlier: &str, later: &str) -> Result<bool, Error> {
    let output = launch(Some(dest), &["merge-base", "--is-ancestor", earlier, later])?;
    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(GitError::Failed {
            command: "merge-base",
            path: dest.to_path_buf(),
            message: stderr_summary(&output),
        }
        .into()),
    }
}
