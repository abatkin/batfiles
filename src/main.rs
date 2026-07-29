//! The `batfiles` binary.

mod app;
mod cli;
mod config;
mod output;
mod var;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
