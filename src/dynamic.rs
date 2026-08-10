//! Running one dynamic variable's command.
//!
//! Given a declaration and the root of the repository that declared it, this
//! produces the string the cache would hold, or the reason there is none. It
//! classifies and returns; it prints nothing and decides nothing about severity.
//! The caller caches, warns, and works out what is missing.
//!
//! The execution contract is `docs/environment.md`'s: the working directory is
//! the declaring repository's root, the process environment is inherited, the
//! resolved user-variable scope is not exported, a command string runs under
//! POSIX `sh` rather than the user's login shell, stdin is connected to nothing,
//! and the run is bounded by `command-timeout`.
//!
//! The mechanism behind that contract is deliberately plain — `try_wait` on a
//! poll interval, and a temporary file rather than a pipe for a captured stdout.
//! A signal-based timeout helper installs a process-wide `SIGCHLD` handler, and
//! a pipe both fills at its capacity while batfiles is waiting on the command
//! and stays open for as long as any descendant holds the writer.
#![allow(dead_code, reason = "no command evaluates a dynamic variable yet")]

use std::fmt;
use std::io::{self, Read as _};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::NamedTempFile;

use crate::output::Verbosity;
use crate::repo::{Capture, CommandSpec, DynamicVar};

/// `command-timeout`'s default (`docs/repoformat.md`).
///
/// A `std::time::Duration` rather than a `SignedDuration` because nothing here
/// does signed arithmetic, so the const is written in the unit it is consumed
/// in.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// The first interval the runner waits before re-checking a running command.
///
/// The check that precedes it fires before any command could plausibly have
/// finished, so this is the floor on what every capture costs. Commands like
/// `git config user.email` finish in a couple of milliseconds, and plan-building
/// evaluates every reachable declaration, so the floor is paid once per variable.
const FIRST_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// The interval the runner backs off to for a command that keeps running.
///
/// The backoff doubles from [`FIRST_POLL_INTERVAL`] up to this, which bounds
/// both the wasted checks on a slow command and the overshoot past its timeout.
/// The last sleep is shortened to the remaining duration.
const MAX_POLL_INTERVAL: Duration = Duration::from_millis(10);

/// One mebibyte, the unit [`MAX_OUTPUT`] is expressed and reported in.
const MIB: u64 = 1024 * 1024;

/// The most captured stdout a variable value may hold.
///
/// A value is written into the cache document, and from there into a symlink
/// target or a rendered template, so a mebibyte is already far past anything a
/// dotfiles variable plausibly is. Exceeding it is a failure rather than a
/// truncation, which is the judgment invalid UTF-8 gets for the same reason: a
/// value cut in half is worse than no value at all.
///
/// The limit is also enforced while the command runs, so a command that writes
/// without end cannot fill the temporary directory for the length of its
/// timeout before anything rejects the result.
const MAX_OUTPUT: u64 = MIB;

/// The shell behind a `command` string: POSIX `sh`, never `$SHELL`.
///
/// A login shell runs the user's rc files, so the same manifest would capture
/// different values on two machines whose owner happens to prefer a different
/// interactive shell — the class of surprise `docs/safety.md` rejects when it
/// insists `~` mean the *selected* home rather than an independently discovered
/// one. `docs/repoformat.md`'s own example spells `["sh", "-c", …]` out by hand.
#[cfg(unix)]
const SHELL: (&str, &str) = ("sh", "-c");
#[cfg(windows)]
const SHELL: (&str, &str) = ("cmd", "/C");

