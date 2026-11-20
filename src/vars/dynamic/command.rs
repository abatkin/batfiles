//! Run dynamic-variable commands as the invoking user, including during dry runs. Return
//! captured values or failures without printing diagnostics. See [command
//! execution](../../../docs/environment.md#how-dynamic-commands-are-run).

use std::fmt;
use std::io::{self, PipeReader, Read as _};
use std::path::Path;
use std::process::{Child, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::manifest::vars::{CaptureMode, DynamicVarSpec};
use crate::subprocess::{self, WaitOutcome, stop};

/// `command-timeout`'s default.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

const MIB: u64 = 1024 * 1024;

/// The most captured stdout a value may hold.
const MAX_OUTPUT: u64 = MIB;

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
    let mut command = subprocess::build(&spec.command, cwd);
    command.stdin(Stdio::null()).stdout(stdout).stderr(stderr);
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

    let too_large = || output.as_mut().is_some_and(OutputCollector::too_large);
    let status = match subprocess::wait_until(&mut child, started, timeout, too_large) {
        Ok(WaitOutcome::Exited(status)) => status,
        // A timeout fails status capture instead of yielding false.
        Ok(WaitOutcome::TimedOut) => return stopped(&mut child, CaptureError::TimedOut(timeout)),
        Ok(WaitOutcome::GaveUp) => return stopped(&mut child, CaptureError::TooLarge),
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

/// Stop a child and report `reason`, unless stopping it failed.
fn stopped(child: &mut Child, reason: CaptureError) -> CaptureOutcome {
    match stop(child) {
        Ok(()) => CaptureOutcome::Failed(reason),
        Err(error) => CaptureOutcome::Failed(CaptureError::NotStopped(error)),
    }
}

#[cfg(test)]
mod tests;
