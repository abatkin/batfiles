//! Parse arguments, resolve presentation, dispatch commands, and report errors.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::process::ExitCode;

use clap::{ColorChoice, CommandFactory, FromArgMatches};

use crate::cli::{Cli, Command, VarsCommand, color};
use crate::commands::{Session, apply, clone, disabled, init, sync, update, vars};
use crate::env::Environment;
use crate::error::Error;
use crate::manifest::disabled::Change;
use crate::manifest::item::ItemKind;
use crate::output::{Reporter, Verbosity};

/// A command that ran and failed.
const EXIT_FAILURE: u8 = 1;

pub(crate) fn run() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    let env = Environment::capture();

    let color = color::resolve(
        color::preparse_choice(&args),
        env.get("BATFILES_COLOR"),
        env.get("NO_COLOR"),
    );
    let mut reporter = Reporter::new(color.enabled(std::io::stderr().is_terminal()));
    if let Some(warning) = &color.warning {
        reporter.warn(warning);
    }

    let cli = match parse(&args, color.mode) {
        Ok(parsed) => parsed,
        Err(error) => {
            // Let clap choose stdout/status 0 for help and stderr/status 2 for usage errors.
            let _ = error.print();
            return ExitCode::from(exit_code(error.exit_code()));
        }
    };
    reporter.set_verbosity(Verbosity::new(cli.global.quiet, cli.global.verbose));

    let session = Session {
        global: &cli.global,
        env: &env,
        reporter: &reporter,
    };
    match dispatch(&cli.command, &session) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            reporter.error(&error.to_string());
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Run the parsed command.
fn dispatch(command: &Command, session: &Session<'_>) -> Result<(), Error> {
    match command {
        Command::Version => {
            print!("{}", Cli::command().render_version());
            Ok(())
        }
        Command::Update(args) => update::run(args, session),
        Command::Init(args) => init::run(args, session),
        Command::Clone(args) => clone::run(args, session),
        Command::Sync(args) => sync::run(args, session),
        Command::ApplyAction(args) => apply::action(args, session),
        Command::ApplyGroup(args) => apply::group(args, session),
        Command::DisableAction(args) => {
            disabled::run(&args.ids, ItemKind::Action, Change::Disable, session)
        }
        Command::EnableAction(args) => {
            disabled::run(&args.ids, ItemKind::Action, Change::Enable, session)
        }
        Command::DisableGroup(args) => {
            disabled::run(&args.groups, ItemKind::Group, Change::Disable, session)
        }
        Command::EnableGroup(args) => {
            disabled::run(&args.groups, ItemKind::Group, Change::Enable, session)
        }
        Command::Vars(VarsCommand::Set { key, value }) => vars::set(key, value, session),
        Command::Vars(VarsCommand::Get { key }) => vars::get(key, session),
        Command::Vars(VarsCommand::Unset { key }) => vars::unset(key, session),
        Command::Vars(VarsCommand::List {
            machine_only,
            no_refresh,
        }) => vars::list(*machine_only, *no_refresh, session),
        Command::Vars(VarsCommand::Refresh { keys }) => vars::refresh::run(keys, session),
    }
}

/// Parse without exiting the process, so the caller controls presentation.
fn parse(args: &[OsString], color: ColorChoice) -> Result<Cli, clap::Error> {
    let matches = Cli::command().color(color).try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}

/// Convert a clap exit status to `u8`, using `EXIT_FAILURE` if it is out of range.
fn exit_code(clap_code: i32) -> u8 {
    u8::try_from(clap_code).unwrap_or(EXIT_FAILURE)
}
