//! The `batfiles` binary.

mod app;
mod cli;
mod config;
mod init;
mod item;
mod output;
mod repo;
mod state;
mod toggle;
mod tomlfile;
mod var;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
