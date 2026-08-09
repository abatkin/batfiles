//! The repository format: `batfiles.toml` and the records inside it.
//!
//! [`BatfilesConfig`] is the document; the other modules are the records it is
//! built from, one file per section. All of it is the repository format
//! expressed as Rust types, and deliberately only that: a document parses
//! whenever it is syntactically a valid `batfiles.toml`, and the rules that span
//! fields, name another record, or depend on whether the repository is the leaf
//! or a remote are applied by the code that consumes the result.
#![allow(dead_code, reason = "no command loads a repository yet")]

mod action;
mod batfiles_config;
mod default_disabled;
mod remote;
mod value;
mod var_decl;

/// The records live in one file per section, but they are one schema, so the
/// module presents them flat. Nothing consumes the manifest yet, hence the
/// allow.
#[allow(unused_imports, reason = "no command loads a repository yet")]
pub(crate) use {
    action::{
        Action, CopyAction, CreateDirAction, FetchUrlAction, GitCloneAction, GitCloneListAction,
        IncludeRemoteAction, SymlinkAction,
    },
    batfiles_config::BatfilesConfig,
    default_disabled::{DefaultDisabled, DefaultDisabledAction, DefaultDisabledGroup},
    remote::{ArchiveRemote, FileRemote, GitRemote, Remote},
    value::{Condition, DurationString, GlobFilter, ItemIdList, RemotePath, RepoPath},
    var_decl::{Capture, CommandSpec, DynamicVar, VarDecl},
};
