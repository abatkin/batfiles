//! `run`: run a command in the root of the repository that declared it. See [the `run`
//! action](../../docs/actions/run.md).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::error::Error;
use crate::filesystem::paths;
use crate::manifest::actions::RunAction;
use crate::manifest::check::ValueRule;
use crate::manifest::item::ItemId;
use crate::manifest::remote::Tree;
use crate::manifest::template::Resolver;
use crate::manifest::trigger::RunCommand;
use crate::manifest::vars::CommandSpec;
use crate::run::context::RunContext;
use crate::subprocess::{self, LaunchFailure, WaitOutcome};

/// `command-timeout`'s default.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Run the command of a `run` action whose trigger fired, in its `cwd` or else the root of
/// `tree`, after resolving its templates and printing the line that names it. A dry run
/// resolves the templates and reports the command without launching it.
///
/// Return whether the command ran and succeeded. A failure is an error, or with `allow-failure`
/// a warning.
pub(super) fn run(
    action: &RunAction,
    tree: Tree<&ItemId>,
    fields: &Resolver<'_, '_>,
    context: &RunContext,
) -> Result<bool, Error> {
    let spec = resolve(&action.command, fields, context)?;
    let root = context.working_directory(tree)?;
    let cwd = match fields.optional("cwd", action.cwd.as_ref(), ValueRule::Destination)? {
        Some(cwd) => working_directory(&root, &cwd, context),
        None => root,
    };
    let env = action
        .env
        .iter()
        .map(|(name, value)| {
            let value = fields.field("env", value, ValueRule::Text)?;
            Ok((name.as_str(), value.into_owned()))
        })
        .collect::<Result<Vec<_>, Error>>()?;
    let reporter = context.reporter();
    let record = fields.record();
    if !context.mode().writes() {
        reporter.info(&format!("would {record}: {}", action.command));
        return Ok(false);
    }
    reporter.info(&format!("{record}: {}", action.command));
    let timeout = action
        .command_timeout
        .map_or(DEFAULT_TIMEOUT, |limit| limit.get());
    // A `cwd` that cannot be inspected, such as a dangling symlink, is a failure to start.
    let launched = if matches!(paths::reaches_directory(&cwd), Ok(true)) {
        launch(&spec, &cwd, &env, timeout, reporter.is_quiet())
    } else {
        Err(LaunchFailure::NoDirectory(cwd))
    };
    match launched {
        Ok(()) => Ok(true),
        Err(failure) if action.allow_failure => {
            reporter.warn(&format!(
                "{record}: the command {failure}; continuing, as `allow-failure` is set"
            ));
            Ok(false)
        }
        Err(failure) => Err(Error::CommandFailed {
            record: record.to_owned(),
            failure,
        }),
    }
}

/// The command with its templates resolved. A list's first element beginning `~/` names a
/// path under the selected home.
fn resolve(
    command: &RunCommand,
    fields: &Resolver<'_, '_>,
    context: &RunContext,
) -> Result<CommandSpec, Error> {
    let resolve = |part| {
        fields
            .field("command", part, ValueRule::Text)
            .map(|text| text.into_owned())
    };
    Ok(match command {
        RunCommand::Shell(line) => CommandSpec::Shell(resolve(line)?),
        RunCommand::Args(parts) => {
            let mut args = parts.iter().map(resolve).collect::<Result<Vec<_>, _>>()?;
            if let Some(program) = args.first_mut()
                && program.starts_with("~/")
            {
                *program = context.destination(program).to_string_lossy().into_owned();
            }
            CommandSpec::Args(args)
        }
    })
}

/// A resolved `cwd`: under the selected home for `~`, otherwise from `root`, the declaring
/// repository's root. An absolute path stays as written.
fn working_directory(root: &Path, cwd: &str, context: &RunContext) -> PathBuf {
    if cwd.starts_with('~') {
        context.destination(cwd)
    } else {
        paths::normalize_lexically(&root.join(cwd))
    }
}

/// Run `spec` in `cwd` with the inherited environment plus `env`, inherited standard error,
/// standard input closed, and standard output inherited unless `quiet`. Kill the direct child at
/// `timeout`.
fn launch(
    spec: &CommandSpec,
    cwd: &Path,
    env: &[(&str, String)],
    timeout: Duration,
    quiet: bool,
) -> Result<(), LaunchFailure> {
    let started = Instant::now();
    let mut command = subprocess::build(spec, cwd);
    command
        .envs(env.iter().map(|(name, value)| (*name, value)))
        .stdin(Stdio::null())
        .stdout(if quiet {
            Stdio::null()
        } else {
            Stdio::inherit()
        })
        .stderr(Stdio::inherit());
    let mut child = command.spawn().map_err(LaunchFailure::NotStarted)?;
    match subprocess::wait_until(&mut child, started, timeout, || false) {
        Ok(WaitOutcome::Exited(status)) if status.success() => Ok(()),
        Ok(WaitOutcome::Exited(status)) => Err(LaunchFailure::Exited(status)),
        Ok(WaitOutcome::TimedOut | WaitOutcome::GaveUp) => match subprocess::stop(&mut child) {
            Ok(()) => Err(LaunchFailure::TimedOut(timeout)),
            Err(error) => Err(LaunchFailure::NotStopped(error)),
        },
        Err(error) => {
            // A failed wait does not prove the child exited.
            let _ = subprocess::stop(&mut child);
            Err(LaunchFailure::NotWaited(error))
        }
    }
}
