//! How batfiles words what it did, and where it says it.

use crate::mode::RunMode;

/// One act an action reports, in whichever tense the mode calls for.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Verb {
    Link,
    Relink,
    Copy,
    Fetch,
    Extract,
    Clone,
    Update,
    SwitchRef,
    Create,
    Remove,
    Keep,
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
            Self::Extract => "extract",
            Self::Clone => "clone",
            Self::Update => "update",
            Self::SwitchRef => "switch",
            Self::Create => "create",
            Self::Remove => "remove",
            Self::Keep => "keep",
        }
    }

    /// What an action did, or — under [`RunMode::DryRun`] — would do.
    pub fn say(self, mode: RunMode) -> String {
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
            Self::Extract => "extracted",
            Self::Clone => "cloned",
            Self::Update => "updated",
            Self::SwitchRef => "switched",
            Self::Create => "created",
            Self::Remove => "removed",
            Self::Keep => "kept",
        }
    }
}

/// A value batfiles is repeating back, quoted and made safe to print.
///
/// Everything batfiles has to show that it did not choose the wording of goes
/// through this: a variable's value, and the environment variable names it
/// warns about. Both come from outside — a repository, a hand-edited state
/// file, the ambient environment — and a raw one can end a line and start a
/// convincing `error:` of its own, or carry the escape sequences that move a
/// terminal's cursor around.
///
/// Control characters, backslashes, and the surrounding quote are escaped the
/// way Rust's own debug output escapes them, so the result is one line. The
/// apostrophe is left alone: it needs no escaping inside double quotes, and
/// `don't` is worth more than the consistency of writing `don\'t`. Printable
/// text of any script passes through as itself.
///
/// The quotes are always written, which is what keeps an empty value visible as
/// `""` rather than as a gap where a value should be.
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

    /// Whether ordinary status output should be printed. Only `--quiet`
    /// suppresses it; `-v` adds detail rather than replacing this.
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
    /// Start at normal verbosity. Color is settled before the arguments are
    /// parsed, so a reporter exists — and can warn about its own inputs —
    /// before `--verbose` and `--quiet` have been read.
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

    /// What a command did, printed at normal verbosity and suppressed by
    /// `--quiet`.
    ///
    /// This is a diagnostic rather than requested data: it says what happened
    /// instead of answering a question, so it goes to standard error.
    pub fn info(&self, message: &str) {
        if self.verbosity.shows_info() {
            eprintln!("{message}");
        }
    }

    /// Data the user asked for. Printed to standard output, unlabeled and
    /// uncolored, and never gated by verbosity: `--quiet` suppresses what a
    /// command *did*, not what it was *asked for*.
    ///
    /// Nothing is added around the value, so `$(batfiles vars get editor)`
    /// yields the stored string and nothing else.
    pub fn data(&self, message: &str) {
        println!("{message}");
    }

    /// Extra detail, printed only at `-v` repeated at least `level` times.
    /// Unlabeled: it elaborates on what a command is doing rather than
    /// reporting a problem.
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
        assert_eq!(Verb::Link.say(RunMode::Perform), "linked");
        assert_eq!(Verb::Link.say(RunMode::DryRun), "would link");
        // The irregular ones, which is why `past` is a table rather than a
        // suffix.
        assert_eq!(Verb::Copy.say(RunMode::Perform), "copied");
        assert_eq!(Verb::Keep.say(RunMode::Perform), "kept");
    }

    #[test]
    fn an_ordinary_value_is_quoted_and_otherwise_left_alone() {
        assert_eq!(quoted_value("nvim"), "\"nvim\"");
        assert_eq!(quoted_value(""), "\"\"");
        assert_eq!(quoted_value("it's"), "\"it's\"");
        // Printable text of any script is the value, not a threat.
        assert_eq!(quoted_value("café"), "\"café\"");
    }

    #[test]
    fn a_value_cannot_forge_a_line_of_its_own() {
        // The point of the escaping: a newline in a value would otherwise end
        // the line it is printed on and start one that reads like batfiles'.
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
        // Non-printable and direction-changing characters go the same way.
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
