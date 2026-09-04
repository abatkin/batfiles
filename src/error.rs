//! The crate's error type.
//!
//! One enum for the whole program, grouped by what was being done when it
//! failed. Callers report an error rather than matching on it, so a variant
//! carries only what its message needs; it earns more when something needs to
//! read it.
//!
//! A variant carries the facts and its `#[error]` attribute carries the prose.
//! No variant holds a message formatted at the call site (`guidance.md`,
//! rule 5).

use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::archive;
use crate::clone_list;
use crate::item::{ItemAddress, ItemAddressError};
use crate::manifest;
use crate::paths::ExistingNode;

#[derive(Debug, Error)]
pub(crate) enum Error {
    // Where batfiles works.
    /// No home directory could be determined for a command that needs one.
    #[error("could not determine a home directory")]
    HomeUnavailable,

    /// The working directory a relative path had to be anchored against could
    /// not be determined.
    #[error("could not determine the current directory: {source}")]
    WorkingDirectory { source: io::Error },

    // Touching the filesystem, for documents, actions, and state files alike.
    /// A path that could not be read or inspected: a document off the disk, or
    /// a destination whose existing node had to be identified.
    #[error("could not read {}: {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },

    /// A path that could not be created or replaced.
    #[error("could not write {}: {source}", .path.display())]
    Write { path: PathBuf, source: io::Error },

    // The documents batfiles parses.
    /// A document that was read but is not valid TOML. The `toml` message is a
    /// multi-line excerpt pointing at the value, so it goes last and on its own
    /// line.
    #[error("invalid TOML in {}:\n{source}", .path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },

    /// A document that could not be turned back into TOML to be written.
    #[error("could not serialize {}: {source}", .path.display())]
    Serialize {
        path: PathBuf,
        source: toml::ser::Error,
    },

    /// A manifest that is valid TOML but breaks a rule serde cannot express.
    /// The rules themselves are in [`manifest::Invalid`].
    #[error("invalid configuration in {}: {source}", .path.display())]
    InvalidManifest {
        path: PathBuf,
        source: manifest::Invalid,
    },