/// Run `decl`'s command with its working directory at `cwd`, the root of the
/// repository that declared it.
///
/// `verbosity` decides one thing only: whether the child's stderr reaches the
/// terminal. It is a `Copy` value from `output`, not the `Reporter` — the runner
/// still prints nothing itself.
pub(crate) fn capture(decl: &DynamicVar, cwd: &Path, verbosity: Verbosity) -> Outcome {
    // `unsigned_abs` is total: a negative `command-timeout` is rejected when the
    // duration parses and a zero one when the field deserializes, so what
    // reaches here is always strictly positive and there is no degenerate case
    // to test for at spawn time.
    let timeout = decl
        .command_timeout
        .map_or(DEFAULT_TIMEOUT, |limit| limit.as_signed().unsigned_abs());

    let (output, stdout) = match child_stdout(decl.capture) {
        Ok(capture) => capture,
        Err(error) => return Outcome::Failed(RunError::OutputFile(error)),
    };

    let mut child = match build(&decl.command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(child_stderr(verbosity))
        .spawn()
    {
        Ok(child) => child,
        // `docs/state.md`: a command that cannot be *started* makes a status
        // capture a transient `"false"`, which is used for this run and never
        // cached. A stdout capture has no such value to fall back on.
        Err(error) => {
            return match decl.capture {
                Capture::Status => Outcome::Transient(FALSE.to_owned()),
                Capture::Stdout => Outcome::Failed(RunError::NotStarted(error)),
            };
        }
    };

    let status = match wait_until(&mut child, timeout, output.as_ref()) {
        Ok(Waited::Exited(status)) => status,
        // Expiry is a failure for both capture modes, distinct from a status
        // capture's `"false"`: a command that was cut off never answered the
        // question. Overrunning the output limit ends the same way, because
        // nothing the command went on to write could make the result usable.
        Ok(Waited::TimedOut) => return stopped(&mut child, RunError::TimedOut(timeout)),
        Ok(Waited::TooLarge) => return stopped(&mut child, RunError::TooLarge),
        Err(error) => {
            // A wait failure does not prove that the child exited. Make a
            // best-effort cleanup before returning the original diagnosis.
            let _ = stop(&mut child);
            return Outcome::Failed(RunError::NotWaited(error));
        }
    };

    match decl.capture {
        Capture::Status => {
            Outcome::Captured(if status.success() { TRUE } else { FALSE }.to_owned())
        }
        Capture::Stdout => {
            if !status.success() {
                return Outcome::Failed(RunError::Exited(status));
            }
            let output = output.expect("a stdout capture prepares an output file");
            // The running check cannot catch a command that writes past the
            // limit and exits between two polls, so the limit is applied to the
            // finished file as well.
            let bytes = match read_output(output) {
                Ok(Some(bytes)) => bytes,
                Ok(None) => return Outcome::Failed(RunError::TooLarge),
                Err(error) => return Outcome::Failed(RunError::Unreadable(error)),
            };
            // Rejected rather than reinterpreted: a lossy conversion would
            // install a mangled value into the cache and from there into a
            // symlink target or a rendered template.
            match String::from_utf8(bytes) {
                Ok(text) => Outcome::Captured(text.trim().to_owned()),
                Err(_) => Outcome::Failed(RunError::NotUtf8),
            }
        }
    }
}

/// The two strings a `capture = "status"` variable can hold (`docs/state.md`).
const TRUE: &str = "true";
const FALSE: &str = "false";

/// What one run produced.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// A value to use and to cache.
    Captured(String),
    /// A value for this run only, never cached: `docs/state.md`'s transient
    /// `"false"` for a status capture whose command could not be started.
    /// Caching it would record that a `$PATH` was wrong once.
    Transient(String),
    /// No value. The caller retains a cached one if there is one, and warns.
    Failed(RunError),
}

/// Why a run produced no value.
///
/// `Display` is written to be dropped into a warning line after a qualified
/// variable name.
#[derive(Debug)]
pub(crate) enum RunError {
    /// The process could not be started: not on `PATH`, not executable, cwd
    /// gone. Carries the `io::Error` because its text is the whole diagnosis.
    NotStarted(io::Error),
    /// A secure temporary file could not be prepared for captured stdout.
    OutputFile(io::Error),
    /// The child could not be checked for completion.
    NotWaited(io::Error),
    /// A timed-out child could not be killed and reaped.
    NotStopped(io::Error),
    /// Killed at its `command-timeout`.
    TimedOut(Duration),
    /// Wrote more output than a variable value may hold. The limit is a
    /// constant of the program rather than a fact about this run, so it is not
    /// carried here; a command stopped while running is only known to have
    /// passed it, not by how much.
    TooLarge,
    /// Ran to completion with a non-zero status under `capture = "stdout"`.
    Exited(ExitStatus),
    /// Exited zero, but its output could not be read back from the temporary
    /// file.
    Unreadable(io::Error),
    /// Wrote bytes that are not UTF-8.
    NotUtf8,
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStarted(error) => write!(f, "could not be started: {error}"),
            Self::OutputFile(error) => {
                write!(f, "could not prepare a file for its output: {error}")
            }
            Self::NotWaited(error) => write!(f, "could not be checked for completion: {error}"),
            Self::NotStopped(error) => {
                write!(f, "timed out but could not be stopped cleanly: {error}")
            }
            // `Duration`'s `Debug` is its human-readable form: `5s`, `100ms`.
            Self::TimedOut(limit) => write!(f, "timed out after {limit:?}"),
            Self::TooLarge => write!(
                f,
                "produced more output than the {} MiB a value may hold",
                MAX_OUTPUT / MIB
            ),
            Self::Exited(status) => match status.code() {
                Some(code) => write!(f, "exited with status {code}"),
                None => write!(f, "did not exit normally ({status})"),
            },
            Self::Unreadable(error) => write!(f, "produced output that could not be read: {error}"),
            Self::NotUtf8 => f.write_str("produced output that is not valid UTF-8"),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NotStarted(error)
            | Self::OutputFile(error)
            | Self::NotWaited(error)
            | Self::NotStopped(error)
            | Self::Unreadable(error) => Some(error),
            Self::TimedOut(_) | Self::TooLarge | Self::Exited(_) | Self::NotUtf8 => None,
        }
    }
}

