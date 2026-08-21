//! The `batfiles` binary.

mod app;
mod cli;
mod error;
mod item;
mod location;
mod manifest;
mod output;
mod sync;
mod tomlfile;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
