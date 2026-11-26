//! The crate's error type.

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::commands::init;
use crate::commands::update;
use crate::commands::vars::refresh;
use crate::filesystem::paths::ExistingNode;
use crate::manifest;
use crate::manifest::clone_list;
use crate::manifest::item::{ItemAddress, ItemAddressError, ItemId, ItemKind};
use crate::manifest::template::TemplateError;
use crate::output::quoted_value;
use crate::release;
use crate::source::archive;
use crate::source::fetch;
use crate::source::git;
use crate::source::remotes;
use crate::vars::name::{VarName, VarNameError};

#[derive(Debug, Error)]
pub(crate) enum Error {
    // Where batfiles works.
    /// No home directory could be determined for a command that needs one.
    #[error("could not determine a home directory")]
    HomeUnavailable,

    /// The working directory needed to anchor a relative path could not be determined.
    #[error("could not determine the current directory: {source}")]
    WorkingDirectory { source: io::Error },

    /// Another process holds the run lock under the cache directory.
    #[error(
        "another batfiles run holds {}; try again once it finishes",
        .path.display()
    )]
    RunLockHeld { path: PathBuf },

    /// The run lock could not be created, opened, or taken.
    #[error("could not take the run lock {}: {source}", .path.display())]
    RunLock { path: PathBuf, source: io::Error },

    // Touching the filesystem, for documents, actions, and state files alike.
    /// A path could not be read or inspected.
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

    /// A document could not be serialized to TOML.
    #[error("could not serialize {}: {source}", .path.display())]
    Serialize {
        path: PathBuf,
        source: toml::ser::Error,
    },

    /// Variables whose values read each other. `chain` names them in the order they were
    /// read, ending with the first repeated, joined by ` -> `.
    #[error(
        "variables refer to each other in a cycle: {chain}; a [vars] value cannot read itself, \
         directly or through other variables"
    )]
    VariableCycle { chain: String },

    /// A read that would evaluate more than `limit` templates nested inside each other.
    /// `chain` names the variables in the order they were read, joined by ` -> `.
    #[error(
        "variables read each other too deeply: {chain}; a [vars] value can read through at \
         most {limit} templated variables in a row"
    )]
    VariableDepth { chain: String, limit: usize },

    /// A template field of a record that has no usable value.
    #[error("{record}: {field} {} {source}", quoted_value(.template))]
    Template {
        /// The record's heading, as run output names it.
        record: String,
        field: &'static str,
        /// The template as written.
        template: String,
        source: Box<TemplateError>,
    },

    /// A `run` action's command that did not succeed.
    #[error("{record}: the command {failure}")]
    CommandFailed {
        /// The record's heading, as run output names it.
        record: String,
        failure: crate::subprocess::LaunchFailure,
    },

    /// A manifest that is valid TOML but breaks a rule serde cannot express.
    #[error("invalid configuration in {}: {source}", .path.display())]
    InvalidManifest {
        path: PathBuf,
        source: manifest::ManifestError,
    },

    /// A command-line argument that is not a well-formed address.
    #[error(transparent)]
    InvalidAddress(#[from] ItemAddressError),

    /// An invalid command-line variable name, with the rejected key and validation error.
    #[error("invalid variable name `{key}`: {source}")]
    InvalidVarName { key: String, source: VarNameError },

    /// A `vars get` naming a variable this machine has no value for.
    #[error("`{key}` has no machine-local value")]
    VarNotSet { key: VarName },

    // Naming what to apply.
    /// An `apply-action` target absent from the manifest.
    #[error("no action in {} has the id `{id}`", .path.display())]
    UnknownAction { path: PathBuf, id: ItemAddress },

    /// An `apply-group` target with no records in the manifest.
    #[error("no action in {} is in the group `{group}`", .path.display())]
    UnknownGroup { path: PathBuf, group: ItemAddress },

    /// An apply command's target inside an inclusion the run excluded, whose
    /// manifest was therefore not read. `kind` says what the target names.
    #[error(
        "{kind} `{address}` would come from include-remote `{inclusion}`, which is \
         excluded: {reason}"
    )]
    TargetInExcludedInclusion {
        kind: ItemKind,
        address: ItemAddress,
        inclusion: ItemAddress,
        reason: String,
    },

    /// An apply command's target inside an inclusion whose remote has no
    /// materialization to read. `kind` says what the target names.
    #[error(
        "{kind} `{address}` would come from include-remote `{inclusion}`, which \
         cannot be read: remote `{remote}` is not materialized; run `batfiles sync` \
         to bring it down"
    )]
    TargetInUnreadInclusion {
        kind: ItemKind,
        address: ItemAddress,
        inclusion: ItemAddress,
        remote: ItemId,
    },

    /// An `apply-action` target inside a clone list the run excluded, whose
    /// entries were therefore not read.
    #[error(
        "entry `{address}` would come from git-clone-list `{list}`, which is \
         excluded: {reason}"
    )]
    TargetInExcludedList {
        address: ItemAddress,
        list: ItemAddress,
        reason: String,
    },

    /// An `apply-action` target naming an inclusion instead of an executable action.
    #[error(
        "`{id}` is an include-remote action, which cannot be applied on its own; \
         name one of the actions it includes, as `{id}.<action>`"
    )]
    InclusionNotApplyable { id: ItemAddress },

    // Carrying an action out.
    /// An action naming a source the repository does not contain.
    #[error("no such file in the repository: {}", .path.display())]
    SourceMissing { path: PathBuf },

    /// An action source refers to a remote that has not been materialized.
    #[error(
        "remote `{remote}` is not materialized at {}; run `batfiles sync` to bring it down",
        .path.display()
    )]
    RemoteNotMaterialized { remote: ItemId, path: PathBuf },

    /// A file or archive materialization exists without a matching ownership stamp.
    #[error(
        "cannot materialize remote `{remote}`: {} is {found} that batfiles did not \
         fetch for it; remove it and run sync again",
        .path.display()
    )]
    MaterializationNotFetched {
        remote: ItemId,
        path: PathBuf,
        found: remotes::FilesystemEntryKind,
    },

    /// A remote was fetched, but its ownership stamp could not be saved.
    #[error(
        "remote `{remote}` was fetched to {}, but the stamp recording it could not be \
         written: {source}; delete {} and run sync again",
        .path.display(),
        .path.display()
    )]
    StampNotWritten {
        remote: ItemId,
        path: PathBuf,
        source: Box<Error>,
    },

    /// An action source refers to a remote excluded by its condition.
    #[error("remote `{remote}` is excluded on this machine: {reason}")]
    RemoteExcluded { remote: ItemId, reason: String },

    /// A Git remote's checkout whose origin cannot be shown to be the repository the leaf
    /// declares, so the permissions the leaf grants that remote cannot apply to it. Names no
    /// URL, which may carry credentials.
    #[error(
        "{inclusion}: cannot use the checkout of remote `{remote}` at {}: {reason}, so it cannot \
         be shown to be the repository `[remotes.{remote}]` names, and its {permissions} cannot \
         apply; move the checkout aside and run `batfiles sync` to clone that repository",
        .path.display()
    )]
    CheckoutUnverified {
        inclusion: String,
        remote: ItemId,
        path: PathBuf,
        reason: &'static str,
        permissions: String,
    },

    /// A contributed `run` action the command names, whose remote does not let it run.
    #[error(
        "{record}: remote `{remote}` does not set `allow-commands = true`, so the `run` \
         actions it contributes do not run"
    )]
    CommandsNotAllowed { record: String, remote: ItemId },

    /// A contributed `run` action whose trigger keeps state, from an inclusion without the `id`
    /// its address needs.
    #[error(
        "{record}: its trigger records state under the action's address, which needs an `id` on \
         {inclusion}"
    )]
    StatefulRunInUnnamedInclusion { record: String, inclusion: String },

    /// An included remote's materialization has no manifest.
    #[error(
        "remote `{remote}` has no {} at {}; an include-remote reads one, \
         and a remote that only supplies content to install does not have one",
        crate::manifest::Manifest::FILE_NAME,
        .path.display()
    )]
    IncludedManifestMissing { remote: ItemId, path: PathBuf },

    /// A `source-dir` naming something the repository holds, but not a directory.
    #[error("not a directory: {}", .path.display())]
    SourceNotADirectory { path: PathBuf },

    /// A symlink was found inside a directory being copied.
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

    /// A destination contains the source that would replace it.
    #[error(
        "cannot replace {}: {} is inside it, and setting it aside would take the source with it",
        .dest.display(),
        .installed.display()
    )]
    SourceInsideDestination { installed: PathBuf, dest: PathBuf },

    /// A staging or download scratch path is already occupied.
    #[error(
        "cannot install: something is already at {}. If it is left over from an \
         interrupted run, remove it and run sync again",
        .path.display()
    )]
    StagingPathTaken { path: PathBuf },

    /// A `copy` with entry filters whose source is a file, with no entries to choose among.
    #[error(
        "cannot copy {}: it is a file, and `include` and `exclude` choose entries within a \
         directory",
        .path.display()
    )]
    FilteredSourceIsAFile { path: PathBuf },

    /// A copy source is neither a regular file nor a directory.
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

    /// A tool-owned destination contains an unmanaged node.
    #[error(
        "{} already exists and is {found}; move it aside and run sync again",
        .path.display()
    )]
    DestinationExists { path: PathBuf, found: ExistingNode },

    /// Installation failed after moving the existing node aside, and restoring it also failed.
    #[error("{source}; what was at {} is now at {}", .path.display(), .aside.display())]
    SetAside {
        path: PathBuf,
        aside: PathBuf,
        source: Box<Error>,
    },

    /// Standard input ended while waiting for a conflict decision.
    #[error(
        "no answer about {}: standard input ended; nothing was done there",
        .path.display()
    )]
    NoAnswer { path: PathBuf },

    /// A conflict decision could not be read from standard input.
    #[error("could not read an answer about {}: {source}", .path.display())]
    Prompt { path: PathBuf, source: io::Error },

    /// An action type this build of batfiles cannot carry out on this platform.
    #[error("`{action_type}` actions are not supported on this platform")]
    UnsupportedOnPlatform { action_type: &'static str },

    /// A clone list failed parsing or validation.
    #[error("invalid clone list in {}, line {line}: {source}", .path.display())]
    CloneList {
        path: PathBuf,
        line: usize,
        source: clone_list::CloneListError,
    },

    /// A download failed during connection, transfer, or redirect handling.
    #[error("could not fetch {url}: {source}")]
    Fetch { url: String, source: ureq::Error },

    /// The HTTP server returned an unsuccessful status.
    #[error("could not fetch {url}: the server answered {status}")]
    FetchStatus { url: String, status: u16 },

    /// An invalid local-file URL. Manifest validation normally rejects these before fetching.
    #[error("could not fetch {url}: the URL {source}")]
    FileUrl {
        url: String,
        source: fetch::FileUrlError,
    },

    /// Downloaded content does not match the declared SHA-256 digest.
    #[error(
        "{url} does not match the declared sha256:\n  declared {expected}\n  received {actual}"
    )]
    DigestMismatch {
        url: String,
        expected: String,
        actual: String,
    },

    /// A Git operation failed.
    #[error(transparent)]
    Git(#[from] git::GitError),

    /// An archive could not be unpacked.
    #[error("the archive from {url} {source}")]
    Archive {
        url: String,
        source: archive::ArchiveError,
    },

    /// A file declared `decompress` could not be decompressed.
    #[error("the file from {url} {source}")]
    Decompress {
        url: String,
        source: archive::DecompressError,
    },

    // Refreshing dynamic variables.
    /// Requested dynamic variables could not be refreshed.
    #[error(transparent)]
    Refresh(#[from] refresh::RefreshError),

    // Laying out a new repository.
    /// Initialization refused the directory or failed to initialize Git.
    #[error(transparent)]
    Init(#[from] init::InitError),

    /// The release base the stub or `update` would use is unusable.
    #[error(transparent)]
    ReleaseBase(#[from] release::ReleaseBaseError),

    // Replacing the binary.
    /// `update` could not install a release.
    #[error(transparent)]
    Update(#[from] update::UpdateError),

    // Bringing a repository onto a machine.
    /// The destination for the leaf repository clone is already occupied.
    #[error(
        "{} already exists; `clone` creates the repository it clones into. \
         Move it aside, name another directory with --batfiles-dir, \
         or run `batfiles sync` if it is already the repository you want",
        .path.display()
    )]
    CloneDestinationExists { path: PathBuf },

    /// The cloned repository has no batfiles manifest.
    #[error(
        "{} has no {}; what was cloned is a Git repository, but not a batfiles one",
        .path.display(),
        manifest::Manifest::FILE_NAME
    )]
    ClonedWithoutManifest { path: PathBuf },
}

impl Error {
    /// Attach the path of a failed read to its I/O error.
    pub fn reading(path: &Path) -> impl Fn(io::Error) -> Self + Copy {
        move |source| Self::Read {
            path: path.to_path_buf(),
            source,
        }
    }

    /// Attach the path of a failed write to its I/O error.
    pub fn writing(path: &Path) -> impl Fn(io::Error) -> Self + Copy {
        move |source| Self::Write {
            path: path.to_path_buf(),
            source,
        }
    }

    /// Whether the failure was simply that the file does not exist.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Read { source, .. } if source.kind() == io::ErrorKind::NotFound)
    }
}
