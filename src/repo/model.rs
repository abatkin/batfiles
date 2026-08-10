//! The repository as loaded: the leaf, the remotes its inclusions select, and
//! the inclusions themselves.
//!
//! Everything here is data. [`load`](super::load) is the verb that produces it,
//! and the types are shaped for what a consumer needs rather than for how the
//! manifest is written: the fields record **facts about what was found**, never
//! a decision about what to do with them. Which severity an unmaterialized
//! remote deserves is `sync`'s, `vars list`'s, and `vars refresh`'s to disagree
//! about, so it is not settled here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::item::ItemId;
use crate::repo::batfiles_config::BatfilesConfig;
use crate::var::VarName;

/// One repository — a root directory and the manifest at it.
///
/// A leaf and an included remote share this type. They differ in how the handle
/// was obtained, not in what a consumer does with it: both are asked for the
/// root, which is a dynamic command's working directory and the base for
/// repository source paths, and for the config, which is where `[vars]` lives.
/// One type is what lets a declaration or an action point at "the repository
/// that declared this".
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Repository {
    pub root: PathBuf,
    pub config: BatfilesConfig,
}

/// What the loader found at a remote's materialization root.
///
/// Three facts, not three verdicts. The two materialized cases behave
/// identically for declaration discovery and differently for per-remote
/// reporting, which is why this is an enum on the entry rather than an
/// `Option<Repository>`: an empty manifest, an absent one, and nothing on disk
/// at all are distinguishable only here, and the [`Repository`] beside it stays
/// total so "iterate this repository's declarations" needs no branch.
///
/// A manifest that exists and does *not* parse is absent from this list on
/// purpose: no caller can proceed past it, so the loader fails instead. Adding
/// that variant later is a local change, which is the reason this is a state
/// enum in the first place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoteState {
    /// Materialized, with a `batfiles.toml` that parsed.
    Present,
    /// Materialized, with no `batfiles.toml`: it declares nothing.
    NoManifest,
    /// Nothing usable at the root yet. `sync` materializes, `vars list`
    /// reports, `vars refresh` fails only when a key names it.
    NotMaterialized,
}

/// A declared remote that at least one `include-remote` selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IncludedRemote {
    /// The declared remote id — the `[remotes]` map key, and the dynamic
    /// cache's identity for this remote's captures.
    pub id: ItemId,
    /// From the leaf's `[remotes]` entry, not from the inclusion: including a
    /// repository does not by itself let it execute commands.
    pub allow_dynamic_vars: bool,
    /// The materialization. `root` is `<remotes_dir>/<id>` and is meaningful
    /// even when nothing is there — it is where `sync` will materialize — while
    /// `config` is the parsed manifest, or empty when `state` is not
    /// [`RemoteState::Present`].
    pub repo: Repository,
    pub state: RemoteState,
}

/// One `include-remote` action, as the variable machinery needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Inclusion {
    /// Index of the `include-remote` action in the leaf's action list. The
    /// stable identity of an inclusion with no `id`, and the way back to the
    /// action's selection fields, which stay on the action for `sync` to read.
    pub position: usize,
    pub id: Option<ItemId>,
    /// The declared remote this inclusion selects — a key of
    /// [`Leaf::included`].
    pub remote: ItemId,
    /// Per-inclusion variable overrides, cloned here because it is the one
    /// field a variable consumer needs.
    pub vars: BTreeMap<VarName, String>,
    /// Display-ready and unique within this leaf: the `id`, or
    /// `remote=<remote-id>` for the first id-less inclusion of a remote and
    /// `remote=<remote-id> #<n>` for its nth id-less occurrence thereafter.
    ///
    /// Computed once, at load time, so uniqueness is a property of the model
    /// rather than of whichever printer happens to render it.
    pub label: String,
}

/// The leaf repository and everything its inclusions reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Leaf {
    pub repo: Repository,
    /// One entry per declared remote that at least one inclusion selects, keyed
    /// by the remote's `[remotes]` map key — the cache identity. Cache identity
    /// is per remote and scope is per inclusion, so the model carries both, and
    /// "two inclusions of one remote share a single capture" is true by
    /// construction.
    ///
    /// The full `[remotes]` map needs no separate carrying: it is already
    /// `repo.config.remotes`. This is the subset that has anything on disk to
    /// load.
    pub included: BTreeMap<ItemId, IncludedRemote>,
    /// One entry per `include-remote` action, in action order.
    pub inclusions: Vec<Inclusion>,
}
