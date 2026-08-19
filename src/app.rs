//! Wiring: resolve presentation, parse arguments, dispatch, and map the result
//! to an exit status.
//!
//! Every command in the surface parses; only `version` runs. The rest report
//! that they do not exist yet, which is the honest thing to do and the reason
//! the whole surface can be committed before the tool works.

use std::ffi::OsString;
use std::process::ExitCode;

use clap::{ArgMatches, ColorChoice, CommandFactory, FromArgMatches};

use crate::cli::{Cli, Command, color};

/// A command that ran and failed.
const EXIT_FAILURE: u8 = 1;

/// A command that did not run at all. This is clap's status for a usage error,
/// and asking for a command batfiles cannot perform yet is the same kind of
/// mistake: the invocation was wrong, and nothing happened.
const EXIT_UNIMPLEMENTED: u8 = 2;

pub(crate) fn run() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().collect();

    // Presentation is settled first: clap may need to render `--help`,
    // `--version`, or a usage error before there is a parsed `Cli` to consult,
    // and that output should honor the requested color too.
    let color = color::resolve(
        color::preparse_choice(&args),
        env("BATFILES_COLOR").as_deref(),
        env("NO_COLOR").as_deref(),
    );
    if let Some(warning) = &color.warning {
        eprintln!("warning: {warning}");
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

    match cli.command {
        // Rendered through clap so `version` and `--version` cannot drift.
        Command::Version => {
            print!("{}", Cli::command().render_version());
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("error: `{name}` is not implemented yet");
            ExitCode::from(EXIT_UNIMPLEMENTED)
        }
    }
}

/// One environment value, decoded the way batfiles decodes every other one: a
/// name it cannot read as text is not an error here, it is simply a value that
/// fails whatever rule applies to it.
fn env(name: &str) -> Option<String> {
    std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
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
