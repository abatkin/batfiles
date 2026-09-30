//! Run dynamic-variable commands as the invoking user, including during dry runs. Return
//! captured values or failures without printing diagnostics. See [command
//! execution](../../docs/environment.md#how-dynamic-commands-are-run).

use std::fmt;
use std::io::{self, PipeReader, Read as _};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::manifest::vars::{CaptureMode, CommandSpec, DynamicVarSpec};

/// `command-timeout`'s default.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Initial interval between checks for command completion.
const FIRST_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// Maximum interval between checks for command completion.
const MAX_POLL_INTERVAL: Duration = Duration::from_millis(10);

const MIB: u64 = 1024 * 1024;

/// The most captured stdout a value may hold.
const MAX_OUTPUT: u64 = MIB;

/// Shell and execution flag used for string commands.
#[cfg(unix)]
const SHELL: (&str, &str) = ("sh", "-c");
#[cfg(windows)]
const SHELL: (&str, &str) = ("cmd", "/C");

/// The two strings a `capture = "status"` variable can hold.
const TRUE: &str = "true";
const FALSE: &str = "false";

/// Run `spec`'s command in its declaring repository's root, `cwd`.
///
/// Inherit the process environment and stderr, suppressing stderr when `quiet`. Disconnect
/// stdin. Limit captured stdout to [`MAX_OUTPUT`] bytes and enforce the declaration's timeout.
pub(crate) fn capture(spec: &DynamicVarSpec, cwd: &Path, quiet: bool) -> CaptureOutcome {
    let started = Instant::now();
    let timeout = spec
        .command_timeout
        .map_or(DEFAULT_TIMEOUT, |limit| limit.get());
    let (reader, stdout) = match child_stdout(spec.capture) {
        Ok(capture) => capture,
        Err(error) => return CaptureOutcome::Failed(CaptureError::OutputPipe(error)),
    };
    let stderr = if quiet {
        Stdio::null()
    } else {
        Stdio::inherit()
    };
    let mut command = build(&spec.command);
    command
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(stdout)
        .stderr(stderr);
    let spawned = command.spawn();
    // Drop the builder's pipe writer so the reader can observe EOF.
    drop(command);
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            return match spec.capture {
                CaptureMode::Status => CaptureOutcome::Assumed {
                    value: FALSE.to_owned(),
                    reason: CaptureError::NotStarted(error),
                },
                CaptureMode::Stdout => CaptureOutcome::Failed(CaptureError::NotStarted(error)),
            };
        }
    };
    let mut output = reader.map(OutputCollector::read);

    let status = match wait_until(&mut child, started, timeout, output.as_mut()) {
        Ok(WaitOutcome::Exited(status)) => status,
        // A timeout fails status capture instead of yielding false.
        Ok(WaitOutcome::TimedOut) => return stopped(&mut child, CaptureError::TimedOut(timeout)),
        Ok(WaitOutcome::TooLarge) => return stopped(&mut child, CaptureError::TooLarge),
        Err(error) => {
            // A failed wait does not prove the child exited.
            let _ = stop(&mut child);
            return CaptureOutcome::Failed(CaptureError::NotWaited(error));
        }
    };

    let Some(mut output) = output else {
        return CaptureOutcome::Captured(if status.success() { TRUE } else { FALSE }.to_owned());
    };
    // Prefer the output-limit error over a resulting broken-pipe exit.
    if output.too_large() {
        return CaptureOutcome::Failed(CaptureError::TooLarge);
    }
    if !status.success() {
        return CaptureOutcome::Failed(CaptureError::Exited(status));
    }
    let bytes = match output.finish(started, timeout) {
        Ok(bytes) => bytes,
        Err(error) => return CaptureOutcome::Failed(error),
    };
    match String::from_utf8(bytes) {
        Ok(text) => CaptureOutcome::Captured(text.trim().to_owned()),
        Err(_) => CaptureOutcome::Failed(CaptureError::NotUtf8),
    }
}

/// What one run produced.
#[derive(Debug)]
pub(crate) enum CaptureOutcome {
    /// A value to use and to cache.
    Captured(String),
    /// `"false"` for a status capture whose command could not be started: used
    /// for this run and never cached. `reason` says why.
    Assumed { value: String, reason: CaptureError },
    /// No value.
    Failed(CaptureError),
}