/// Prepare the child's stdout and, for a stdout capture, the independent handle
/// from which batfiles will read the result.
///
/// A regular file deliberately replaces a pipe. The command can fill a pipe
/// while batfiles waits for it, and a background descendant can keep a pipe's
/// writer open after the command exits. Neither condition delays a regular-file
/// reader. `reopen` gives the child its own file position, so reading does not
/// disturb a descendant that inherited the writer.
fn child_stdout(capture: Capture) -> io::Result<(Option<NamedTempFile>, Stdio)> {
    match capture {
        Capture::Stdout => {
            let output = NamedTempFile::new()?;
            let writer = output.reopen()?;
            Ok((Some(output), Stdio::from(writer)))
        }
        Capture::Status => Ok((None, Stdio::null())),
    }
}

/// How the wait for the direct child ended.
#[derive(Debug)]
enum Waited {
    /// It exited on its own, with this status.
    Exited(ExitStatus),
    /// It was still running at its `command-timeout`.
    TimedOut,
    /// It had already written more than [`MAX_OUTPUT`], so the rest of the run
    /// could not change the verdict.
    TooLarge,
}

/// Wait no longer than `timeout` for the direct child to exit, giving up early
/// once it has written more output than a value may hold.
///
/// `try_wait` is enough for this synchronous tool and, unlike a signal-based
/// timeout helper, changes no process-wide state. `saturating_sub` also avoids
/// constructing an `Instant` beyond the platform's representable range for a
/// very large but valid friendly duration.
fn wait_until(
    child: &mut Child,
    timeout: Duration,
    output: Option<&NamedTempFile>,
) -> io::Result<Waited> {
    let started = Instant::now();
    let mut interval = FIRST_POLL_INTERVAL;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Waited::Exited(status));
        }
        // Checked while the command runs, not only after it exits: at the write
        // rate of a `/dev/zero` copy, a command with the default timeout could
        // otherwise put gigabytes into the temporary directory before anything
        // rejected the result.
        if let Some(output) = output
            && output.as_file().metadata()?.len() > MAX_OUTPUT
        {
            return Ok(Waited::TooLarge);
        }

        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Ok(Waited::TimedOut);
        }
        thread::sleep(remaining.min(interval));
        interval = next_poll_interval(interval);
    }
}

/// Back off from one poll interval to the next.
///
/// Doubling keeps a fast command — the common case, and the one that decides
/// what plan building costs — from paying the ceiling, while a command that is
/// going to run for its whole timeout settles at the ceiling within a few
/// checks instead of being polled hundreds of times.
fn next_poll_interval(current: Duration) -> Duration {
    (current * 2).min(MAX_POLL_INTERVAL)
}

/// Kill and reap the direct child after a timeout or wait failure.
fn stop(child: &mut Child) -> io::Result<()> {
    child.kill()?;
    child.wait()?;
    Ok(())
}

/// Stop a child that will not be waited for any longer and report `reason` —
/// unless stopping it failed, which is the more urgent problem to name.
fn stopped(child: &mut Child, reason: RunError) -> Outcome {
    match stop(child) {
        Ok(()) => Outcome::Failed(reason),
        Err(error) => Outcome::Failed(RunError::NotStopped(error)),
    }
}

