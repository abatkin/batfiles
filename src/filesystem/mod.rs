//! Filesystem primitives: what is at a path, and the write helpers that create,
//! replace, and stage nodes, each consulting the run mode before it writes.

pub(crate) mod directory;
pub(crate) mod install;
pub(crate) mod mode;
pub(crate) mod paths;
pub(crate) mod replace;
pub(crate) mod tomlfile;
