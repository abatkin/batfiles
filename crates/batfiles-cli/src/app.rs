//! Wiring: parse arguments, resolve presentation, dispatch, and map the result
//! to an exit status.
//!
//! The command implementations themselves are not written yet; each one reports
//! that it is unimplemented rather than pretending to succeed.

use std::ffi::OsString;
use std::fmt;
use std::io::IsTerminal;
use std::process::ExitCode;

use clap::{ColorChoice, CommandFactory, FromArgMatches};

use crate::cli::{Cli, Command, GlobalOptions};
use crate::color::{self, ColorEnv};
use crate::output::{Reporter, Verbosity};
use crate::trace;

/// Exit status for a command that ran but failed. Usage errors exit with 2,
/// which clap chooses for the errors it renders.
const EXIT_FAILURE: u8 = 1;

pub fn run() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();

    // Presentation is settled first: clap may need to render `--help`,
    // `--version`, or a usage error before there is a parsed `Cli` to consult,
    // and that output should honor the requested color too.
    let color = color::resolve(color::preparse_choice(&args), &ColorEnv::from_process());
    let enabled = color.enabled(std::io::stdout().is_terminal());

    // Verbosity arrives with the parsed arguments; warnings print at every
    // level, so the reporter is usable before then.
    let mut reporter = Reporter::new(enabled, Verbosity::Normal);
    if let Some(warning) = &color.warning {
        reporter.warn(warning);
    }

    let cli = match parse(&args, color.mode) {
        Ok(cli) => cli,
        Err(error) => {
            // clap picks the stream and status: help and version on standard
            // output with 0, usage errors on standard error with 2.
            let _ = error.print();
            return exit_code(error.exit_code());
        }
    };

    reporter.set_verbosity(Verbosity::new(cli.global.quiet, cli.global.verbose));

    reporter.detail(1, &trace::summary(&cli.command));
    trace_locations(&reporter, &cli.global);

    match dispatch(&cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            reporter.error(&error.to_string());
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Parse without exiting the process, so the caller controls presentation.
fn parse(args: &[OsString], color: ColorChoice) -> Result<Cli, clap::Error> {
    let matches = Cli::command().color(color).try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}

fn exit_code(clap_code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(clap_code).unwrap_or(EXIT_FAILURE))
}

fn dispatch(command: &Command) -> Result<(), Error> {
    match command {
        // Rendered through clap so `version` and `--version` cannot drift.
        Command::Version => {
            print!("{}", Cli::command().render_version());
            Ok(())
        }
        other => Err(Error::Unimplemented(trace::name(other))),
    }
}

/// Report which locations were selected explicitly. Resolving the defaults is
/// the configuration layer's job, so nothing is resolved here.
fn trace_locations(reporter: &Reporter, global: &GlobalOptions) {
    for (name, path) in [
        ("--batfiles-dir", &global.batfiles_dir),
        ("--home-dir", &global.home_dir),
        ("--config-dir", &global.config_dir),
        ("--cache-dir", &global.cache_dir),
    ] {
        if let Some(path) = path {
            reporter.detail(2, &format!("{name} = {}", path.display()));
        }
    }
}

#[derive(Debug)]
enum Error {
    Unimplemented(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unimplemented(command) => {
                write!(f, "`{command}` is not implemented yet")
            }
        }
    }
}

impl std::error::Error for Error {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::InitArgs;

    #[test]
    fn version_is_the_only_implemented_command() {
        assert!(dispatch(&Command::Version).is_ok());
        assert!(dispatch(&Command::Init(InitArgs { no_git_init: false })).is_err());
    }

    #[test]
    fn the_unimplemented_message_names_the_command() {
        let error = dispatch(&Command::Init(InitArgs { no_git_init: false })).unwrap_err();
        assert_eq!(error.to_string(), "`init` is not implemented yet");
    }
}
