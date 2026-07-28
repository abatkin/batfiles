//! Reusable, non-configuration side effects for batfiles.
//!
//! This crate holds the I/O mechanisms that are not tied to batfiles'
//! configuration documents: Git materialization, URL fetch, archive extraction,
//! subprocess execution, and the clock. Each mechanism is a plain reusable
//! primitive; thin adapters on top of those primitives implement the capability
//! interfaces that [`batfiles_core`] defines and injects.
//!
//! Nothing here decides policy. Refresh policy, dry-run policy, freshness,
//! capture interpretation, and cache identity belong to the domain core, which
//! reaches outward only through its own capability traits.
//!
//! Reading and writing `batfiles.toml`, the state files, and `git-clone-list`
//! manifests belongs to `batfiles-config` instead: those are configuration
//! documents, not general-purpose I/O.
