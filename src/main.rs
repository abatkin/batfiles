//! The `batfiles` binary.

mod app;
mod cli;
mod condition;
mod config;
mod dynamic;
mod init;
mod item;
mod output;
mod repo;
mod scope;
mod state;
mod toggle;
mod tomlfile;
mod var;
mod vars;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
