//! End-to-end tests of the built `batfiles` binary, grouped by behavior with shared fixtures in
//! `support`.

mod support;

mod actions;
mod apply;
mod bootstrap;
mod clone;
mod clone_lists;
mod cloning;
mod conditions;
mod disabled;
mod fetching;
mod groups;
mod inclusion;
mod inclusion_addresses;
mod inclusion_clone_lists;
mod inclusion_composition;
mod inclusion_filters;
mod inclusion_vars;
mod init;
mod locations;
mod manifest;
mod refreshing;
mod remotes;
mod selection;
mod surface;
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
#[cfg(unix)]
mod linking;
#[cfg(unix)]
mod vars_refresh;
