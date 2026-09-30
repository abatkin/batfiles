//! Format progress messages, diagnostics, and requested output.

use crate::mode::RunMode;

/// An installation operation reported in the tense selected by the run mode.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Verb {
    Link,
    Relink,
    Copy,
    Fetch,
    Refetch,
    Extract,
    Clone,
    Update,
    SwitchRef,
    Create,
    Remove,
    Keep,
    Refresh,
    BackUp,
    Discard,
    Restore,
    Skip,
}

impl Verb {
    /// The bare verb, for a sentence that names an act rather than reporting
    /// one: "no children to link in …".
    pub fn infinitive(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Relink => "relink",
            Self::Copy => "copy",
            Self::Fetch => "fetch",
            Self::Refetch => "refetch",
            Self::Extract => "extract",
            Self::Clone => "clone",
            Self::Update => "update",
            Self::SwitchRef => "switch",
            Self::Create => "create",
            Self::Remove => "remove",
            Self::Keep => "keep",
            Self::Refresh => "refresh",
            Self::BackUp => "back up",
            Self::Discard => "discard",
            Self::Restore => "restore",
            Self::Skip => "skip",
        }
    }

    /// What an action did, or — under [`RunMode::DryRun`] — would do.
    pub fn for_mode(self, mode: RunMode) -> String {
        match mode {
            RunMode::Perform => self.past().to_string(),
            RunMode::DryRun => format!("would {}", self.infinitive()),
        }
    }

    fn past(self) -> &'static str {
        match self {
            Self::Link => "linked",
            Self::Relink => "relinked",
            Self::Copy => "copied",
            Self::Fetch => "fetched",
            Self::Refetch => "refetched",
            Self::Extract => "extracted",
            Self::Clone => "cloned",
            Self::Update => "updated",
            Self::SwitchRef => "switched",
            Self::Create => "created",
            Self::Remove => "removed",
            Self::Keep => "kept",
            Self::Refresh => "refreshed",
            Self::BackUp => "backed up",
            Self::Discard => "discarded",
            Self::Restore => "restored",
            Self::Skip => "skipped",
        }
    }
}

/// Quote a string for single-line output, escaping control characters,
/// backslashes, and double quotes. Preserve apostrophes and printable Unicode.
/// Empty strings render as `""`.
pub(crate) fn quoted_value(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('"');
    for character in value.chars() {
        match character {
            '\'' => quoted.push(character),
            _ => quoted.extend(character.escape_debug()),
        }
    }
    quoted.push('"');
    quoted
}

/// Verbosity derived from `--quiet` and repeated `--verbose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verbosity {
    Quiet,
    Normal,
    /// One level per `-v`, starting at 1.
    Verbose(u8),
}

impl Verbosity {
    pub fn new(quiet: bool, verbose: u8) -> Self {
        match (quiet, verbose) {
            (true, _) => Self::Quiet,
            (false, 0) => Self::Normal,
            (false, level) => Self::Verbose(level),
        }
    }

    /// Return whether informational output is enabled; only quiet mode suppresses it.
    fn shows_info(self) -> bool {
        !matches!(self, Self::Quiet)
    }

    /// Whether detail at `level` (1 for `-v`, 2 for `-vv`, …) should be
    /// printed.
    fn shows_detail(self, level: u8) -> bool {
        matches!(self, Self::Verbose(current) if current >= level)
    }
}

/// Writes diagnostics honoring the resolved verbosity and color.
#[derive(Debug)]
pub(crate) struct Reporter {
    color: bool,
    verbosity: Verbosity,
}

impl Reporter {
    /// Create a reporter with the selected color setting and normal verbosity.
    pub fn new(color: bool) -> Self {
        Self {
            color,
            verbosity: Verbosity::Normal,
        }
    }

    /// Adopt the verbosity requested on the command line.
    pub fn set_verbosity(&mut self, verbosity: Verbosity) {
        self.verbosity = verbosity;
    }

    /// A failure. Always printed, including under `--quiet`.
    pub fn error(&self, message: &str) {
        eprintln!("{}", self.line(Label::Error, message));
    }

    /// A recoverable problem. Always printed, including under `--quiet`.
    pub fn warn(&self, message: &str) {
        eprintln!("{}", self.line(Label::Warning, message));
    }

    /// Print a progress message to stderr unless quiet mode is enabled.
    pub fn info(&self, message: &str) {
        if self.verbosity.shows_info() {
            eprintln!("{message}");
        }
    }

    /// Print a prompt to stderr without a newline, regardless of verbosity.
    pub fn prompt(&self, question: &str) {
        eprint!("{question} ");
    }

    /// Print requested data to stdout with a trailing newline, without labels or color,
    /// regardless of verbosity.
    pub fn data(&self, message: &str) {
        println!("{message}");
    }

