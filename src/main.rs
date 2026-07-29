//! The `batfiles` binary.

mod app;
mod cli;
mod color;
mod config;
mod output;
mod trace;
mod var;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
