//! The `batfiles` binary.

mod app;
mod cli;
mod config;
mod error;
mod output;
mod tomlfile;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
