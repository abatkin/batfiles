use super::*;
use crate::manifest::duration::FriendlyDuration;

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
    use crate::manifest::vars::CommandSpec;
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
        let (outcome, ended) = left_behind("(sleep 0.5; yes; touch ended) & printf done", "200ms");
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
        let CaptureOutcome::Assumed { value, reason } = run(&args(missing, CaptureMode::Status))
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
