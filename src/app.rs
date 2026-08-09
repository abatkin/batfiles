//! Wiring: parse arguments, resolve presentation, dispatch, and map the result
//! to an exit status.
//!
//! The command implementations live in their own modules. The ones not written
//! yet report that they are unimplemented rather than pretending to succeed.

use std::ffi::OsString;
use std::fmt;
use std::io::IsTerminal;
use std::process::ExitCode;

use clap::{ColorChoice, CommandFactory, FromArgMatches};

use crate::cli::{Cli, Command, GlobalOptions, color, trace};
use crate::config::{Environment, LocationInputs, Roots, detect_os_home, resolve_roots};
use crate::output::{Reporter, Verbosity};
use crate::toggle::{self, Direction, List};

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
    let roots = if needs_roots(&cli.command) {
        match resolve_roots(&locations(&cli.global), &env, detect_os_home) {
            Ok(roots) => {
                trace_roots(&reporter, &roots);
                Some(roots)
            }
            Err(error) => {
                reporter.error(&error.to_string());
                return ExitCode::from(EXIT_FAILURE);
            }
        }
    } else {
        None
    };

    match dispatch(&cli.command, roots.as_ref(), &reporter) {
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

fn dispatch(command: &Command, roots: Option<&Roots>, reporter: &Reporter) -> Result<(), Error> {
    // The four persistent enable/disable commands are one implementation
    // differing only in which list they edit and which way they move an address.
    // `needs_roots` decides who is handed roots, so a command reading them here
    // is one it already answered `true` for.
    let toggle = |addresses: &[String], list, direction| {
        let roots = roots.expect("a command that needs its roots is handed them");
        toggle::run(addresses, list, direction, roots, reporter).map_err(Error::Toggle)
    };

    match command {
        // Rendered through clap so `version` and `--version` cannot drift.
        Command::Version => {
            print!("{}", Cli::command().render_version());
            Ok(())
        }
        Command::DisableAction(args) => toggle(&args.ids, List::Actions, Direction::Disable),
        Command::EnableAction(args) => toggle(&args.ids, List::Actions, Direction::Enable),
        Command::DisableGroup(args) => toggle(&args.groups, List::Groups, Direction::Disable),
        Command::EnableGroup(args) => toggle(&args.groups, List::Groups, Direction::Enable),
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
    Toggle(toggle::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unimplemented(command) => {
                write!(f, "`{command}` is not implemented yet")
            }
            // A command's own diagnostic already says what failed, so this adds
            // nothing to it.
            Self::Toggle(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unimplemented(_) => None,
            Self::Toggle(error) => Some(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::InitArgs;
    use crate::cli::testing::parse;
    use std::path::PathBuf;

    /// A reporter that prints nothing, so the dispatch tests stay silent.
    fn silent() -> Reporter {
        Reporter::new(false, Verbosity::Quiet)
    }

    #[test]
    fn a_command_without_an_implementation_says_so() {
        let error = dispatch(
            &Command::Init(InitArgs { no_git_init: false }),
            None,
            &silent(),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "`init` is not implemented yet");
    }

    #[test]
    fn version_needs_no_roots() {
        assert!(dispatch(&Command::Version, None, &silent()).is_ok());
    }

    #[test]
    fn a_toggle_reports_its_own_failure() {
        // The dispatch layer adds no wrapper of its own around a command's
        // diagnostic; the address the user wrote is what they need to see.
        let dir = tempfile::tempdir().expect("temp dir");
        let roots = Roots {
            home: dir.path().to_path_buf(),
            batfiles_dir: dir.path().join("dotfiles"),
            config_dir: dir.path().to_path_buf(),
            cache_dir: dir.path().to_path_buf(),
        };
        let command = parse(&["batfiles", "disable-action", "a..b"]).command;
        let error = dispatch(&command, Some(&roots), &silent()).unwrap_err();
        assert!(error.to_string().contains("`a..b`"), "{error}");
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
