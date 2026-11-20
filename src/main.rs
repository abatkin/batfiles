//! The `batfiles` binary.

mod action;
mod app;
mod cli;
mod commands;
mod env;
mod error;
mod filesystem;
mod location;
mod manifest;
mod output;
mod release;
mod run;
mod source;
mod subprocess;
mod vars;
mod version;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
