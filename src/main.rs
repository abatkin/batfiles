//! The `batfiles` binary.

mod action;
mod app;
mod cli;
mod directory;
mod disabled;
mod env;
mod error;
mod execute;
mod install;
mod item;
mod location;
mod manifest;
mod mode;
mod output;
mod paths;
mod selection;
mod tomlfile;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