    /// Whether `--quiet` was given.
    pub fn is_quiet(&self) -> bool {
        matches!(self.verbosity, Verbosity::Quiet)
    }

    /// Whether [`Self::detail`] at `level` is enabled; check before expensive formatting.
    pub fn shows_detail(&self, level: u8) -> bool {
        self.verbosity.shows_detail(level)
    }

    /// Print unlabeled detail to stderr when `-v` is repeated at least `level` times.
    pub fn detail(&self, level: u8, message: &str) {
        if self.verbosity.shows_detail(level) {
            eprintln!("{message}");
        }
    }

    fn line(&self, label: Label, message: &str) -> String {
        let text = label.text();
        if self.color {
            format!("\x1b[1;{}m{text}:\x1b[0m {message}", label.ansi_color())
        } else {
            format!("{text}: {message}")
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Label {
    Error,
    Warning,
}

impl Label {
    fn text(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }

    fn ansi_color(self) -> u8 {
        match self {
            Self::Error => 31,
            Self::Warning => 33,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verb_is_reported_in_the_tense_the_mode_calls_for() {
        assert_eq!(Verb::Link.for_mode(RunMode::Perform), "linked");
        assert_eq!(Verb::Link.for_mode(RunMode::DryRun), "would link");
        assert_eq!(Verb::Copy.for_mode(RunMode::Perform), "copied");
        assert_eq!(Verb::Keep.for_mode(RunMode::Perform), "kept");
    }

    #[test]
    fn an_ordinary_value_is_quoted_and_otherwise_left_alone() {
        assert_eq!(quoted_value("nvim"), "\"nvim\"");
        assert_eq!(quoted_value(""), "\"\"");
        assert_eq!(quoted_value("it's"), "\"it's\"");
        assert_eq!(quoted_value("café"), "\"café\"");
    }

    #[test]
    fn a_value_cannot_forge_a_line_of_its_own() {
        assert_eq!(
            quoted_value("ok\nerror: forged"),
            "\"ok\\nerror: forged\"",
            "a value must not be able to write a second line"
        );
        assert_eq!(quoted_value("a\r\tb"), "\"a\\r\\tb\"");
    }

    #[test]
    fn a_value_cannot_steer_the_terminal() {
        assert_eq!(quoted_value("\u{1b}[31mred"), "\"\\u{1b}[31mred\"");
        assert_eq!(quoted_value("a\u{202e}b"), "\"a\\u{202e}b\"");
    }

    #[test]
    fn a_value_cannot_close_its_own_quotes() {
        assert_eq!(quoted_value("quote\"d"), "\"quote\\\"d\"");
        assert_eq!(quoted_value("back\\slash"), "\"back\\\\slash\"");
    }

    #[test]
    fn quiet_wins_over_verbose_when_both_somehow_arrive() {
        assert_eq!(Verbosity::new(true, 2), Verbosity::Quiet);
    }

    #[test]
    fn only_quiet_suppresses_ordinary_status() {
        assert!(!Verbosity::Quiet.shows_info());
        assert!(Verbosity::Normal.shows_info());
        assert!(Verbosity::Verbose(1).shows_info());
    }

    #[test]
    fn detail_requires_enough_verbose_flags() {
        assert!(!Verbosity::Quiet.shows_detail(1));
        assert!(!Verbosity::Normal.shows_detail(1));
        assert!(Verbosity::Verbose(1).shows_detail(1));
        assert!(!Verbosity::Verbose(1).shows_detail(2));
        assert!(Verbosity::Verbose(2).shows_detail(2));
    }

    #[test]
    fn a_reporter_answers_for_detail_the_way_it_prints_it() {
        let mut reporter = Reporter::new(false);
        assert!(!reporter.shows_detail(1));
        reporter.set_verbosity(Verbosity::Verbose(1));
        assert!(reporter.shows_detail(1));
        assert!(!reporter.shows_detail(2));
        reporter.set_verbosity(Verbosity::Quiet);
        assert!(!reporter.shows_detail(1));
    }

    #[test]
    fn plain_lines_carry_no_escape_sequences() {
        let reporter = Reporter::new(false);
        assert_eq!(reporter.line(Label::Error, "boom"), "error: boom");
        assert_eq!(reporter.line(Label::Warning, "hmm"), "warning: hmm");
    }

    #[test]
    fn colored_lines_wrap_the_label_only() {
        let reporter = Reporter::new(true);
        assert_eq!(
            reporter.line(Label::Error, "boom"),
            "\x1b[1;31merror:\x1b[0m boom"
        );
        assert_eq!(
            reporter.line(Label::Warning, "hmm"),
            "\x1b[1;33mwarning:\x1b[0m hmm"
        );
    }
}
