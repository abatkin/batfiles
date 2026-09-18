//! End-to-end checks of the built `batfiles` binary.
//!
//! One test target, in the pieces it divides into. `support` holds what the
//! others are written against; the rest are grouped by what they exercise.

mod support;

mod actions;
mod apply;
mod clone_lists;
mod cloning;
mod conditions;
mod disabled;
mod fetching;
mod groups;
mod inclusion;
mod inclusion_addresses;
mod inclusion_composition;
mod inclusion_filters;
mod inclusion_vars;
mod locations;
mod manifest;
mod remotes;
mod selection;
mod surface;
mod vars;

#[cfg(unix)]
mod dry_run;
#[cfg(unix)]
mod linking;
