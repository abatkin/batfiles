//! The `batfiles` binary.

mod action;
mod app;
mod cli;
mod directory;
mod error;
mod install;
mod item;
mod location;
mod manifest;
mod mode;
mod output;
mod paths;
mod sync;
mod tomlfile;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
