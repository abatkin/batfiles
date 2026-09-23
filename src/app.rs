//! Parse arguments, resolve presentation and locations, dispatch commands, and report errors.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{ArgMatches, ColorChoice, CommandFactory, FromArgMatches};

use crate::cli::unsupported::{self, Unsupported};
use crate::cli::{Cli, Command, GlobalOptions, VarsCommand, color};
use crate::clone;
use crate::disabled::{self, Change, DisabledList};
use crate::env::Environment;
use crate::error::Error;
use crate::execute::{self, Invocation};
use crate::init;
use crate::location::{
    LocationInputs, Roots, StateRoots, detect_os_home, discover_working_repository, resolve_roots,
    resolve_state_roots,
};
use crate::machine_vars;
use crate::mode::RunMode;
use crate::output::{Reporter, Verbosity};
use crate::var::VarName;
use crate::var_set;

/// A command that ran and failed.
const EXIT_FAILURE: u8 = 1;

/// A command that did not run: clap's usage-error status, also used for a
/// command or option batfiles does not implement yet.
const EXIT_UNIMPLEMENTED: u8 = 2;

pub(crate) fn run() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    // Read once, so every later lookup sees the same environment.
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

    let (cli, name) = match parse(&args, color.mode) {
        Ok(parsed) => parsed,
        Err(error) => {
            // clap picks the stream and status: help and version on standard
            // output with 0, usage errors on standard error with 2.
            let _ = error.print();
            return ExitCode::from(exit_code(error.exit_code()));
        }
    };
    reporter.set_verbosity(Verbosity::new(cli.global.quiet, cli.global.verbose));

    match dispatch(&cli, &name, &env, &reporter) {
        Ok(code) => code,
        // Reported in one place, so every failure gets one label and one status
        // no matter which command raised it.
        Err(error) => {
            reporter.error(&error.to_string());
            ExitCode::from(EXIT_FAILURE)
        }
    }
}

