//! The crate's error type.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::archive;
use crate::clone_list;
use crate::git;
use crate::item::{ItemAddress, ItemAddressError};
use crate::manifest;
use crate::paths::ExistingNode;
use crate::var::{VarName, VarNameError};

#[derive(Debug, Error)]
pub(crate) enum Error {
    // Where batfiles works.
    /// No home directory could be determined for a command that needs one.
    #[error("could not determine a home directory")]
    HomeUnavailable,

    /// The working directory a relative path had to be anchored against could not be
    /// determined.
    #[error("could not determine the current directory: {source}")]
    WorkingDirectory { source: io::Error },

    // Touching the filesystem, for documents, actions, and state files alike.
    /// A path that could not be read or inspected: a document off the disk, or a
    /// destination whose existing node had to be identified.
    #[error("could not read {}: {source}", .path.display())]
    Read { path: PathBuf, source: io::Error },

    /// A path that could not be created or replaced.
    #[error("could not write {}: {source}", .path.display())]
    Write { path: PathBuf, source: io::Error },

    // The documents batfiles parses.
    /// A document that was read but is not valid TOML.
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
    #[error("invalid configuration in {}: {source}", .path.display())]
    InvalidManifest {
        path: PathBuf,
        source: manifest::Invalid,
    },

    /// A command-line argument that is not a well-formed address.
    #[error(transparent)]
    InvalidAddress(#[from] ItemAddressError),

    /// A command-line argument that is not a well-formed variable name. The key
    /// is quoted here because [`VarNameError`] states the rule alone: that is
    /// what a serde key error wants, where TOML supplies the position, and it
    /// leaves a diagnostic about an argument with nothing to point at.
    #[error("invalid variable name `{key}`: {source}")]
    InvalidVarName { key: String, source: VarNameError },

    /// A `vars get` naming a variable this machine has no value for.
    #[error("`{key}` has no machine-local value")]
    VarNotSet { key: VarName },

    // Naming what to apply.
    /// An `apply-action` naming an address no record in the manifest answers to.
    #[error("no action in {} has the id `{id}`", .path.display())]
    UnknownAction { path: PathBuf, id: ItemAddress },

    /// An `apply-group` naming a group no record in the manifest belongs to.
    #[error("no action in {} is in the group `{group}`", .path.display())]
    UnknownGroup { path: PathBuf, group: ItemAddress },

    // Carrying an action out.
    /// An action naming a source the repository does not contain.
    #[error("no such file in the repository: {}", .path.display())]
    SourceMissing { path: PathBuf },

    /// A `source-dir` naming something the repository holds, but not a directory.
    #[error("not a directory: {}", .path.display())]
    SourceNotADirectory { path: PathBuf },

    /// A symlink found inside something a `copy` action was reproducing.
    #[error(
        "cannot copy {}: it is a symlink, and copying installs files and directories; \
         `symlink` is the action for a link",
        .path.display()
    )]
    SourceIsSymlink { path: PathBuf },

    /// An action whose destination is inside the directory it installs from.
    #[error(
        "cannot install {} into {}, which is inside it",
        .installed.display(),
        .dest.display()
    )]
    DestinationInsideSource { installed: PathBuf, dest: PathBuf },

    /// Something is at a path an install would be built on — the staging node, or the
    /// scratch file an archive is downloaded to.
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
    #[error(
        "`{child}` already starts with a dot, so `dot-prefix` would install it as `.{child}`; \
         a dot-prefixed directory holds undotted names"
    )]
    DotPrefixOnDotfile { child: String },

    /// A destination holding something batfiles did not create and cannot safely
    /// replace (`guidance.md`, rule 13).
    #[error(
        "{} already exists and is {found}; move it aside and run sync again",
        .path.display()
    )]
    DestinationExists { path: PathBuf, found: ExistingNode },

    /// An action type this build of batfiles cannot carry out on this platform.
    #[error("`{action_type}` actions are not supported on this platform")]
    UnsupportedOnPlatform { action_type: &'static str },

    /// A clone list that was read and breaks one of its own rules.
    #[error("invalid clone list in {}, line {line}: {source}", .path.display())]
    CloneList {
        path: PathBuf,
        line: usize,
        source: clone_list::Invalid,
    },

    /// A URL that could not be fetched at all: the name did not resolve, the connection
    /// was refused or interrupted, TLS was not established, or the redirects did not
    /// end.
    #[error("could not fetch {url}: {source}")]
    Fetch { url: String, source: ureq::Error },

    /// A server that answered, with something other than the file.
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

    /// Cloning or updating a repository.
    #[error(transparent)]
    Git(#[from] git::Failure),

    /// An archive that arrived whole and cannot be unpacked.
    #[error("the archive from {url} {source}")]
    Archive {
        url: String,
        source: archive::Invalid,
    },
}

impl Error {
    /// Whether the failure was simply that the file does not exist.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Read { source, .. } if source.kind() == io::ErrorKind::NotFound)
    }
}
