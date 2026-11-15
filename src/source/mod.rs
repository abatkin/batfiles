//! Where content comes from: downloads, archives, Git, and the remotes a
//! manifest declares, materialized from those sources.

pub(crate) mod archive;
pub(crate) mod fetch;
pub(crate) mod git;
pub(crate) mod remotes;
