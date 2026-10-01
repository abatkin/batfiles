//! Parse arguments, resolve presentation and locations, dispatch commands, and report errors.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ColorChoice, CommandFactory, FromArgMatches};

use crate::action::DestinationOptions;
use crate::bootstrap::BootstrapDecisions;
use crate::cli::{Cli, Command, ExecutionOptions, GlobalOptions, VarsCommand, color};
use crate::clone;
use crate::disabled::{self, Change};
use crate::dynamic::{CachePolicy, refresh};
use crate::env::Environment;
use crate::error::Error;
use crate::execute::{self, Invocation};
use crate::init;
use crate::item::ItemKind;
use crate::location::{
    LocationInputs, Roots, StateRoots, detect_os_home, discover_working_repository, resolve_roots,
    resolve_state_roots,
};
use crate::machine_vars;
use crate::mode::RunMode;
use crate::output::{Reporter, Verbosity};
use crate::replace::ConflictPolicy;
use crate::var_set;

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

    match dispatch(&cli, &env, &reporter) {
        Ok(code) => code,
        Err(error) => {
            reporter.error(&error.to_string());
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Run the parsed command.
fn dispatch(cli: &Cli, env: &Environment, reporter: &Reporter) -> Result<ExitCode, Error> {
    match &cli.command {
        Command::Version => {
            print!("{}", Cli::command().render_version());
            Ok(ExitCode::SUCCESS)
        }
        Command::Init(args) => {
            init::run(args, env, reporter)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Clone(args) => {
            let roots = locate_for_new_repository(cli, env, reporter)?;
            clone::run(
                &invocation(&roots, false, &args.action, env, reporter),
                &args.url,
                &args.bootstrap,
                env,
                &args.selection.actions.skip_actions,
                &args.selection.groups.skip_groups,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Sync(args) => {
            // Validate bootstrap options before resolving anything.
            let bootstrap = if args.bootstrap {
                Some(BootstrapDecisions::read(
                    &args.bootstrap_options,
                    env,
                    reporter,
                )?)
            } else {
                None
            };
            let roots = locate_repository(cli, env, reporter)?;
            execute::sync(
                &invocation(&roots, args.dry_run, &args.action, env, reporter),
                &args.selection.actions.skip_actions,
                &args.selection.groups.skip_groups,
                args.refresh_remotes,
                bootstrap.as_ref(),
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Command::ApplyAction(args) => {
            let roots = locate_repository(cli, env, reporter)?;
            execute::apply_action(
                &invocation(&roots, args.dry_run, &args.action, env, reporter),
                &args.id,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Command::ApplyGroup(args) => {
            let roots = locate_repository(cli, env, reporter)?;
            execute::apply_group(
                &invocation(&roots, args.dry_run, &args.action, env, reporter),
                &args.group,
                &args.selection.skip_actions,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Command::DisableAction(args) => edit_disabled_list(
            cli,
            env,
            reporter,
            &args.ids,
            ItemKind::Action,
            Change::Disable,
        ),
        Command::EnableAction(args) => edit_disabled_list(
            cli,
            env,
            reporter,
            &args.ids,
            ItemKind::Action,
            Change::Enable,
        ),
        Command::DisableGroup(args) => edit_disabled_list(
            cli,
            env,
            reporter,
            &args.groups,
            ItemKind::Group,
            Change::Disable,
        ),
        Command::EnableGroup(args) => edit_disabled_list(
            cli,
            env,
            reporter,
            &args.groups,
            ItemKind::Group,
            Change::Enable,
        ),
        Command::Vars(VarsCommand::Set { key, value }) => {
            let roots = locate_state(cli, env, reporter)?;
            machine_vars::set(key, value, &roots, reporter)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Vars(VarsCommand::Get { key }) => {
            let roots = locate_state(cli, env, reporter)?;
            machine_vars::get(key, &roots, reporter)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Vars(VarsCommand::Unset { key }) => {
            let roots = locate_state(cli, env, reporter)?;
            machine_vars::unset(key, &roots, reporter)?;
            Ok(ExitCode::SUCCESS)
        }
        // `--machine-only` must work without a repository.
        Command::Vars(VarsCommand::List {
            machine_only,
            no_refresh,
        }) => {
            if *machine_only {
                let state = locate_state(cli, env, reporter)?;
                var_set::list_machine(&state, reporter)?;
            } else {
                let roots = locate_repository(cli, env, reporter)?;
                let policy = if *no_refresh {
                    CachePolicy::Never
                } else {
                    CachePolicy::Auto
                };
                var_set::list(&roots, env, policy, reporter)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Vars(VarsCommand::Refresh { keys }) => {
            let roots = locate_repository(cli, env, reporter)?;
            refresh::run(keys, &roots, env, reporter)?;
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// Build the shared execution inputs from resolved roots and command options.
fn invocation<'a>(
    roots: &'a Roots,
    dry_run: bool,
    action: &'a ExecutionOptions,
    env: &'a Environment,
    reporter: &'a Reporter,
) -> Invocation<'a> {
    Invocation {
        roots,
        mode: RunMode::new(dry_run),
        vars: &action.vars,
        refresh_vars: action.refresh_vars,
        destination_options: DestinationOptions {
            policy: if action.no_overwrite {
                ConflictPolicy::Skip
            } else if action.interactive {
                ConflictPolicy::Ask
            } else {
                ConflictPolicy::Backup
            },
            refresh_content: action.refresh_content,
        },
        env,
        reporter,
    }
}

/// Edit one of the machine-local disabled lists.
fn edit_disabled_list(
    cli: &Cli,
    env: &Environment,
    reporter: &Reporter,
    names: &[String],
    kind: ItemKind,
    change: Change,
) -> Result<ExitCode, Error> {
    let roots = locate_state(cli, env, reporter)?;
    disabled::run(names, kind, change, &roots, reporter)?;
    Ok(ExitCode::SUCCESS)
}

/// Resolve config and cache roots without repository discovery, and report them at `-v`.
fn locate_state(cli: &Cli, env: &Environment, reporter: &Reporter) -> Result<StateRoots, Error> {
    let state = resolve_state_roots(&location_inputs(&cli.global), env, detect_os_home)?;
    report_state_roots(reporter, &state);
    Ok(state)
}

/// Resolve every root, for a command that reads the leaf repository.
fn locate_repository(cli: &Cli, env: &Environment, reporter: &Reporter) -> Result<Roots, Error> {
    locate(cli, env, reporter, discover_working_repository)
}

/// Resolve roots for `clone`, without searching for an existing repository.
fn locate_for_new_repository(
    cli: &Cli,
    env: &Environment,
    reporter: &Reporter,
) -> Result<Roots, Error> {
    locate(cli, env, reporter, || Ok(None))
}

/// Resolve all roots using the supplied repository discovery function, and report them at `-v`.
fn locate(
    cli: &Cli,
    env: &Environment,
    reporter: &Reporter,
    working_repository: impl FnOnce() -> Result<Option<PathBuf>, Error>,
) -> Result<Roots, Error> {
    let roots = resolve_roots(
        &location_inputs(&cli.global),
        env,
        working_repository,
        detect_os_home,
    )?;
    reporter.detail(
        1,
        &format!("{:<12}{}", "repository:", roots.batfiles_repo.display()),
    );
    reporter.detail(1, &format!("{:<12}{}", "home:", roots.home.display()));
    report_state_roots(reporter, &roots.state);
    Ok(roots)
}

/// Extract the four location options used by root resolution.
fn location_inputs(global: &GlobalOptions) -> LocationInputs {
    LocationInputs {
        batfiles_dir: global.batfiles_dir.clone(),
        home_dir: global.home_dir.clone(),
        config_dir: global.config_dir.clone(),
        cache_dir: global.cache_dir.clone(),
    }
}

/// Report the config and cache paths at `-v`.
fn report_state_roots(reporter: &Reporter, state: &StateRoots) {
    for (label, path) in [("config:", &state.config_dir), ("cache:", &state.cache_dir)] {
        reporter.detail(1, &format!("{label:<12}{}", path.display()));
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
