//! The local files batfiles keeps outside the leaf repository.
//!
//! There are three, one per file here: `vars.toml` and `disabled.toml` are
//! machine-local user configuration, and `dynamic-vars.toml` is disposable
//! cache data. Each is a whole document that batfiles rewrites atomically, so
//! each type loads and stores itself through the shared path in
//! [`crate::tomlfile`]. A type knows its own file *name*; the directory it sits
//! in is a resolved root, so callers pass the path
//! ([`Roots`](crate::config::Roots) pairs the two).
//!
//! A missing file means an empty document, which is why every type is `Default`.
//! A file that exists but does not parse is fatal and is left untouched, so
//! nothing here recovers from a malformed document.

// `disabled.toml` and `vars.toml` are the documents commands read and write so
// far, so they are the modules held to the usual dead-code rule.
mod disabled;
#[allow(
    dead_code,
    reason = "no command reads or writes the dynamic-variable cache yet"
)]
mod dynamic_vars;
mod vars;

pub(crate) use {disabled::Disabled, dynamic_vars::DynamicVarCache, vars::MachineVars};

#[allow(
    unused_imports,
    reason = "no command reads the dynamic-variable cache's entries yet"
)]
pub(crate) use dynamic_vars::CachedVar;
