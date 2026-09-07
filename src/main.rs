//! The `batfiles` binary.

mod action;
mod app;
mod archive;
mod cli;
mod clone_list;
mod directory;
mod disabled;
mod env;
mod error;
mod execute;
mod fetch;
mod git;
mod install;
mod item;
mod location;
mod manifest;
mod mode;
mod output;
mod paths;
mod selection;
mod tomlfile;
mod var;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
