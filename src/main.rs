//! The `batfiles` binary.

mod app;
mod cli;
mod config;
mod item;
mod output;
mod repo;
mod state;
mod tomlfile;
mod var;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
