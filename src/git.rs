//! Shelling out to `git`, and the one rule that decides whether it runs at all.
//!
//! Never a git library (`guidance.md`, rule 6): the user's `~/.gitconfig`,
//! credential helpers, and SSH agent have to apply, and they apply to the `git`
//! on their `PATH` and to nothing else. Both streams are captured, so batfiles'
//! own report is the only thing on the terminal and a failure can quote what git
//! said; a passphrase or credential prompt still reaches the user, because those
//! read and write `/dev/tty` rather than the streams a parent hands down.
//!
//! **Under [`RunMode::DryRun`] this module runs no git, for any caller.** Not a
//! read either: a dry run neither contacts the network nor touches a checkout it
//! was asked only to describe. That is one rule with no exception argument and
//! no write scope, and 6.2 must not add one — a dry run does not materialize
//! remotes either, and that limit is deliberate (`guidance.md`, "Where the mode
//! is read"). It is also not inferred from the destination: a containment test
//! against the home would be wrong in both directions, and with one rule for
//! every caller there is nothing for such a test to decide.
//!
//! What a dry run gives up by that rule is one distinction, and only one. What
//! is at a destination is [`Occupancy`]'s answer, which is a read and is
//! therefore the same in both modes: a file or a foreign symlink is refused in
//! a dry run exactly as in a real one. Telling a *clone* from a plain directory
//! is the part that takes `git`, so a dry run says it would update either, and
//! the real run is where the second is refused. That is the "intent, not
//! success" boundary (`guidance.md`, "What a dry run says"), not a partial plan.

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
use crate::paths::{self, ExistingNode, Occupancy, Repository};

/// What can go wrong reaching a clone, as one enum a caller can match on.
///
/// Nested under [`Error::Git`] rather than spread flat through `error.rs`,
/// because `git-clone-list` is the first caller that has to tell these apart:
/// one repository it cannot have costs that entry, and a `git` it cannot run
/// costs the run. Which of these is which is that action's to say, so nothing
/// here classifies itself.
///
/// **None of the conservative update rules is in here.** A dirty worktree, a
/// branch tracking nothing, and a history that has diverged are all `Ok` plus a
/// warning, so a caller never sees one. What this enum holds is the failures.
#[derive(Debug, ThisError)]
pub(crate) enum Failure {
    /// `git` could not be run at all, most often because it is not on `PATH`.
    #[error(
        "could not run git: {source}. batfiles runs the `git` on your PATH so that your \
         gitconfig, credential helpers, and SSH agent apply"
    )]
    Unavailable { source: io::Error },

    /// A `git` command ran and failed, carrying git's own diagnostic. The
    /// message is built by [`complaint`] rather than at each call site, so it
    /// stays a fact about a subprocess.
    #[error("git {command} failed in {}: {message}", .path.display())]
    Failed {
        command: &'static str,
        path: PathBuf,
        message: String,
    },

    /// A clone destination holding a directory with no `.git` in it. Named
    /// apart from [`Error::DestinationExists`] because the remedy is the same
    /// but the reason is not one the node's kind gives away: it is a directory,
    /// and the refusal is about what is missing inside it.
    #[error(
        "cannot update {}: it is a directory, and not a git clone; \
         move it aside and run sync again",
        .path.display()
    )]
    NotAClone { path: PathBuf },

    /// A destination whose git directory is not its own: a `.git` that is a
    /// symlink or a file rather than the directory `git clone` makes, or a real
    /// one whose configured worktree is somewhere else. Refused because running
    /// git here reaches a checkout batfiles never installed — a fetch and a
    /// fast-forward would move *that* one's branch.
    #[error(
        "cannot update {}: its .git belongs to a checkout somewhere else, so updating it \
         would change that one; move it aside and run sync again",
        .path.display()
    )]
    CloneElsewhere { path: PathBuf },

    /// A destination holding a `.git` whose `HEAD` names no commit: what an
    /// interrupted clone leaves, and what a damaged one looks like. This is the
    /// arm that stops a later run mistaking either for finished work, so it
    /// carries git's own account of which it was.
    #[error(
        "cannot update {}: it has a .git but nothing checked out, so it is an incomplete \
         or damaged clone ({message}); move it aside and run sync again",
        .path.display()
    )]
    CloneIncomplete { path: PathBuf, message: String },

    /// A declared `ref` that names nothing in the clone: a branch nobody
    /// publishes, a tag spelled wrong, a commit that was rebased away. A
    /// repository bug rather than a state of the checkout, which is why it is a
    /// failure where the update rules are warnings.
    #[error(
        "cannot follow `{git_ref}` in {}: no branch, tag, or commit of that name is in the clone",
        .path.display()
    )]
    RefUnresolvable { path: PathBuf, git_ref: String },
}

