//! The `batfiles` binary.

mod app;
mod cli;
mod config;
mod error;
mod item;
mod output;
mod repo;
mod sync;
mod tomlfile;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
