//! The local files batfiles keeps outside the leaf repository.
//!
//! There are three (`docs/state.md`), one per file here: `vars.toml` and
//! `disabled.toml` are machine-local user configuration, and `dynamic-vars.toml`
//! is disposable cache data. Each is a whole document that batfiles rewrites
//! atomically, so each type loads and stores itself through the shared path in
//! [`crate::tomlfile`]. A type knows its own file *name*; the directory it sits
//! in is a resolved root, so callers pass the path
//! ([`Roots`](crate::config::Roots) pairs the two).
//!
//! A missing file means an empty document, which is why every type is `Default`.
//! A file that exists but does not parse is fatal and is left untouched, so
//! nothing here recovers from a malformed document.
#![allow(dead_code, reason = "no command reads or writes local state yet")]

mod disabled;
mod dynamic_vars;
mod vars;

#[allow(unused_imports, reason = "no command reads or writes local state yet")]
pub(crate) use {
    disabled::Disabled,
    dynamic_vars::{CachedVar, DynamicVarCache},
    vars::MachineVars,
};
