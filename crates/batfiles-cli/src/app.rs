//! Wiring: parse arguments, resolve presentation, dispatch, and map the result
//! to an exit status.
//!
//! The command implementations themselves are not written yet; each one reports
//! that it is unimplemented rather than pretending to succeed.

use std::ffi::OsString;
use std::fmt;
use std::io::IsTerminal;
use std::process::ExitCode;

use batfiles_config::{CliInputs, Environment, LocationInputs, Roots};
use clap::{ColorChoice, CommandFactory, FromArgMatches};

use crate::cli::{Cli, Command};
use crate::color;
use crate::output::{Reporter, Verbosity};
use crate::trace;

/// Exit status for a command that ran but failed. Usage errors exit with 2,
/// which clap chooses for the errors it renders.
const EXIT_FAILURE: u8 = 1;

pub fn run() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();

    // The process environment is captured once, before anything interprets it:
    // the CLI decodes, and the configuration layer parses. Color is the sole
    // exception, resolved here because it is presentation-only.
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
    // `version` resolve nothing. The merge of variables, skips, and bootstrap
    // adoption is not wired yet, so only the roots come back for now.
    if needs_roots(&cli.command) {
        match batfiles_config::assemble(&build_inputs(&cli), &env) {
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

/// Copy the parsed clap values into the config-owned [`CliInputs`].
///
/// Config cannot depend on clap, so the translation lives here. Locations are
/// global; the variable, skip, and bootstrap lists are command-specific and
/// stay empty for commands that do not accept them.
fn build_inputs(cli: &Cli) -> CliInputs {
    let mut inputs = CliInputs {
        locations: LocationInputs {
            batfiles_dir: cli.global.batfiles_dir.clone(),
            home_dir: cli.global.home_dir.clone(),
            config_dir: cli.global.config_dir.clone(),
            cache_dir: cli.global.cache_dir.clone(),
        },
        ..CliInputs::default()
    };

    match &cli.command {
        Command::Sync(args) => {
            inputs.vars = args.action.vars.clone();
            inputs.skip_actions = args.selection.skip_actions.clone();
            inputs.skip_groups = args.selection.skip_groups.clone();
        }
        Command::Clone(args) => {
            inputs.vars = args.action.vars.clone();
            inputs.skip_actions = args.selection.skip_actions.clone();
            inputs.skip_groups = args.selection.skip_groups.clone();
            inputs.enable_actions = args.bootstrap.enable_actions.clone();
            inputs.disable_actions = args.bootstrap.disable_actions.clone();
            inputs.enable_groups = args.bootstrap.enable_groups.clone();
            inputs.disable_groups = args.bootstrap.disable_groups.clone();
        }
        Command::ApplyAction(args) => inputs.vars = args.action.vars.clone(),
        Command::ApplyGroup(args) => inputs.vars = args.action.vars.clone(),
        _ => {}
    }

    inputs
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

/// Report the resolved roots at high verbosity, once the configuration layer
/// has applied the option/environment/default precedence.
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
    fn build_inputs_copies_the_global_locations() {
        let cli = parse(&[
            "batfiles",
            "--home-dir",
            "/h",
            "--config-dir",
            "/c",
            "version",
        ]);
        let inputs = build_inputs(&cli);
        assert_eq!(inputs.locations.home_dir, Some(PathBuf::from("/h")));
        assert_eq!(inputs.locations.config_dir, Some(PathBuf::from("/c")));
        // A command without action/selection/bootstrap options contributes no
        // lists.
        assert!(inputs.vars.is_empty());
        assert!(inputs.skip_actions.is_empty());
    }

    #[test]
    fn build_inputs_copies_the_command_specific_lists() {
        let cli = parse(&[
            "batfiles",
            "clone",
            "url",
            "--var",
            "profile=work",
            "--skip-action",
            "a",
            "--disable-group",
            "gui",
            "--enable-action",
            "shell",
        ]);
        let inputs = build_inputs(&cli);
        assert_eq!(inputs.vars, vec![("profile".to_owned(), "work".to_owned())]);
        assert_eq!(inputs.skip_actions, vec!["a".to_owned()]);
        assert_eq!(inputs.disable_groups, vec!["gui".to_owned()]);
        assert_eq!(inputs.enable_actions, vec!["shell".to_owned()]);
    }
}
