//! Dynamic variables: running their commands, and resolving them against the
//! cache.
//!
//! The two halves are deliberately separate. [`run`] classifies one command's
//! result and returns; it reads no clock, touches no cache, and prints nothing.
//! [`resolve`] owns everything that needs an opinion — freshness, the cache
//! policy, what a failure falls back on, and the warning that says so.
//!
//! `resolve` here is the *cache* resolver, not the precedence one: this module
//! answers "what does this declaration's command produce, and should it run at
//! all", and the pass that layers repository, remote, machine-local, and
//! command-line values on top of each other is a different verb in a different
//! module.

mod resolve;
mod run;

/// The two halves present flat, so a caller says `dynamic::capture` and
/// `dynamic::resolve` rather than naming the file each lives in.
pub(crate) use run::{Outcome, RunError, capture};

#[allow(
    unused_imports,
    reason = "no command dispatches to the cache resolver yet"
)]
pub(crate) use resolve::{
    Absence, CachePolicy, Declaration, Identity, Refresh, Resolution, Resolved, resolve,
};
