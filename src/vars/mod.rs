//! User variables: their names, the machine's `vars.toml`, environment
//! overrides, dynamic variables, and the resolved scope that conditions read.

pub(crate) mod dynamic;
pub(crate) mod env_overrides;
pub(crate) mod machine;
pub(crate) mod name;
pub(crate) mod scope;