/// Why a run produced no value. Displays as a clause following a variable's
/// name.
#[derive(Debug)]
pub(crate) enum CaptureError {
    /// Not on `PATH`, not executable, or the working directory is gone.
    NotStarted(io::Error),
    /// No pipe could be opened for captured stdout.
    OutputPipe(io::Error),
    /// The child could not be checked for completion.
    NotWaited(io::Error),
    /// A child that had to be stopped could not be killed and reaped.
    NotStopped(io::Error),
    /// Killed at its `command-timeout`.
    TimedOut(Duration),
    /// Wrote more than [`MAX_OUTPUT`].
    TooLarge,
    /// Exited non-zero under `capture = "stdout"`.
    Exited(ExitStatus),
    /// Exited zero, but its output was still open at its `command-timeout`:
    /// something it left running holds it.
    OutputLeftOpen(Duration),
    /// Its output could not be read.
    Unreadable(io::Error),
    /// Wrote bytes that are not UTF-8.
    NotUtf8,
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotStarted(error) => write!(f, "could not be started: {error}"),
            Self::OutputPipe(error) => {
                write!(f, "could not open a pipe for its output: {error}")
            }
            Self::NotWaited(error) => write!(f, "could not be checked for completion: {error}"),
            Self::NotStopped(error) => {
                write!(f, "had to be stopped and could not be: {error}")
            }
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
            Self::OutputLeftOpen(limit) => write!(
                f,
                "exited, but something it left running still held its output open after \
                 {limit:?}"
            ),
            Self::Unreadable(error) => write!(f, "produced output that could not be read: {error}"),
            Self::NotUtf8 => f.write_str("produced output that is not valid UTF-8"),
        }
    }
}

/// Return the child's stdout configuration and an optional reader for stdout capture.
fn child_stdout(capture: CaptureMode) -> io::Result<(Option<PipeReader>, Stdio)> {
    match capture {
        CaptureMode::Stdout => {
            let (reader, writer) = io::pipe()?;
            Ok((Some(reader), Stdio::from(writer)))
        }
        CaptureMode::Status => Ok((None, Stdio::null())),
    }
}

/// Collect stdout on a background thread until EOF or [`MAX_OUTPUT`] is exceeded. The reader
/// may outlive the capture if descendants keep the pipe open.
struct OutputCollector {
    receiver: mpsc::Receiver<OutputReadOutcome>,
    ended: Option<OutputReadOutcome>,
}

/// How reading an output ended.
enum OutputReadOutcome {
    Complete(Vec<u8>),
    TooLarge,
    Failed(io::Error),
}

impl OutputCollector {
    fn read(reader: PipeReader) -> Self {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            // One byte past the limit is enough to know it was passed.
            let mut limited = reader.take(MAX_OUTPUT + 1);
            let ended = match limited.read_to_end(&mut bytes) {
                Ok(_) if bytes.len() as u64 > MAX_OUTPUT => OutputReadOutcome::TooLarge,
                Ok(_) => OutputReadOutcome::Complete(bytes),
                Err(error) => OutputReadOutcome::Failed(error),
            };
            // Send the verdict before closing the pipe, which may cause the child to exit.
            let _ = sender.send(ended);
            drop(limited);
        });
        Self {
            receiver,
            ended: None,
        }
    }

    /// Whether the output has passed [`MAX_OUTPUT`]. Does not wait.
    fn too_large(&mut self) -> bool {
        if self.ended.is_none() {
            self.ended = self.receiver.try_recv().ok();
        }
        matches!(self.ended, Some(OutputReadOutcome::TooLarge))
    }

    /// Wait for complete output until `timeout` after `started`, or return a capture failure.
    fn finish(mut self, started: Instant, timeout: Duration) -> Result<Vec<u8>, CaptureError> {
        let ended = match self.ended.take() {
            Some(ended) => ended,
            None => self
                .receiver
                .recv_timeout(timeout.saturating_sub(started.elapsed()))
                .map_err(|_| CaptureError::OutputLeftOpen(timeout))?,
        };
        match ended {
            OutputReadOutcome::Complete(bytes) => Ok(bytes),
            OutputReadOutcome::TooLarge => Err(CaptureError::TooLarge),
            OutputReadOutcome::Failed(error) => Err(CaptureError::Unreadable(error)),
        }
    }
}

/// How the wait for the direct child ended.
#[derive(Debug)]
enum WaitOutcome {
    Exited(ExitStatus),
    TimedOut,
    TooLarge,
}

