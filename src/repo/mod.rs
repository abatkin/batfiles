//! The repository format — `batfiles.toml` and the records inside it — and the
//! one verb that reads it off disk.
//!
//! [`BatfilesConfig`] is the document; the other record modules are the sections
//! it is built from, one file per section. A document parses whenever it is
//! syntactically a valid `batfiles.toml`; the rules that span fields, name
//! another record, or depend on whether the repository is the leaf or a remote
//! are applied afterwards — the repository-wide ones by
//! [`BatfilesConfig::validate`], the rest by the code that consumes the result.
//!
//! [`load::leaf`] turns resolved [`Roots`](crate::config::Roots) into the
//! [`Leaf`] model: the leaf repository plus every remote an `include-remote`
//! selects. It keeps its module rather than joining the flat re-export below —
//! `repo::load::leaf(roots)` reads as a sentence and `repo::load_leaf` does not,
//! and the flat rule is about records rather than verbs.

mod action;
mod batfiles_config;
mod default_disabled;
mod duration;
#[allow(dead_code, reason = "no command dispatches to the loader yet")]
pub(crate) mod load;
#[allow(dead_code, reason = "no command consumes the loaded model yet")]
mod model;
mod remote;
mod value;
mod var_decl;

/// The records live in one file per section, but they are one schema, so the
/// module presents them flat. The manifest is loaded now, but only its
/// `[vars]`, `[remotes]`, and `include-remote` records have readers: the other
/// action variants are parsed and not yet planned, which is `sync`'s.
#[allow(
    unused_imports,
    reason = "actions are parsed but not yet planned or applied"
)]
pub(crate) use {
    action::{
        Action, CopyAction, CreateDirAction, FetchUrlAction, GitCloneAction, GitCloneListAction,
        IncludeRemoteAction, SymlinkAction,
    },
    batfiles_config::BatfilesConfig,
    default_disabled::{DefaultDisabled, DefaultDisabledAction, DefaultDisabledGroup},
    duration::FriendlyDuration,
    model::{IncludedRemote, Inclusion, Leaf, RemoteState, Repository},
    remote::{ArchiveRemote, FileRemote, GitRemote, Remote},
    value::{Condition, GlobFilter, ItemIdList, RemotePath, RepoPath},
    var_decl::{Capture, CommandSpec, DynamicVar, VarDecl},
};
