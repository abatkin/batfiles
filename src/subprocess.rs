//! Launch the commands a manifest declares, as the invoking user: building the process, waiting
//! for it under a timeout, and stopping it. See [how commands are
//! run](../docs/environment.md#how-dynamic-commands-are-run).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

use crate::manifest::vars::CommandSpec;

/// Initial interval between checks for command completion.
const FIRST_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// Maximum interval between checks for command completion.
const MAX_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// The command to spawn in `cwd`, before its streams are set: a string through the platform
/// shell, a list as a program and its arguments. A relative program with a directory part, such
/// as `./bin/setup`, is found from `cwd`, even when `cwd` is itself relative; a bare name is
/// looked up as usual.
pub(crate) fn build(spec: &CommandSpec, cwd: &Path) -> Command {
    let mut command = match spec {
        CommandSpec::Shell(line) => shell(line),
        CommandSpec::Args(args) => {
            let (program, rest) = args
                .split_first()
                .expect("an empty `command` list is rejected when it deserializes");
            let program = Path::new(program);
            let mut command = if program.is_relative()
                && program
                    .parent()
                    .is_some_and(|parent| !parent.as_os_str().is_empty())
            {
                // Anchored, so the child does not look for the program from inside a relative
                // `cwd` it has already entered.
                let anchored = std::path::absolute(cwd).unwrap_or_else(|_| cwd.to_path_buf());
                Command::new(anchored.join(program))
            } else {
                Command::new(program)
            };
            command.args(rest);
            command
        }
    };
    command.current_dir(cwd);
    command
}

/// `line` through `sh -c`.
#[cfg(unix)]
fn shell(line: &str) -> Command {
    let mut command = Command::new("sh");
    command.arg("-c").arg(line);
    command
}

/// `line` through `cmd /S /C`, which strips the quotes around the rest of its command line and
/// parses what is between them as written. Ordinary argument quoting would escape quotes inside
/// `line` in a way `cmd` does not read.
#[cfg(windows)]
fn shell(line: &str) -> Command {
    use std::os::windows::process::CommandExt as _;
    let mut command = Command::new("cmd");
    command.args(["/S", "/C"]).raw_arg(format!("\"{line}\""));
    command
}

/// How the wait for the direct child ended.
#[derive(Debug)]
pub(crate) enum WaitOutcome {
    Exited(ExitStatus),
    TimedOut,
    /// `give_up` asked to stop waiting.
    GaveUp,
}

/// Wait for the direct child until `timeout` after `started`, polling `give_up` between checks
/// to stop waiting early.
pub(crate) fn wait_until(
    child: &mut Child,
    started: Instant,
    timeout: Duration,
    mut give_up: impl FnMut() -> bool,
) -> io::Result<WaitOutcome> {
    let mut interval = FIRST_POLL_INTERVAL;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(WaitOutcome::Exited(status));
        }
        if give_up() {
            return Ok(WaitOutcome::GaveUp);
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(WaitOutcome::TimedOut);
        }
        thread::sleep(remaining.min(interval));
        interval = next_poll_interval(interval);
    }
}

/// Double the interval, up to [`MAX_POLL_INTERVAL`].
fn next_poll_interval(current: Duration) -> Duration {
    (current * 2).min(MAX_POLL_INTERVAL)
}

/// Why a command that was meant to succeed did not. Displays as a clause following "the
/// command".
#[derive(Debug)]
pub(crate) enum LaunchFailure {
    /// Not on `PATH`, not executable, or the working directory is gone.
    NotStarted(io::Error),
    /// The working directory it was to run in is not a directory.
    NoDirectory(PathBuf),
    /// The child could not be checked for completion.
    NotWaited(io::Error),
    /// A child that timed out could not be killed and reaped.
    NotStopped(io::Error),
    /// Killed at its timeout.
    TimedOut(Duration),
    /// Exited non-zero, or was ended by a signal.
    Exited(ExitStatus),
}

impl fmt::Display for LaunchFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStarted(error) => write!(f, "could not be started: {error}"),
            Self::NoDirectory(path) => write!(
                f,
                "could not be started: its working directory {} is not a directory",
                path.display()
            ),
            Self::NotWaited(error) => write!(f, "could not be checked for completion: {error}"),
            Self::NotStopped(error) => {
                write!(f, "timed out and could not be stopped: {error}")
            }
            Self::TimedOut(limit) => write!(f, "timed out after {limit:?} and was stopped"),
            Self::Exited(status) => match status.code() {
                Some(code) => write!(f, "exited with status {code}"),
                None => write!(f, "did not exit normally ({status})"),
            },
        }
    }
}

/// Kill and reap the direct child. Descendant processes are not stopped.
pub(crate) fn stop(child: &mut Child) -> io::Result<()> {
    child.kill()?;
    child.wait()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_poll_interval_doubles_up_to_the_ceiling_and_stops_there() {
        let mut interval = FIRST_POLL_INTERVAL;
        let mut sequence = vec![interval];
        for _ in 0..5 {
            interval = next_poll_interval(interval);
            sequence.push(interval);
        }
        assert_eq!(
            sequence,
            [1, 2, 4, 8, 10, 10].map(Duration::from_millis).to_vec()
        );
    }
}
