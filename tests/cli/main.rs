//! End-to-end tests of the built `batfiles` binary, grouped by behavior with shared fixtures in
//! `support`.

mod support;

mod actions;
mod apply;
mod bootstrap;
mod clone;
mod clone_list_addresses;
mod clone_lists;
mod cloning;
mod conditions;
mod decompressing;
mod disabled;
mod fetching;
mod groups;
mod inclusion;
mod inclusion_addresses;
mod inclusion_clone_lists;
mod inclusion_filters;
mod inclusion_vars;
mod init;
mod locations;
mod manifest;
mod refreshing;
mod remotes;
mod selection;
mod surface;
mod unzipping;
mod update;
mod vars;

#[cfg(unix)]
mod conflicts;
#[cfg(unix)]
mod dry_run;
#[cfg(unix)]
mod dynamic_vars;
#[cfg(unix)]
mod fetched_remotes;
// Symlink actions, which are Unix-only.
#[cfg(unix)]
mod inclusion_composition;
#[cfg(unix)]
mod linking;
#[cfg(unix)]
mod vars_refresh;
