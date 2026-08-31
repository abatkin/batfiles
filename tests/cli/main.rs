//! End-to-end checks of the built `batfiles` binary.
//!
//! One test target, in the pieces it divides into. `support` holds what the
//! others are written against; the rest are grouped by what they exercise.

mod support;

mod actions;
mod apply;
mod disabled;
mod groups;
mod locations;
mod manifest;
mod selection;
mod surface;

#[cfg(unix)]
mod dry_run;
#[cfg(unix)]
mod linking;
