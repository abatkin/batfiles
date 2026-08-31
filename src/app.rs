//! Wiring: capture the environment, resolve presentation, parse arguments,
//! resolve the roots a command works in, dispatch, and map the result to an
//! exit status.
//!
//! Every command in the surface parses; `version`, `sync`, the two apply
//! commands, and the four enable/disable commands run. The rest report an
//! option they accept and do not honor yet, or else resolve their roots and
//! report that they are not implemented yet, which is the honest thing to do
//! and the reason the whole surface can be committed before the tool works.

use std::ffi::OsString;
use std::io::IsTerminal;
use std::process::ExitCode;

use clap::{ArgMatches, ColorChoice, CommandFactory, FromArgMatches};

use crate::cli::unsupported::{self, Unsupported};
use crate::cli::{Cli, Command, GlobalOptions, color};
use crate::disabled::{self, Change, DisabledList};
use crate::env::Environment;
use crate::error::Error;
use crate::execute;
use crate::location::{LocationInputs, Roots, detect_os_home, resolve_roots};
use crate::mode::RunMode;
use crate::output::{Reporter, Verbosity};

/// A command that ran and failed.
const EXIT_FAILURE: u8 = 1;

/// A command that did not run at all. This is clap's status for a usage error,
/// and asking for a command batfiles cannot perform yet is the same kind of
/// mistake: the invocation was wrong, and nothing happened.
const EXIT_UNIMPLEMENTED: u8 = 2;

pub(crate) fn run() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();
    // Read once, so every later lookup sees the same environment.
    let env = Environment::capture();

    // Presentation is settled first: clap may need to render `--help`,
    // `--version`, or a usage error before there is a parsed `Cli` to consult,
    // and that output should honor the requested color too.
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
    // Ahead of every command and of root resolution: an unsupported option
    // means nothing was attempted, and resolving first would let a missing-home
    // failure preempt it on the machines least able to explain why.
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
        Command::Init(_) => Ok(unimplemented(reporter, name)),
        Command::Sync(args) => {
            let roots = locate(cli, env, reporter)?;
            execute::sync(
                &roots,
                RunMode::new(args.dry_run),
                &args.selection.actions.skip_actions,
                &args.selection.groups.skip_groups,
                env,
                reporter,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        // The two apply commands run the same loop over the same list as
        // `sync`, restricted to what they name. `apply-action` reads no
        // environment because it honors neither run-only skip list: naming one
        // action waives every exclusion, which is why it accepts neither option
        // either.
        Command::ApplyAction(args) => {
            let roots = locate(cli, env, reporter)?;
            execute::apply_action(&roots, RunMode::new(args.dry_run), &args.id, reporter)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::ApplyGroup(args) => {
            let roots = locate(cli, env, reporter)?;
            execute::apply_group(
                &roots,
                RunMode::new(args.dry_run),
                &args.group,
                &args.selection.skip_actions,
                env,
                reporter,
            )?;
            Ok(ExitCode::SUCCESS)
        }
        // The four differ only in which list they edit and which way they move a
        // name, so they share one implementation and are told apart here rather
        // than inside it.
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
        _ => {
            locate(cli, env, reporter)?;
            Ok(unimplemented(reporter, name))
        }
    }
}

/// Edit one of the machine-local disabled lists.
///
/// The roots are resolved as for any other command, though only the config one
/// is read: these commands never open the leaf repository, so a manifest that is
/// missing or malformed cannot fail an enable or a disable.
fn edit_disabled_list(
    cli: &Cli,
    env: &Environment,
    reporter: &Reporter,
    names: &[String],
    list: DisabledList,
    change: Change,
) -> Result<ExitCode, Error> {
    let roots = locate(cli, env, reporter)?;
    disabled::run(names, list, change, &roots, reporter)?;
    Ok(ExitCode::SUCCESS)
}

/// Resolve the roots a command works in, and report them at `-v`.
fn locate(cli: &Cli, env: &Environment, reporter: &Reporter) -> Result<Roots, Error> {
    let roots = resolve_roots(&locations(&cli.global), env, detect_os_home)?;
    report_roots(reporter, &roots);
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

/// Report where a command decided to work. Four inputs with four fallbacks
/// apiece are hard to reason about from the outside, so `-v` shows the answer
/// rather than leaving it to be inferred.
fn report_roots(reporter: &Reporter, roots: &Roots) {
    for (label, path) in [
        ("repository:", &roots.batfiles_dir),
        ("home:", &roots.home),
        ("config:", &roots.config_dir),
        ("cache:", &roots.cache_dir),
    ] {
        reporter.detail(1, &format!("{label:<12}{}", path.display()));
    }
}

/// A command that parsed but does not exist yet.
fn unimplemented(reporter: &Reporter, name: &str) -> ExitCode {
    reporter.error(&format!("`{name}` is not implemented yet"));
    ExitCode::from(EXIT_UNIMPLEMENTED)
}

/// An option that parsed but does nothing yet (`guidance.md`, rule 12).
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
///
/// Read back from the parse rather than matched over `Command`, so adding a
/// command does not require remembering to name it here.
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
