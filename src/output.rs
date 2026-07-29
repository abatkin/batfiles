//! Diagnostic output for the CLI.
//!
//! Warnings and errors go to standard error; requested data goes to standard
//! output so it stays usable in scripts.

/// Verbosity derived from `--quiet` and repeated `--verbose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
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

    /// Whether detail at `level` (1 for `-v`, 2 for `-vv`, …) should be printed.
    /// `Quiet` never shows detail, so the two flags cannot both take effect.
    fn shows_detail(self, level: u8) -> bool {
        matches!(self, Self::Verbose(current) if current >= level)
    }
}

/// Writes diagnostics honoring the resolved verbosity and color.
#[derive(Debug)]
pub struct Reporter {
    color: bool,
    verbosity: Verbosity,
}

impl Reporter {
    pub fn new(color: bool, verbosity: Verbosity) -> Self {
        Self { color, verbosity }
    }

    /// Adopt the verbosity requested on the command line. Color is known
    /// earlier than verbosity, so a reporter starts out at `Normal` and is
    /// adjusted once the arguments have been parsed.
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

    /// Extra detail, printed only at `-v` repeated at least `level` times.
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
    fn quiet_wins_over_verbose_when_both_somehow_arrive() {
        assert_eq!(Verbosity::new(true, 2), Verbosity::Quiet);
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
        let reporter = Reporter::new(false, Verbosity::Normal);
        assert_eq!(reporter.line(Label::Error, "boom"), "error: boom");
        assert_eq!(reporter.line(Label::Warning, "hmm"), "warning: hmm");
    }

    #[test]
    fn colored_lines_wrap_the_label_only() {
        let reporter = Reporter::new(true, Verbosity::Normal);
        assert_eq!(
            reporter.line(Label::Error, "boom"),
            "\x1b[1;31merror:\x1b[0m boom"
        );
    }
}