/// Wait for the direct child until `timeout` after `started`. Stop waiting early if captured
/// output exceeds the limit.
fn wait_until(
    child: &mut Child,
    started: Instant,
    timeout: Duration,
    mut output: Option<&mut OutputCollector>,
) -> io::Result<WaitOutcome> {
    let mut interval = FIRST_POLL_INTERVAL;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(WaitOutcome::Exited(status));
        }
        if output.as_mut().is_some_and(|output| output.too_large()) {
            return Ok(WaitOutcome::TooLarge);
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

/// Kill and reap the direct child. Descendant processes are not stopped.
fn stop(child: &mut Child) -> io::Result<()> {
    child.kill()?;
    child.wait()?;
    Ok(())
}

/// Stop a child and report `reason`, unless stopping it failed.
fn stopped(child: &mut Child, reason: CaptureError) -> CaptureOutcome {
    match stop(child) {
        Ok(()) => CaptureOutcome::Failed(reason),
        Err(error) => CaptureOutcome::Failed(CaptureError::NotStopped(error)),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::duration::FriendlyDuration;

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

    #[test]
    fn a_declaration_without_a_timeout_gets_the_documented_default() {
        assert_eq!(
            FriendlyDuration::new("5s").expect("valid").get(),
            DEFAULT_TIMEOUT
        );
    }

    /// Tests using real subprocesses.
    #[cfg(unix)]
    mod running {
        use super::*;
        use tempfile::TempDir;

        fn spec_of(
            command: CommandSpec,
            capture: CaptureMode,
            timeout: Option<&str>,
        ) -> DynamicVarSpec {
            DynamicVarSpec {
                command,
                capture,
                cache: None,
                command_timeout: timeout.map(|text| FriendlyDuration::new(text).expect("valid")),
            }
        }

        fn shell(line: &str, capture: CaptureMode) -> DynamicVarSpec {
            spec_of(CommandSpec::Shell(line.to_owned()), capture, None)
        }

        fn args<const N: usize>(args: [&str; N], capture: CaptureMode) -> DynamicVarSpec {
            let args = args.iter().map(|arg| (*arg).to_owned()).collect();
            spec_of(CommandSpec::Args(args), capture, None)
        }

        /// Capture a command in a temporary directory with stderr suppressed.
        fn run(spec: &DynamicVarSpec) -> CaptureOutcome {
            let dir = TempDir::new().expect("temp dir");
            capture(spec, dir.path(), true)
        }

        fn captured(spec: &DynamicVarSpec) -> String {
            match run(spec) {
                CaptureOutcome::Captured(value) => value,
                other => panic!("expected a captured value, got {other:?}"),
            }
        }

        fn failure(spec: &DynamicVarSpec) -> CaptureError {
            match run(spec) {
                CaptureOutcome::Failed(error) => error,
                other => panic!("expected a failure, got {other:?}"),
            }
        }

        /// `dd` copying exactly `bytes` NULs, which trimming leaves alone.
        fn writes_bytes(bytes: u64, timeout: &str) -> DynamicVarSpec {
            spec_of(
                CommandSpec::Args(vec![
                    "dd".to_owned(),
                    "if=/dev/zero".to_owned(),
                    format!("bs={bytes}"),
                    "count=1".to_owned(),
                ]),
                CaptureMode::Stdout,
                Some(timeout),
            )
        }

        #[test]
        fn a_stdout_capture_is_trimmed() {
            assert_eq!(captured(&shell("echo hi", CaptureMode::Stdout)), "hi");
        }

        #[test]
        fn both_command_spellings_reach_the_same_value() {
            assert_eq!(captured(&args(["echo", "hi"], CaptureMode::Stdout)), "hi");
        }

        #[test]
        fn stdout_larger_than_a_pipe_is_captured_without_blocking_the_command() {
            let half = 512 * 1024;
            assert_eq!(captured(&writes_bytes(half, "5s")).len(), half as usize);
        }

        #[test]
        fn output_at_the_limit_is_captured_and_output_past_it_is_not() {
            assert_eq!(
                captured(&writes_bytes(MAX_OUTPUT, "5s")).len(),
                MAX_OUTPUT as usize
            );
            let error = failure(&writes_bytes(MAX_OUTPUT + 1, "5s"));
            assert!(matches!(error, CaptureError::TooLarge), "{error:?}");
        }

        #[test]
        fn a_command_that_writes_without_end_is_stopped_before_its_timeout() {
            let spec = spec_of(
                CommandSpec::Args(vec!["yes".to_owned()]),
                CaptureMode::Stdout,
                Some("20s"),
            );
            let started = Instant::now();
            let error = failure(&spec);
            assert!(matches!(error, CaptureError::TooLarge), "{error:?}");
            assert!(started.elapsed() < Duration::from_secs(10));
        }

        /// Run `line` in a temporary directory, then wait up to five seconds for its descendant
        /// to create an `ended` marker after stdout closes. Return the capture outcome and
        /// whether the marker appeared.
        fn left_behind(line: &str, timeout: &str) -> (CaptureOutcome, bool) {
            let dir = TempDir::new().expect("temp dir");
            let spec = spec_of(
                CommandSpec::Shell(line.to_owned()),
                CaptureMode::Stdout,
                Some(timeout),
            );
            let outcome = capture(&spec, dir.path(), true);
            let waited = Instant::now();
            while !dir.path().join("ended").exists() && waited.elapsed() < Duration::from_secs(5) {
                thread::sleep(Duration::from_millis(10));
            }
            (outcome, dir.path().join("ended").exists())
        }

        #[test]
        fn a_writer_left_behind_past_the_limit_is_stopped() {
            // The surviving `yes` process exits when the output pipe closes.
            let (outcome, ended) = left_behind("(yes; touch ended) & sleep 30", "20s");
            assert!(
                matches!(outcome, CaptureOutcome::Failed(CaptureError::TooLarge)),
                "{outcome:?}"
            );
            assert!(ended, "the writer left behind is still writing");
        }

        #[test]
        fn a_writer_left_behind_by_a_timeout_is_stopped_at_the_limit() {
            let (outcome, ended) = left_behind("(sleep 0.3; yes; touch ended) & sleep 30", "100ms");
            assert!(
                matches!(outcome, CaptureOutcome::Failed(CaptureError::TimedOut(_))),
                "{outcome:?}"
            );
            assert!(ended, "the writer left behind is still writing");
        }

        #[test]
        fn output_held_open_past_the_timeout_fails_a_command_that_exited() {
            // The direct child exits before its descendant writes, exercising the output
            // deadline.
            let started = Instant::now();
            let (outcome, ended) =
                left_behind("(sleep 0.5; yes; touch ended) & printf done", "200ms");
            assert!(
                matches!(
                    outcome,
                    CaptureOutcome::Failed(CaptureError::OutputLeftOpen(_))
                ),
                "{outcome:?}"
            );
            assert!(started.elapsed() < Duration::from_secs(5));
            assert!(ended, "the writer left behind is still writing");
        }

        #[test]
        fn something_left_running_without_the_output_does_not_hold_the_capture() {
            let spec = spec_of(
                CommandSpec::Shell("sleep 2 >/dev/null & printf done".to_owned()),
                CaptureMode::Stdout,
                Some("1s"),
            );
            let started = Instant::now();
            assert_eq!(captured(&spec), "done");
            assert!(started.elapsed() < Duration::from_millis(500));
        }

        #[test]
        fn the_working_directory_is_the_one_given() {
            let dir = TempDir::new().expect("temp dir");
            let CaptureOutcome::Captured(value) =
                capture(&shell("pwd", CaptureMode::Stdout), dir.path(), true)
            else {
                panic!("expected a captured value");
            };
            assert_eq!(
                std::path::PathBuf::from(value)
                    .canonicalize()
                    .expect("exists"),
                dir.path().canonicalize().expect("exists")
            );
        }

        #[test]
        fn a_status_capture_is_true_or_false() {
            assert_eq!(captured(&args(["true"], CaptureMode::Status)), "true");
            assert_eq!(captured(&args(["false"], CaptureMode::Status)), "false");
        }

        #[test]
        fn a_non_zero_exit_fails_a_stdout_capture_even_with_output() {
            let error = failure(&shell("echo partial; exit 3", CaptureMode::Stdout));
            let CaptureError::Exited(status) = error else {
                panic!("expected a non-zero exit, got {error:?}");
            };
            assert_eq!(status.code(), Some(3));
        }

        #[test]
        fn a_missing_program_is_assumed_false_only_for_a_status_capture() {
            let missing = ["batfiles-no-such-program-exists"];
            let error = failure(&args(missing, CaptureMode::Stdout));
            assert!(matches!(error, CaptureError::NotStarted(_)), "{error:?}");
            let CaptureOutcome::Assumed { value, reason } =
                run(&args(missing, CaptureMode::Status))
            else {
                panic!("expected an assumed value");
            };
            assert_eq!(value, "false");
            assert!(matches!(reason, CaptureError::NotStarted(_)), "{reason:?}");
        }

        #[test]
        fn a_command_that_overruns_its_timeout_is_killed_and_fails_in_both_modes() {
            for capture in [CaptureMode::Stdout, CaptureMode::Status] {
                let spec = spec_of(
                    CommandSpec::Args(vec!["sleep".to_owned(), "30".to_owned()]),
                    capture,
                    Some("100ms"),
                );
                let started = Instant::now();
                let error = failure(&spec);
                assert!(matches!(error, CaptureError::TimedOut(_)), "{error:?}");
                assert!(started.elapsed() < Duration::from_secs(5));
            }
        }

        #[test]
        fn stdin_is_connected_to_nothing() {
            // Inherited stdin would leave `cat` waiting for input.
            assert_eq!(captured(&args(["cat"], CaptureMode::Stdout)), "");
        }

        #[test]
        fn the_environment_is_inherited() {
            let spec = shell(r#"printf '%s' "${PATH:+inherited}""#, CaptureMode::Stdout);
            assert_eq!(captured(&spec), "inherited");
        }

        #[test]
        fn output_that_is_not_utf8_is_a_failure() {
            let error = failure(&shell(r"printf '\377'", CaptureMode::Stdout));
            assert!(matches!(error, CaptureError::NotUtf8), "{error:?}");
        }
    }
}
