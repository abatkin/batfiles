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

use crate::cli::{Cli, Command, GlobalOptions, color, trace};
use crate::config::{Environment, LocationInputs, Roots, detect_os_home, resolve_roots};
use crate::output::{Reporter, Verbosity};

/// Exit status for a command that ran but failed. Usage errors exit with 2,
/// which clap chooses for the errors it renders.
const EXIT_FAILURE: u8 = 1;

pub(crate) fn run() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();

    // The process environment is captured once, before anything interprets it.
    let env = Environment::capture();

    // Presentation is settled first: clap may need to render `--help`,
    // `--version`, or a usage error before there is a parsed `Cli` to consult,
    // and that output should honor the requested color too.
    let color = color::resolve(
        color::preparse_choice(&args),
        env.get("BATFILES_COLOR"),
        env.get("NO_COLOR"),
    );
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

    // Resolve the roots only for the commands that need them; `init` and
    // `version` resolve nothing.
    if needs_roots(&cli.command) {
        match resolve_roots(&locations(&cli.global), &env, detect_os_home) {
            Ok(roots) => trace_roots(&reporter, &roots),
            Err(error) => {
                reporter.error(&error.to_string());
                return ExitCode::from(EXIT_FAILURE);
            }
        }
    }

    match dispatch(&cli.command) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            reporter.error(&error.to_string());
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Whether a command needs its location roots resolved. `init` and `version`
/// act without any resolved root; everything else reads or writes at least one.
fn needs_roots(command: &Command) -> bool {
    !matches!(command, Command::Init(_) | Command::Version)
}

/// The four location options, taken from the globals every command accepts.
fn locations(global: &GlobalOptions) -> LocationInputs {
    LocationInputs {
        batfiles_dir: global.batfiles_dir.clone(),
        home_dir: global.home_dir.clone(),
        config_dir: global.config_dir.clone(),
        cache_dir: global.cache_dir.clone(),
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

/// Report the resolved roots at high verbosity, once the option/environment/
/// default precedence has been applied.
fn trace_roots(reporter: &Reporter, roots: &Roots) {
    for (name, path) in [
        ("batfiles-dir", &roots.batfiles_dir),
        ("home", &roots.home),
        ("config-dir", &roots.config_dir),
        ("cache-dir", &roots.cache_dir),
    ] {
        reporter.detail(2, &format!("{name} = {}", path.display()));
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
    use crate::cli::testing::parse;
    use std::path::PathBuf;

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

    #[test]
    fn only_init_and_version_skip_root_resolution() {
        assert!(needs_roots(&parse(&["batfiles", "sync"]).command));
        assert!(!needs_roots(&parse(&["batfiles", "init"]).command));
        assert!(!needs_roots(&parse(&["batfiles", "version"]).command));
    }

    #[test]
    fn the_location_options_reach_root_resolution() {
        let cli = parse(&[
            "batfiles",
            "--home-dir",
            "/h",
            "--config-dir",
            "/c",
            "version",
        ]);
        let locations = locations(&cli.global);
        assert_eq!(locations.home_dir, Some(PathBuf::from("/h")));
        assert_eq!(locations.config_dir, Some(PathBuf::from("/c")));
        assert_eq!(locations.batfiles_dir, None);
        assert_eq!(locations.cache_dir, None);
    }
}