    /// A command-line argument that is not a well-formed address. The rejected
    /// value travels inside [`ItemAddressError`], which already renders the
    /// whole message.
    #[error(transparent)]
    InvalidAddress(#[from] ItemAddressError),

    // Naming what to apply.
    /// An `apply-action` naming an address no record in the manifest answers
    /// to. The manifest is named because which repository was read is half the
    /// answer when a command resolves nothing.
    #[error("no action in {} has the id `{id}`", .path.display())]
    UnknownAction { path: PathBuf, id: ItemAddress },

    /// An `apply-group` naming a group no record in the manifest belongs to. A
    /// group exists because some action names it, so an empty one and an absent
    /// one are the same thing.
    #[error("no action in {} is in the group `{group}`", .path.display())]
    UnknownGroup { path: PathBuf, group: ItemAddress },

    // Carrying an action out.
    /// An action naming a source the repository does not contain.
    #[error("no such file in the repository: {}", .path.display())]
    SourceMissing { path: PathBuf },

    /// A `source-dir` naming something the repository holds, but not a
    /// directory. There are no children to link, and linking the thing itself
    /// is what a `symlink` action is for.
    #[error("not a directory: {}", .path.display())]
    SourceNotADirectory { path: PathBuf },

    /// A symlink found inside something a `copy` action was reproducing.
    /// Following it would turn a link the repository chose into a detached
    /// file, and recreating it would re-read a relative target from a directory
    /// it is no longer in.
    #[error(
        "cannot copy {}: it is a symlink, and copying installs files and directories; \
         `symlink` is the action for a link",
        .path.display()
    )]
    SourceIsSymlink { path: PathBuf },

    /// An action whose destination is inside the directory it installs from.
    /// The destination would become a child of the source, and the action would
    /// then work on what it was writing.
    ///
    /// The field is not called `source`: `thiserror` reads that name as the
    /// error this one wrapped, and every other variant here uses it that way.
    #[error(
        "cannot install {} into {}, which is inside it",
        .installed.display(),
        .dest.display()
    )]
    DestinationInsideSource { installed: PathBuf, dest: PathBuf },

    /// Something is at a path an install would be built on — the staging node,
    /// or the scratch file an archive is downloaded to. Very likely an earlier
    /// run's, but batfiles does not remove what it did not create, so clearing
    /// it is the user's call.
    #[error(
        "cannot install: something is already at {}. If it is left over from an \
         interrupted run, remove it and run sync again",
        .path.display()
    )]
    StagingPathTaken { path: PathBuf },

    /// A socket, a fifo, a device — something with no meaningful copy.
    #[error(
        "cannot copy {}: it is neither a regular file nor a directory",
        .path.display()
    )]
    SourceNotCopyable { path: PathBuf },

    /// A dot-prefixed action over a child whose name already starts with `.`.
    /// The link would be `..name`, which is a legal file name and never the
    /// one that was meant.
    #[error(
        "`{child}` already starts with a dot, so `dot-prefix` would install it as `.{child}`; \
         a dot-prefixed directory holds undotted names"
    )]
    DotPrefixOnDotfile { child: String },

    /// A destination holding something batfiles did not create and cannot
    /// safely replace (`guidance.md`, rule 13). What was found there is
    /// [`ExistingNode`], which [`crate::paths`] decides and renders.
    #[error(
        "{} already exists and is {found}; move it aside and run sync again",
        .path.display()
    )]
    DestinationExists { path: PathBuf, found: ExistingNode },

    /// An action type this build of batfiles cannot carry out on this platform.
    #[error("`{action_type}` actions are not supported on this platform")]
    UnsupportedOnPlatform { action_type: &'static str },

    /// A clone list that was read and breaks one of its own rules. The rules,
    /// and why the line rather than the file is what a fault is attached to,
    /// are [`clone_list::Invalid`]'s.
    #[error("invalid clone list in {}, line {line}: {source}", .path.display())]
    CloneList {
        path: PathBuf,
        line: usize,
        source: clone_list::Invalid,
    },

    // Fetching over the network. Flat for now: 4.5 is where a caller first has
    // to tell one of these apart from another, and nesting waits for that
    // (`guidance.md`, rule 5).
    /// A URL that could not be fetched at all: the name did not resolve, the
    /// connection was refused or interrupted, TLS was not established, or the
    /// redirects did not end.
    #[error("could not fetch {url}: {source}")]
    Fetch { url: String, source: ureq::Error },

    /// A server that answered, with something other than the file. Kept apart
    /// from [`Self::Fetch`] because it is a manifest problem rather than a
    /// machine one: a 404 means the URL names something that is not there.
    #[error("could not fetch {url}: the server answered {status}")]
    FetchStatus { url: String, status: u16 },

    /// Bytes that arrived whole but are not the ones the manifest named.
    #[error(
        "{url} does not match the declared sha256:\n  declared {expected}\n  received {actual}"
    )]
    DigestMismatch {
        url: String,
        expected: String,
        actual: String,
    },

    // Cloning and updating repositories. Flat for the same reason the fetching
    // variants are: 4.5 is where a caller first has to tell one of these apart
    // from another, and nesting waits for that (`guidance.md`, rule 5). The
    // conservative update rules produce warnings rather than errors, so nothing
    // matches on these yet.
    /// `git` could not be run at all, most often because it is not on `PATH`.
    #[error(
        "could not run git: {source}. batfiles runs the `git` on your PATH so that your \
         gitconfig, credential helpers, and SSH agent apply"
    )]
    GitUnavailable { source: io::Error },

    /// A `git` command ran and failed, carrying git's own diagnostic. The
    /// message is built by one function in [`crate::git`] rather than at each
    /// call site, so it stays a fact about a subprocess.
    #[error("git {command} failed in {}: {message}", .path.display())]
    GitFailed {
        command: &'static str,
        path: PathBuf,
        message: String,
    },

    /// A clone destination holding a directory with no `.git` in it. Named
    /// apart from [`Self::DestinationExists`] because the remedy is the same
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

    /// An archive that arrived whole and cannot be unpacked. Nested rather than
    /// flat, because extraction has nine failures with vocabulary of their own
    /// — entries, roots, formats — that no other part of the tool shares
    /// (`guidance.md`, rule 5).
    ///
    /// The URL is here rather than on each of them, so every [`archive::Invalid`]
    /// is a predicate about the archive this names and no pass inside that
    /// module has to carry a URL to say what went wrong.
    #[error("the archive from {url} {source}")]
    Archive {
        url: String,
        source: archive::Invalid,
    },
}

impl Error {
    /// Whether the failure was simply that the file does not exist.
    ///
    /// The state files treat that as an empty document; a leaf `batfiles.toml`
    /// does not. That difference is the whole reason this is asked, and
    /// [`crate::tomlfile::read_or_default`] is the one place that asks it.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Read { source, .. } if source.kind() == io::ErrorKind::NotFound)
    }
}
