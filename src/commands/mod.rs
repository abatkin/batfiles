//! The commands that stand outside the run pipeline: `clone`'s checkout,
//! `init`, `update`, editing `disabled.toml`, and the `vars` commands. `sync`
//! and the apply commands are entry points of [`crate::run`].

pub(crate) mod clone;
pub(crate) mod disabled;
pub(crate) mod init;
pub(crate) mod update;
pub(crate) mod vars;
