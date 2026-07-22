//! The `batfiles` binary. Argument parsing and output formatting live here; all
//! real work belongs in `batfiles-core` behind `batfiles-config`.

mod app;
mod cli;
mod color;
mod output;
mod trace;

use std::process::ExitCode;

fn main() -> ExitCode {
    app::run()
}
