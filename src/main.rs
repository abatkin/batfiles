//! The `batfiles` binary.

mod app;
mod cli;
mod config;
mod error;
mod output;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