/// Run the parsed command.
fn dispatch(
    cli: &Cli,
    name: &str,
    env: &Environment,
    reporter: &Reporter,
) -> Result<ExitCode, Error> {
    if let Some(found) = unsupported::first(&cli.command) {
        return Ok(not_yet(reporter, &found));
    }

    match &cli.command {
        // Rendered through clap so `version` and `--version` cannot drift.
        Command::Version => {
            print!("{}", Cli::command().render_version());
            Ok(ExitCode::SUCCESS)
        }
        // `init` works on the current directory, so it resolves no roots either.
        Command::Init(args) => {
            init::run(args, reporter)?;
            Ok(ExitCode::SUCCESS)
        }
        // The one command that creates the leaf repository rather than reading
        // one, which is why it resolves its roots differently. It accepts no
        // `--dry-run` either, so the run it hands on always writes.
        Command::Clone(args) => {
            let roots = locate_destination(cli, env, reporter)?;
            clone::run(
                &invocation(&roots, false, &args.action.vars, env, reporter),
                &args.url,
                &args.bootstrap,
                env,
                &args.selection.actions.skip_actions,
                &args.selection.groups.skip_groups,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Sync(args) => {
            let roots = locate_repository(cli, env, reporter)?;
            execute::sync(
                &invocation(&roots, args.dry_run, &args.action.vars, env, reporter),
                &args.selection.actions.skip_actions,
                &args.selection.groups.skip_groups,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Command::ApplyAction(args) => {
            let roots = locate_repository(cli, env, reporter)?;
            execute::apply_action(
                &invocation(&roots, args.dry_run, &args.action.vars, env, reporter),
                &args.id,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        Command::ApplyGroup(args) => {
            let roots = locate_repository(cli, env, reporter)?;
            execute::apply_group(
                &invocation(&roots, args.dry_run, &args.action.vars, env, reporter),
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
            DisabledList::Actions,
            Change::Disable,
        ),
        Command::EnableAction(args) => edit_disabled_list(
            cli,
            env,
            reporter,
            &args.ids,
            DisabledList::Actions,
            Change::Enable,
        ),
        Command::DisableGroup(args) => edit_disabled_list(
            cli,
            env,
            reporter,
            &args.groups,
            DisabledList::Groups,
            Change::Disable,
        ),
        Command::EnableGroup(args) => edit_disabled_list(
            cli,
            env,
            reporter,
            &args.groups,
            DisabledList::Groups,
            Change::Enable,
        ),
        // The three machine-local variable commands read and write `vars.toml`
        // and nothing else, so they resolve roots without discovering a
        // repository.
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
        // The only `vars` command that reads more than `vars.toml`, and the only
        // command whose roots depend on an option: `--machine-only` answers from
        // machine-local state alone, so it resolves no repository to read.
        Command::Vars(VarsCommand::List { machine_only, .. }) => {
            if *machine_only {
                let state = locate_state(cli, env, reporter)?;
                var_set::list_machine(&state, reporter)?;
            } else {
                let roots = locate_repository(cli, env, reporter)?;
                var_set::list(&roots, env, reporter)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Vars(VarsCommand::Refresh { .. }) => {
            locate_state(cli, env, reporter)?;
            Ok(unimplemented(reporter, name))
        }
    }
}

/// What the three action-executing commands share: where they run, whether
/// they write, and the values the invocation itself supplied.
fn invocation<'a>(
    roots: &'a Roots,
    dry_run: bool,
    vars: &'a [(VarName, String)],
    env: &'a Environment,
    reporter: &'a Reporter,
) -> Invocation<'a> {
    Invocation {
        roots,
        mode: RunMode::new(dry_run),
        vars,
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
    list: DisabledList,
    change: Change,
) -> Result<ExitCode, Error> {
    let roots = locate_state(cli, env, reporter)?;
    disabled::run(names, list, change, &roots, reporter)?;
    Ok(ExitCode::SUCCESS)
}

/// Resolve the state roots a command works in, and report them at `-v`.
///
/// A command routed here installs nothing and reads no repository, so no
/// working-directory discovery runs and it is never handed a repository nothing
/// selected. [`locate_repository`] resolves every root instead.
fn locate_state(cli: &Cli, env: &Environment, reporter: &Reporter) -> Result<StateRoots, Error> {
    let state = resolve_state_roots(&locations(&cli.global), env, detect_os_home)?;
    report_state_roots(reporter, &state);
    Ok(state)
}

/// Resolve every root, for a command that reads the leaf repository.
fn locate_repository(cli: &Cli, env: &Environment, reporter: &Reporter) -> Result<Roots, Error> {
    locate(cli, env, reporter, discover_working_repository)
}

/// The same, for the one command that creates the leaf repository instead of
/// reading one.
///
/// Skips working-directory discovery, which selects a directory already holding
/// a manifest: exactly the destination `clone` refuses.
fn locate_destination(cli: &Cli, env: &Environment, reporter: &Reporter) -> Result<Roots, Error> {
    locate(cli, env, reporter, || Ok(None))
}

/// Resolve every root and report them at `-v`, given how the repository is
/// selected when nothing named one.
fn locate(
    cli: &Cli,
    env: &Environment,
    reporter: &Reporter,
    working_repository: impl FnOnce() -> Result<Option<PathBuf>, Error>,
) -> Result<Roots, Error> {
    let roots = resolve_roots(
        &locations(&cli.global),
        env,
        working_repository,
        detect_os_home,
    )?;
    reporter.detail(
        1,
        &format!("{:<12}{}", "repository:", roots.batfiles_dir.display()),
    );
    reporter.detail(1, &format!("{:<12}{}", "home:", roots.home.display()));
    report_state_roots(reporter, &roots.state);
    Ok(roots)
}

/// The four location options, separated from the rest of the global options so
/// that root resolution takes only what it resolves.
fn locations(global: &GlobalOptions) -> LocationInputs {
    LocationInputs {
        batfiles_dir: global.batfiles_dir.clone(),
        home_dir: global.home_dir.clone(),
        config_dir: global.config_dir.clone(),
        cache_dir: global.cache_dir.clone(),
    }
}

/// Report where a command decided to keep its own state. The location
/// precedence is hard to reason about from the outside, so `-v` shows the
/// answer rather than leaving it to be inferred.
///
/// Commands that resolve state roots report these two paths. Repository
/// commands report their repository and home first. Commands that resolve no
/// roots, such as `version`, do not call this helper.
fn report_state_roots(reporter: &Reporter, state: &StateRoots) {
    for (label, path) in [("config:", &state.config_dir), ("cache:", &state.cache_dir)] {
        reporter.detail(1, &format!("{label:<12}{}", path.display()));
    }
}

/// A command that parsed but does not exist yet.
fn unimplemented(reporter: &Reporter, name: &str) -> ExitCode {
    reporter.error(&format!("`{name}` is not implemented yet"));
    ExitCode::from(EXIT_UNIMPLEMENTED)
}

/// An option that parsed but does nothing yet (`architecture.md`, rule 12).
fn not_yet(reporter: &Reporter, found: &Unsupported) -> ExitCode {
    reporter.error(&format!(
        "`{}` is not implemented yet; it arrives at step {}",
        found.option, found.step
    ));
    ExitCode::from(EXIT_UNIMPLEMENTED)
}

/// Parse without exiting the process, so the caller controls presentation, and
/// return the command's name alongside it.
fn parse(args: &[OsString], color: ColorChoice) -> Result<(Cli, String), clap::Error> {
    let matches = Cli::command().color(color).try_get_matches_from(args)?;
    let name = command_name(&matches);
    Cli::from_arg_matches(&matches).map(|cli| (cli, name))
}

/// The command as the user spelled it: `sync`, or `vars set`.
fn command_name(matches: &ArgMatches) -> String {
    let mut names = Vec::new();
    let mut current = matches;
    while let Some((name, sub)) = current.subcommand() {
        names.push(name);
        current = sub;
    }
    names.join(" ")
}

/// clap's statuses are 0 and 2 today, so the fallback is unreachable in
/// practice; a status batfiles cannot represent is still a failure.
fn exit_code(clap_code: i32) -> u8 {
    u8::try_from(clap_code).unwrap_or(EXIT_FAILURE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_of(args: &[&str]) -> String {
        let matches = Cli::command()
            .try_get_matches_from(args)
            .expect("expected the arguments to parse");
        command_name(&matches)
    }

    #[test]
    fn a_command_is_named_as_it_was_spelled() {
        assert_eq!(name_of(&["batfiles", "sync"]), "sync");
        assert_eq!(
            name_of(&["batfiles", "disable-action", "vim"]),
            "disable-action"
        );
    }

    #[test]
    fn a_subcommand_is_named_by_its_whole_path() {
        assert_eq!(name_of(&["batfiles", "vars", "get", "profile"]), "vars get");
    }
}