/// Read exactly the output present when the direct child exited, or `None` if
/// there is more of it than [`MAX_OUTPUT`] allows.
///
/// Snapshotting the length prevents a background descendant from extending the
/// capture by continuing to write after its parent command has finished, and it
/// is what the limit is applied to, so an oversized capture is rejected without
/// being read into memory first. The retained `NamedTempFile` handle has an
/// independent position that is still at the start of the file.
fn read_output(mut output: NamedTempFile) -> io::Result<Option<Vec<u8>>> {
    let length = output.as_file().metadata()?.len();
    if length > MAX_OUTPUT {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    output.as_file_mut().take(length).read_to_end(&mut bytes)?;
    Ok(Some(bytes))
}

/// The command to spawn, before its streams and working directory are set.
fn build(spec: &CommandSpec) -> Command {
    match spec {
        CommandSpec::Shell(line) => {
            let (program, flag) = SHELL;
            let mut command = Command::new(program);
            command.arg(flag).arg(line);
            command
        }
        CommandSpec::Args(args) => {
            let (program, rest) = args
                .split_first()
                .expect("an empty `command` list is rejected when it deserializes");
            let mut command = Command::new(program);
            command.args(rest);
            command
        }
    }
}

/// Where the child's standard error goes.
fn child_stderr(verbosity: Verbosity) -> Stdio {
    if silences_child_stderr(verbosity) {
        Stdio::null()
    } else {
        Stdio::inherit()
    }
}

/// Whether the child's standard error is disconnected rather than inherited.
///
/// Inheriting is the rule: `fatal: not a git repository` reaches the user
/// verbatim and in the order it happened, on the stream batfiles already writes
/// its own diagnostics to. `--quiet` is the exception, because a dynamic
/// command's stderr is the most voluminous thing batfiles can put on that stream
/// and the only part of it batfiles did not write — silencing batfiles' own
/// progress lines while leaving a chatty `git` untouched is not the silence
/// anyone asked for. What survives is what `docs/cmdline.md` promises: a failed
/// capture is still reported, by batfiles' own warning.
fn silences_child_stderr(verbosity: Verbosity) -> bool {
    matches!(verbosity, Verbosity::Quiet)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::FriendlyDuration;

    /// The process tests run real commands, which is the seam
    /// `docs/architecture.md` names for exactly this.
    #[cfg(unix)]
    mod running {
        use super::*;
        use std::time::Instant;
        use tempfile::TempDir;

        fn shell(line: &str, capture: Capture) -> DynamicVar {
            declaration(CommandSpec::Shell(line.to_owned()), capture, None)
        }

        fn args<const N: usize>(args: [&str; N], capture: Capture) -> DynamicVar {
            let args = args.iter().map(|arg| (*arg).to_owned()).collect();
            declaration(CommandSpec::Args(args), capture, None)
        }

        fn declaration(
            command: CommandSpec,
            capture: Capture,
            timeout: Option<&str>,
        ) -> DynamicVar {
            DynamicVar {
                command,
                capture,
                cache: None,
                command_timeout: timeout
                    .map(|text| FriendlyDuration::new(text).expect("valid duration")),
            }
        }

        /// A fresh working directory per run, and `Quiet` so a test that expects
        /// a failing command does not spray the harness's own stderr. The
        /// verbosity mapping is asserted separately, below.
        fn run(decl: &DynamicVar) -> (Outcome, TempDir) {
            let dir = TempDir::new().expect("temp dir");
            let outcome = capture(decl, dir.path(), Verbosity::Quiet);
            (outcome, dir)
        }

        fn captured(decl: &DynamicVar) -> String {
            match run(decl).0 {
                Outcome::Captured(value) => value,
                other => panic!("expected a captured value, got {other:?}"),
            }
        }

        fn failure(decl: &DynamicVar) -> RunError {
            match run(decl).0 {
                Outcome::Failed(error) => error,
                other => panic!("expected a failure, got {other:?}"),
            }
        }

        #[test]
        fn a_stdout_capture_is_trimmed() {
            // The assertion that `echo`'s trailing newline never reaches the
            // cache, and from there a symlink target.
            assert_eq!(captured(&shell("echo hi", Capture::Stdout)), "hi");
        }

        /// `dd` copying `bytes` from `/dev/zero`, which is the shortest way to
        /// ask for an exact amount of output. NUL is not whitespace, so nothing
        /// is trimmed off either end of what it writes.
        fn writes_bytes(bytes: u64, timeout: &str) -> DynamicVar {
            declaration(
                CommandSpec::Args(vec![
                    "dd".to_owned(),
                    "if=/dev/zero".to_owned(),
                    format!("bs={bytes}"),
                    "count=1".to_owned(),
                ]),
                Capture::Stdout,
                Some(timeout),
            )
        }

        #[test]
        fn stdout_larger_than_a_pipe_is_captured_without_blocking_the_command() {
            // Half a mebibyte is several times the default pipe capacity and
            // still inside the value limit. A runner that waits before draining
            // a pipe times out here; the regular-file sink lets `dd` finish.
            let half = 512 * 1024;
            assert_eq!(captured(&writes_bytes(half, "2s")).len(), half as usize);
        }

        #[test]
        fn output_at_the_limit_is_captured_and_output_past_it_is_not() {
            // The boundary in both directions, so neither an off-by-one nor a
            // limit that never fires can pass.
            assert_eq!(
                captured(&writes_bytes(MAX_OUTPUT, "2s")).len(),
                MAX_OUTPUT as usize
            );

            let error = failure(&writes_bytes(MAX_OUTPUT + 1, "2s"));
            assert!(
                matches!(error, RunError::TooLarge),
                "expected an oversized capture, got {error:?}"
            );
        }

        #[test]
        fn a_command_that_writes_without_end_is_stopped_before_its_timeout() {
            // The disk-fill guard: `yes` never exits, so a limit applied only to
            // the finished file would let it write for the whole timeout. The
            // elapsed assertion is what distinguishes the two.
            let decl = declaration(
                CommandSpec::Args(vec!["yes".to_owned()]),
                Capture::Stdout,
                Some("10s"),
            );

            let started = Instant::now();
            let error = failure(&decl);
            let elapsed = started.elapsed();

            assert!(
                matches!(error, RunError::TooLarge),
                "expected an oversized capture, got {error:?}"
            );
            assert!(elapsed < Duration::from_secs(2), "took {elapsed:?}");
        }

        #[test]
        fn a_descendant_holding_stdout_does_not_extend_the_capture() {
            // The shell answers immediately, but its background child retains
            // the stdout handle for a second. A pipe reader would wait for that
            // child to close the handle and violate the 100ms command timeout.
            let decl = declaration(
                CommandSpec::Shell("sleep 1 & printf done".to_owned()),
                Capture::Stdout,
                Some("100ms"),
            );

            let started = Instant::now();
            assert_eq!(captured(&decl), "done");
            let elapsed = started.elapsed();
            assert!(elapsed < Duration::from_millis(500), "took {elapsed:?}");
        }

        #[test]
        fn both_command_spellings_reach_the_same_value() {
            assert_eq!(captured(&shell("echo hi", Capture::Stdout)), "hi");
            assert_eq!(
                captured(&args(["echo", "hi"], Capture::Stdout)),
                "hi",
                "a direct argument vector skips the shell but not the value"
            );
        }

        #[test]
        fn the_working_directory_is_the_declaring_repository() {
            let decl = shell("pwd", Capture::Stdout);
            let dir = TempDir::new().expect("temp dir");
            let outcome = capture(&decl, dir.path(), Verbosity::Quiet);

            let Outcome::Captured(value) = outcome else {
                panic!("expected a captured value, got {outcome:?}");
            };
            // Canonicalized on both sides: macOS resolves `/var` to
            // `/private/var` and this test is not about that.
            assert_eq!(
                std::fs::canonicalize(&value).expect("the captured path exists"),
                std::fs::canonicalize(dir.path()).expect("the temp dir exists")
            );
        }

        #[test]
        fn a_status_capture_is_true_or_false_and_both_are_cacheable() {
            assert_eq!(captured(&args(["true"], Capture::Status)), "true");
            assert_eq!(captured(&args(["false"], Capture::Status)), "false");
        }

        #[test]
        fn a_non_zero_exit_fails_a_stdout_capture() {
            // Printing first is the case worth pinning: the output is real, and
            // it is still not a value.
            let error = failure(&shell("echo partial; exit 3", Capture::Stdout));
            let RunError::Exited(status) = error else {
                panic!("expected a non-zero exit, got {error:?}");
            };
            assert_eq!(status.code(), Some(3));
        }

        #[test]
        fn a_missing_program_is_transient_only_for_a_status_capture() {
            // The asymmetry `Transient` exists for: `docs/state.md`'s "cannot be
            // started ⇒ `false`" is a status-capture rule alone, and the value
            // it produces is never cached.
            let missing = ["batfiles-no-such-program-exists"];
            let error = failure(&args(missing, Capture::Stdout));
            assert!(
                matches!(error, RunError::NotStarted(_)),
                "expected a spawn failure, got {error:?}"
            );

            let outcome = run(&args(missing, Capture::Status)).0;
            let Outcome::Transient(value) = outcome else {
                panic!("expected a transient value, got {outcome:?}");
            };
            assert_eq!(value, "false");
        }

        #[test]
        fn a_command_that_overruns_its_timeout_is_killed_and_fails() {
            let decl = declaration(
                CommandSpec::Args(vec!["sleep".to_owned(), "30".to_owned()]),
                Capture::Stdout,
                Some("100ms"),
            );

            let started = Instant::now();
            let error = failure(&decl);
            let elapsed = started.elapsed();

            assert!(
                matches!(error, RunError::TimedOut(_)),
                "expected a timeout, got {error:?}"
            );
            // Without this the test would also pass on a runner that never
            // enforced anything and simply waited out the `sleep`.
            assert!(elapsed < Duration::from_secs(1), "took {elapsed:?}");
        }

        #[test]
        fn a_status_capture_that_times_out_fails_rather_than_saying_false() {
            // Decision 11 of the step plan, and the one behavior a reasonable
            // implementation gets wrong: a command that was cut off never
            // answered the question, so there is no `"false"` to cache.
            let decl = declaration(
                CommandSpec::Args(vec!["sleep".to_owned(), "30".to_owned()]),
                Capture::Status,
                Some("100ms"),
            );

            let error = failure(&decl);
            assert!(
                matches!(error, RunError::TimedOut(_)),
                "expected a timeout, got {error:?}"
            );
        }

        #[test]
        fn stdin_is_connected_to_nothing() {
            // A command that reads gets EOF immediately rather than blocking on
            // a terminal nobody is watching. This test fails by hanging the
            // suite, which is the loudest failure available.
            assert_eq!(captured(&args(["cat"], Capture::Stdout)), "");
        }

        #[test]
        fn the_environment_is_inherited_and_the_scope_is_not_exported() {
            // Read-only: nothing calls `set_var`, which is `unsafe` in edition
            // 2024 and racy under a threaded test harness.
            let decl = shell(
                r#"printf '%s %s' "${PATH:+inherited}" "${BATFILES_VAR_ANYTHING:-unset}""#,
                Capture::Stdout,
            );
            assert_eq!(captured(&decl), "inherited unset");
        }

        #[test]
        fn output_that_is_not_utf8_is_a_failure() {
            // Octal rather than `\xff`: `printf '\377'` is the POSIX spelling
            // and every `sh` supports it.
            let error = failure(&shell(r"printf '\377'", Capture::Stdout));
            assert!(
                matches!(error, RunError::NotUtf8),
                "expected invalid UTF-8, got {error:?}"
            );
        }
    }

    #[test]
    fn only_quiet_silences_the_child_stderr() {
        // A child's inherited stderr goes to the harness's own, which a unit
        // test cannot intercept, so the seam is this mapping. The end-to-end
        // assertion waits for `vars list --quiet`.
        assert!(silences_child_stderr(Verbosity::Quiet));
        assert!(!silences_child_stderr(Verbosity::Normal));
        assert!(!silences_child_stderr(Verbosity::Verbose(1)));
    }

    #[test]
    fn the_poll_interval_doubles_up_to_the_ceiling_and_stops_there() {
        // The floor is what every capture pays, so it is worth pinning that the
        // backoff starts small; the ceiling is what keeps timeout overshoot
        // bounded for a command that runs to it.
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

    #[test]
    fn a_declaration_without_a_timeout_gets_the_documented_default() {
        assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(5));
        assert_eq!(
            FriendlyDuration::new("5s")
                .expect("valid duration")
                .as_signed()
                .unsigned_abs(),
            DEFAULT_TIMEOUT
        );
    }

    #[test]
    fn every_failure_reads_as_a_clause_after_a_variable_name() {
        assert_eq!(
            RunError::TimedOut(Duration::from_millis(100)).to_string(),
            "timed out after 100ms"
        );
        assert_eq!(
            RunError::NotUtf8.to_string(),
            "produced output that is not valid UTF-8"
        );
        assert_eq!(
            RunError::TooLarge.to_string(),
            "produced more output than the 1 MiB a value may hold"
        );
    }
}