/// Put a repository at `dest`, whether or not one is there already.
///
/// The clone-or-update decision, which is the branch this module reads
/// [`RunMode`] in. Every caller that wants a worktree on disk comes through
/// here: `git-clone` and `git-clone-list` today, and remote materialization at
/// 6.2.
///
/// Cloning writes straight into `dest` rather than into a staging sibling, which
/// is the one place a rule-15 install is not built beside its destination. What
/// the rule is actually about is a later run mistaking wreckage for finished
/// work, and the classification below is what forecloses that: a destination is
/// absent, is a clone with a checkout, or is refused by name. An interrupted
/// clone lands in the third case rather than the second.
pub(crate) fn clone_or_update(
    url: &str,
    dest: &Path,
    git_ref: Option<&str>,
    repository: &Repository,
    mode: RunMode,
    reporter: &Reporter,
) -> Result<(), Error> {
    // What is at the destination is asked the way every other action asks it,
    // and the final symlink is never followed. That is the whole of rule 13
    // here: a link out to somebody's checkout is refused rather than fetched
    // into, and a link holding nothing is cleared rather than handed to `git`
    // as a working directory.
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
        // A regular file, a device, or a symlink that leaves the repository and
        // lands on something. All of them hold someone's data, including — and
        // this is the one that matters — a link to a checkout somewhere else,
        // which an update reached through would silently fetch into.
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

/// Clone into a destination nothing is at.
///
/// A declared `ref` is checked out after the clone rather than passed as
/// `--branch`, which refuses a raw commit — but the two are one act and report
/// one line, because a clone that was never anywhere else has nothing to have
/// switched from.
fn clone(
    url: &str,
    dest: &Path,
    git_ref: Option<&str>,
    mode: RunMode,
    reporter: &Reporter,
) -> Result<(), Error> {
    // `git clone` would make the missing parents itself, but not the broken
    // symlink at an ancestor that rule 13 clears, and not the report that goes
    // with clearing one.
    for link in directory::create_parents(dest, mode)?.removals() {
        reporter.info(&link.removal_note(mode));
    }
    if mode.writes() {
        // The destination is spelled as an argument rather than as a working
        // directory, and after `--`, so a source that begins with a dash is
        // read as a repository and a destination that is not valid UTF-8 is
        // still a path git can be handed.
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
        if let Some(git_ref) = git_ref {
            // The outcome is not reported: whatever it took to get there, the
            // line below is where this run put the repository.
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

/// ` at <ref>`, or nothing where none was declared.
///
/// The declared string rather than what it resolved to: what a reader can check
/// against the manifest is what they wrote, and in a dry run nothing has been
/// resolved at all.
fn at(git_ref: Option<&str>) -> String {
    git_ref.map(|it| format!(" at {it}")).unwrap_or_default()
}

/// Bring an existing clone up to date, conservatively, or say why it was left
/// alone.
///
/// The rules are `docs/repoformat.md`'s, and every one of them is about not
/// destroying work: a dirty worktree, a branch tracking nothing, and a history
/// that has diverged are each **skipped with a warning rather than failed**,
/// because none of them is a repository bug and a `sync` that stopped on one
/// would strand every action after it. A network failure is not in that
/// company and does fail.
///
/// Only ever reached when the mode allows a write, so nothing below asks about
/// it.
///
/// Below this, two routines rather than one with a flag, because a declared
/// `ref` changes
/// what the steady state *is*: without one, an update follows whatever branch
/// the clone is on and a detached `HEAD` is a skip; with one, detached at the
/// right object is exactly right and the upstream is never consulted. What they
/// share is the pair of gates above them — a dirty worktree blocks either.
fn update(dest: &Path, git_ref: Option<&str>, reporter: &Reporter) -> Result<(), Error> {
    inspect(dest)?;

    if is_dirty(dest)? {
        return skip(reporter, dest, "it has uncommitted changes");
    }
    match git_ref {
        Some(git_ref) => update_to_ref(dest, git_ref, reporter),
        None => update_tracking(dest, reporter),
    }
}

/// Bring a clone to the `ref` its record declares, and say what that took.
///
/// One line per entry per run: `unchanged` where the checkout is already at
/// what the ref resolved to, `updated` where it only moved forward on the
/// branch it was already on, and `switched` where the checkout itself changed.
/// A history that cannot be fast-forwarded has already warned and reports
/// nothing more.
fn update_to_ref(dest: &Path, git_ref: &str, reporter: &Reporter) -> Result<(), Error> {
    // Before the ref is resolved, and the only command here that reaches the
    // network: what `main` means is decided by what upstream published, not by
    // what this clone last heard.
    //
    // `--all` rather than a bare fetch, because a bare one fetches the remote
    // the *current branch* tracks and [`resolve`] searches every remote: on a
    // clone with two, following a branch the second one publishes would
    // otherwise resolve against whatever was last fetched, or report a branch
    // that exists as one that does not. Every clone batfiles makes has exactly
    // one remote, so this differs from a bare fetch only where the user added
    // another.
    run(Some(dest), "fetch", &["fetch", "--all"], dest)?;

    match follow(dest, git_ref, reporter)? {
        Followed::Unchanged => reporter.detail(1, &format!("unchanged {}", dest.display())),
        Followed::Advanced => reporter.info(&format!(
            "{} {}",
            Verb::Update.say(RunMode::Perform),
            dest.display()
        )),
        Followed::Switched => reporter.info(&format!(
            "{} {} to {git_ref}",
            Verb::SwitchRef.say(RunMode::Perform),
            dest.display()
        )),
        Followed::Skipped => {}
    }
    Ok(())
}

/// What following a declared `ref` came to.
enum Followed {
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
fn follow(dest: &Path, git_ref: &str, reporter: &Reporter) -> Result<Followed, Error> {
    match resolve(dest, git_ref)? {
        Target::Branch { remote } => branch(dest, git_ref, &remote, reporter),
        Target::Object { commit } => detach(dest, &commit),
    }
}

/// What a declared `ref` turned out to name.
enum Target {
    /// A branch on a remote, held here as its full `refs/remotes/…` name. A
    /// local branch of the declared name follows it.
    Branch { remote: String },
    /// Anything else that resolves — a tag, a commit, a full ref name — held as
    /// the commit it resolved to.
    Object { commit: String },
}

/// Decide which of the two a declared `ref` is, **looking in the
/// remote-tracking namespace first**.
///
/// Handing the bare string to `rev-parse` would be the obvious implementation
/// and is wrong for the case the field exists for: in a clone, `main` names
/// `refs/heads/main`, which a fetch never moves, so `ref=main` would pin the
/// worktree to the commit it was first cloned at and report success from then
/// on. Asking the remote-tracking namespace is what makes `ref=main` mean "what
/// upstream publishes on main", which is what a plugin list is asking for.
///
/// Which remote is a question rather than a constant: a clone batfiles adopted
/// rather than made need not spell its remote `origin`. Where more than one has
/// the ref, `origin` decides, since that is the one `git clone` makes.
fn resolve(dest: &Path, git_ref: &str) -> Result<Target, Error> {
    // Both halves of the lookup refuse to evaluate the ref as an *expression*,
    // and both are needed. A `ref` is a string the manifest wrote, so it can be
    // `HEAD` or `main~1`, which git resolves and neither of which can be a
    // branch: `refs/remotes/origin/HEAD` is a real ref in any ordinary clone,
    // and `refs/remotes/origin/main~1` is something `rev-parse` evaluates
    // happily. Either would enter the branch case here and then fail trying to
    // make a local branch by that name.
    if names_a_branch(dest, git_ref)? {
        for remote in remotes(dest)? {
            let candidate = format!("refs/remotes/{remote}/{git_ref}");
            if has_ref(dest, &candidate)? {
                return Ok(Target::Branch { remote: candidate });
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

/// Follow a branch a remote publishes, on a local branch of the declared name.
///
/// The local branch is made to track the remote one even though nothing here
/// reads that: it is what makes `git pull` inside the clone do what the manifest
/// says, and what leaves 4.3's routine an upstream to follow if the `ref` is
/// later dropped from the record.
fn branch(
    dest: &Path,
    git_ref: &str,
    remote: &str,
    reporter: &Reporter,
) -> Result<Followed, Error> {
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
    // A switch is the more surprising of the two facts and is what the line
    // reports; the fast-forward that may have followed it is implied.
    Ok(if switching {
        Followed::Switched
    } else {
        advanced
    })
}

/// Fast-forward the checked-out branch onto `remote`, or say why it was left
/// alone.
fn advance(dest: &Path, remote: &str, reporter: &Reporter) -> Result<Followed, Error> {
    if commit(dest, "HEAD")? == commit(dest, remote)? {
        return Ok(Followed::Unchanged);
    }
    // The same question the tracking routine asks, for the same reason: this is
    // what separates a history batfiles declines to touch from a merge that
    // broke.
    if !ancestor(dest, "HEAD", remote)? {
        skip(
            reporter,
            dest,
            &format!("it has commits that {} does not", named(remote)),
        )?;
        return Ok(Followed::Skipped);
    }
    if let Some(path) = overwritten_by(dest, remote)? {
        skip(reporter, dest, &in_the_way(&path, named(remote)))?;
        return Ok(Followed::Skipped);
    }
    run(Some(dest), "merge", &["merge", "--ff-only", remote], dest)?;
    Ok(Followed::Advanced)
}

/// The first path a fast-forward would create that something is already at, if
/// there is one.
///
/// **`git merge --ff-only` replaces an ignored file without a word**, and
/// [`is_dirty`] cannot see one by design: ignored build output deliberately does
/// not block an update. That is fine right up until upstream starts tracking a
/// path the user keeps their own copy at, and then a fast-forward silently
/// overwrites it. Git refuses this for an *untracked* file and not for an
/// ignored one, so the question is asked here instead — [`KEEP_IGNORED`] is the
/// same decision where git offers a flag for it, and merge offers none.
///
/// Only an added path can collide. One already tracked at `HEAD` would have made
/// the worktree dirty, which is asked before this and answered by leaving the
/// clone alone.
///
/// **`--no-renames` is what makes "added" mean it.** Rename detection is on by
/// default, so a file upstream moved onto this path is an `R` rather than an
/// `A` and a filter on additions passes straight over it — while the merge
/// writes the path exactly as it would for an addition. Turning detection off
/// splits every rename back into a delete and an add, which is the shape this
/// question is about. Passed explicitly rather than left to the default for
/// [`is_dirty`]'s reason: `diff.renames` is a user's display preference, and a
/// safety decision must not turn on one.
///
/// The path is relative to the clone, which is how git prints it and how a
/// reader will look for it. `-z` so an unusual name arrives as its own bytes
/// rather than quoted.
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
fn named(remote: &str) -> &str {
    remote.strip_prefix("refs/remotes/").unwrap_or(remote)
}

/// Sit the worktree on one object, detached.
///
/// Detached at the right object is the steady state for a tag or a commit, so
/// being on a *branch* that happens to point there is still a switch: leaving it
/// would let the next upstream push move a checkout the record pinned.
fn detach(dest: &Path, commit_id: &str) -> Result<Followed, Error> {
    if head_branch(dest)?.is_none() && commit(dest, "HEAD")? == commit_id {
        return Ok(Followed::Unchanged);
    }
    run(
        Some(dest),
        "checkout",
        &["checkout", KEEP_IGNORED, "--detach", commit_id, "--"],
        dest,
    )?;
    Ok(Followed::Switched)
}

/// What stops a checkout replacing a file the user keeps and git ignores.
///
/// **Git protects an untracked file from being overwritten by a checkout and an
/// ignored one it does not**, on the reasoning that ignored content is build
/// output. That reasoning is right for the tool that made the file and wrong for
/// batfiles, which did not: a `.gitignore` entry says "do not commit this", and
/// what is at that path may be somebody's local notes or a machine-local config
/// the branch being switched to happens to track. It is data batfiles did not
/// create, so it is not batfiles' to replace (`guidance.md`, rule 13).
///
/// The refusal comes from git and names the path.
const KEEP_IGNORED: &str = "--no-overwrite-ignore";

/// What a revision resolves to, or `None` where it resolves to nothing.
///
/// `--verify --quiet` is git's own spelling of that question: exit 1 and no
/// output for a name it does not have, which [`ask`] keeps apart from the 128 it
/// uses for a repository it cannot read.
///
/// **This resolves revision expressions**, which is what it is for on the object
/// path and exactly what must not decide a branch. [`has_ref`] is the question to
/// ask about a ref by name.
fn verify(dest: &Path, revision: &str) -> Result<Option<String>, Error> {
    ask(
        dest,
        "rev-parse",
        &["rev-parse", "--verify", "--quiet", revision],
    )
}

/// Whether a ref exists under exactly that name.
///
/// `show-ref --verify` takes a full ref name and evaluates nothing: no `~`, no
/// `@{…}`, no shorthand. That is the whole reason it is here rather than a
/// second [`verify`].
fn has_ref(dest: &Path, name: &str) -> Result<bool, Error> {
    Ok(ask(dest, "show-ref", &["show-ref", "--verify", "--quiet", name])?.is_some())
}

/// Whether a declared `ref` is a name a local branch could have.
///
/// The gate in front of the remote-tracking lookup. `check-ref-format` rejects
/// `HEAD`, `main~1`, and a leading dash, which is precisely the set that
/// resolves to something and cannot be branched.
///
/// Not asked through [`ask`], and this is the one place that is right: the
/// command validates a string and opens no repository, so a non-zero status
/// cannot mean "this clone cannot be read" — the distinction `ask` exists to
/// keep. It spells its refusal 128 all the same.
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
        return skip(reporter, dest, "it is not on a branch that tracks a remote");
    };

    // The one command that reaches the network, and the one failure here that is
    // an error rather than a warning.
    run(Some(dest), "fetch", &["fetch"], dest)?;

    if commit(dest, "HEAD")? == commit(dest, &upstream)? {
        reporter.detail(1, &format!("unchanged {}", dest.display()));
        return Ok(());
    }
    // Asked before the merge rather than read out of its failure: this is what
    // separates a history batfiles declines to touch from a merge that broke,
    // and deciding it by parsing git's diagnostic would be guessing.
    if !ancestor(dest, "HEAD", &upstream)? {
        return skip(
            reporter,
            dest,
            &format!("it has commits that {upstream} does not"),
        );
    }
    // A file the clone keeps and upstream has started tracking. Ignored content
    // does not block an update, but it is not batfiles' to overwrite either.
    if let Some(path) = overwritten_by(dest, &upstream)? {
        return skip(reporter, dest, &in_the_way(&path, &upstream));
    }
    run(
        Some(dest),
        "merge",
        &["merge", "--ff-only", &upstream],
        dest,
    )?;
    // No `from <url>`, unlike the clone line. What was fetched is the remote
    // the clone was made with, and batfiles neither compares that against the
    // manifest's `source` nor rewrites it, so naming the source here would
    // claim a repository this run never contacted.
    reporter.info(&format!(
        "{} {}",
        Verb::Update.say(RunMode::Perform),
        dest.display()
    ));
    Ok(())
}

/// Leave a clone as it is, and say why at a volume that is not hidden by
/// default.
///
/// A warning rather than a note: the user asked for the repository to be up to
/// date and it is not, which is worth knowing even though the run continues.
fn skip(reporter: &Reporter, dest: &Path, because: &str) -> Result<(), Error> {
    reporter.warn(&format!("not updating {}: {because}", dest.display()));
    Ok(())
}

/// Confirm that the directory at `dest` is a clone of its own, with a checkout.
///
/// Three questions, and the first two are here because **neither one alone
/// establishes that git will operate on `dest`.** Each catches what the other
/// misses, and both were verified against git rather than reasoned about:
///
/// **Is the git directory `dest`'s own?** A `.git` that is a symlink to another
/// checkout's git directory leaves git using *that* repository's refs while
/// treating `dest` as the worktree — so a fetch and a fast-forward advance the
/// other checkout's branch. `rev-parse --show-toplevel` does not notice: it
/// reports `dest`, because the worktree really is `dest`. Only the filesystem
/// says so, and what `git clone` makes is a real directory, so anything else —
/// a symlink, or the file a linked worktree and a submodule use — is refused.
///
/// **Does git agree the worktree is `dest`?** A real `.git` whose config sets
/// `core.worktree` elsewhere points every command at that tree, and no
/// filesystem check can see it. This is the half `--show-toplevel` does answer,
/// and it is also what keeps a plain directory *inside* somebody's repository
/// from being updated as though it were its own clone — asked from in there, git
/// reports the enclosing repository, and that is a mismatch.
///
/// **Does `HEAD` name a commit?** A clone that was interrupted leaves a `.git`
/// with nothing checked out, and that is the wreckage a later run must not
/// mistake for finished work. Asked through [`run`] so that a repository which
/// is damaged rather than merely incomplete arrives with git's own account of it
/// rather than being folded into the same answer.
fn inspect(dest: &Path) -> Result<(), Error> {
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
        Some(root) if paths::resolved(&root) == paths::resolved(dest) => {}
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

/// One path printed by git, with its line terminator removed and nothing else.
///
/// Not `trim`: a destination whose last component ends in a space is a path git
/// prints faithfully, and trimming would take the space along with the newline
/// and then report a healthy clone as rooted somewhere else. Not
/// `from_utf8_lossy` either, on the platform where a path is bytes — a home that
/// is not valid UTF-8 would be mangled into the same mistake.
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

/// Whether the worktree or the index holds anything an update might disturb.
///
/// Staged, unstaged, and untracked content all count, and
/// `--untracked-files=normal` is passed rather than left to the default: a user
/// with `status.showUntrackedFiles = no` in their gitconfig would otherwise get
/// a `--porcelain` listing with the untracked half missing, and a safety
/// decision must not turn on somebody's display preference. Ignored build and
/// cache output still does not count, which is what `--porcelain` leaves out.
///
/// The bytes are weighed rather than decoded: git quotes an unusual path in
/// this listing, but the only question here is whether it said anything at all.
fn is_dirty(dest: &Path) -> Result<bool, Error> {
    let status = run(
        Some(dest),
        "status",
        &["status", "--porcelain", "--untracked-files=normal"],
        dest,
    )?;
    Ok(!status.stdout.iter().all(u8::is_ascii_whitespace))
}

/// The remote-tracking branch the checked-out branch follows, if it follows one.
///
/// `None` covers a detached `HEAD` and a branch with no upstream configured
/// alike: in both, there is nothing this routine could move the worktree
/// towards, which is why both are skips. A record declaring a `ref` says where
/// to move it and never reaches here.
///
/// **Not one `rev-parse @{upstream}`**, because that command exits 128 for every
/// way of having no upstream *and* for a `branch.<name>.merge` that names
/// something unusable — so reading its failure as "tracks nothing" would report
/// a broken configuration as an ordinary skip and let the run succeed. Verified
/// against git rather than assumed: detached, unset, and malformed all give 128.
///
/// So the two absences are asked of commands that spell theirs as exit 1, which
/// git documents and keeps apart from its 128 for trouble, and the lookup itself
/// then goes through [`run`] — by the time it is reached the branch is known to
/// have an upstream configured, so anything it says is a failure really is one.
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
///
/// `merge-base --is-ancestor` answers with its exit status and prints nothing,
/// so a *no* is not a failure and must not be read as one. It is spelled 1, and
/// only 1: git documents that status for the negative answer and uses the rest
/// for actual trouble, so a 128 from missing or corrupt objects becomes an error
/// here rather than a divergence warning over a repository that cannot be read.
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

/// Ask git one question whose *no* it spells as exit 1, and read the answer.
///
/// **Only exit 1 is an answer.** Git documents that status for the absences
/// these callers are asking about — a detached `HEAD`, a config key nobody set
/// — and uses 128 for a repository it cannot read. Folding those together is
/// how a broken configuration becomes a warning and a successful run, so
/// everything that is not 0 or 1 keeps git's diagnostic and fails.
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
///
/// Only ever called on that answer, which is what makes it safe: a link into the
/// repository or one reaching nothing gives access to no data, so clearing it
/// destroys nothing (`guidance.md`, rule 13).
fn remove(dest: &Path) -> Result<(), Error> {
    fs::remove_file(dest).map_err(|source| Error::Write {
        path: dest.to_path_buf(),
        source,
    })
}

/// The environment variables that tell git to work somewhere other than where
/// it is standing, all of which batfiles clears.
///
/// **This is the structural half of "act on the destination, and nothing
/// else".** [`inspect`] checks the destination, but a check can only cover what
/// it thought to look at, and an inherited `GIT_DIR` is not on disk to be looked
/// at: with one set, `.git` at the destination is still a real directory and
/// `--show-toplevel` still answers with the destination, while `fetch` and
/// `merge` move the refs of whatever `GIT_DIR` names. Verified, not assumed.
/// Clearing the family removes the whole class instead of detecting instances,
/// including the three that redirect part of an operation without moving the
/// repository at all — the index, the object store, and the ref namespace.
///
/// Whoever ran batfiles need not have set any of these deliberately. Git exports
/// several of them to what it runs, so batfiles is quite likely to be one of
/// those children: `git submodule foreach` exports `GIT_DIR`, a `pre-commit`
/// hook gets `GIT_INDEX_FILE`, and `git rebase --exec` gets `GIT_PREFIX`.
///
/// **This is not a security boundary, and the list is short because of that.**
/// Against a parent that means harm, `GIT_CONFIG_GLOBAL` and `GIT_SSH_COMMAND`
/// are every bit as potent and are kept deliberately — rule 6 exists so the
/// user's own setup applies, so nothing naming their configuration, their
/// credentials, or their transport is touched. What this defends against is
/// state inherited by accident.
///
/// Two entries are worth their own note, because what justifies them is not
/// what it looks like:
///
/// - **`GIT_CONFIG` redirects nothing.** In modern git it is `git config
///   --file`, so it reaches that one command and no other; a `core.worktree` in
///   the file it names does not move `--show-toplevel`. It is cleared for a
///   different reason: [`upstream`] asks `git config --get` a question *about
///   this repository*, and a `GIT_CONFIG` set for the user's own use of the
///   `git config` command would have it answered out of an unrelated file.
///   Defending the call site instead is not available — `--local` alongside
///   `GIT_CONFIG` is "only one config file at a time".
/// - **`GIT_CONFIG_COUNT` is the weakest entry here.** Git ignores
///   `core.worktree` from that scope, as it does from `-c`, so the redirect it
///   appears to offer is not real; and anything else it can set, the global
///   config it is kept alongside can set too. What clearing it costs is a
///   plausible one-off — an `http.proxy` or an `http.extraHeader` in front of a
///   single `sync`. It stays on the list pending a decision, not because the
///   case for it is strong.
const REDIRECTS: [&str; 12] = [
    // Where the repository is.
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    // How it is found from the working directory, which decides nothing today:
    // `inspect` has already established a `.git` at the destination, so
    // discovery stops there and never walks up. These only ever narrow it.
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    "GIT_PREFIX",
    // Which parts of it a command reads and writes.
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_NAMESPACE",
    // Configuration that reaches one invocation. Neither redirects git; see the
    // note above for what each is actually doing here.
    "GIT_CONFIG",
    "GIT_CONFIG_COUNT",
];

/// Run `git`, capturing both of its streams.
///
/// Neither may inherit batfiles': the questions above are asked by running
/// commands that are *expected* to fail, and `fatal: not a git repository` on
/// the terminal would be batfiles reporting its own inspection as a problem.
/// Captured standard error is shown only where a command that had to succeed
/// did not.
///
/// The working directory and [`REDIRECTS`] together are what decide which
/// repository is acted on: git is stood in the destination and left no way of
/// being pointed anywhere else.
fn git<S: AsRef<OsStr>>(dir: Option<&Path>, args: &[S]) -> Result<Output, Error> {
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
        .map_err(|source| Failure::Unavailable { source }.into())
}

/// What git said about a failure, or the status it exited with when it said
/// nothing.
///
/// The one place a `git` failure is put into words, so [`Error::GitFailed`]'s
/// message field is filled from here and from nowhere else — which is what keeps
/// it a fact about a subprocess rather than the free-form field rule 5 warns
/// about.
fn complaint(output: &Output) -> String {
    let said = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if said.is_empty() {
        format!("git exited with {}", output.status)
    } else {
        said
    }
}
