//! The `batfiles` binary.

mod action;
mod app;
mod archive;
mod bootstrap;
mod cli;
mod clone;
mod clone_list;
mod condition;
mod directory;
mod disabled;
mod dynamic;
mod env;
mod env_vars;
mod error;
mod execute;
mod fetch;
mod git;
mod inclusion;
mod init;
mod install;
mod item;
mod location;
mod machine_vars;
mod manifest;
mod mode;
mod output;
mod paths;
mod remotes;
mod repo_path;
mod selection;
mod tomlfile;
mod var;
mod var_set;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
