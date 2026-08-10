//! The duration behind `cache` and `command-timeout`.
//!
//! Both fields are written as a TOML string, and jiff's "friendly" format
//! defines what may be written in one. That format is wider than a batfiles
//! duration, so three things are rejected here rather than left to whatever
//! eventually interprets the value: a negative duration, a calendar unit with no
//! fixed length, and anything the grammar does not recognize at all. Rejecting
//! them at deserialize time is what puts a file and a line number on the
//! diagnostic, which `docs/state.md:233` requires — parsing and schema
//! validation happen before any input precedence or dynamic command runs.

use std::fmt;

use jiff::fmt::friendly::SpanParser;
use jiff::{SignedDuration, SpanRelativeTo};
use serde::{Deserialize, Serialize, Serializer};

/// A validated duration.
///
/// Only the parsed duration is kept, not the text it was written as, so `1d` and
/// `24h` are one value and both render as `24h`. This follows
/// [`RepoPath`](super::RepoPath), which canonicalizes the `@remote/path`
/// shorthand for the same stated reason: batfiles never writes a manifest back,
/// so there is no spelling worth preserving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct FriendlyDuration(SignedDuration);

/// The friendly format alone: `Span`'s own `FromStr` would also accept ISO 8601,
/// and the friendly format is the one the repository format is defined in.
static PARSER: SpanParser = SpanParser::new();

impl FriendlyDuration {
    /// Validate `text` as a duration, or report why it was rejected.
    pub fn new(text: &str) -> Result<Self, DurationError> {
        let rejected = |reason| DurationError {
            candidate: text.to_owned(),
            reason,
        };

        // A `SignedDuration` cannot be the parse target: parsing a calendar unit
        // into one is a hard error, and `1d` — `cache`'s own default — is a
        // calendar unit as far as the parser is concerned. So parse a `Span` and
        // convert.
        let span = PARSER
            .parse_span(text)
            .map_err(|_| rejected(Reason::Syntax))?;
        // The sign is tested first, so a value that breaks both rules — `-1y` —
        // reports the one that is true of the whole duration.
        if span.is_negative() {
            return Err(rejected(Reason::Negative));
        }
        if span.get_years() != 0 || span.get_months() != 0 {
            return Err(rejected(Reason::Calendar));
        }
        // Weeks and days need no reference date because the format defines a day
        // as 24 hours and a week as 7 days (`docs/repoformat.md`), which is
        // exactly what this relative-to setting means. jiff applies the rule
        // rather than us.
        span.to_duration(SpanRelativeTo::days_are_24_hours())
            .map(Self)
            .map_err(|_| rejected(Reason::Range))
    }

    /// The duration itself: the `now - captured-at < cache` freshness test and
    /// the subprocess timeout both want a real duration, not this wrapper.
    pub fn as_signed(self) -> SignedDuration {
        self.0
    }
}

/// Why a candidate duration was rejected.
///
/// The rejected text travels with the error because serde renders the error
/// verbatim and the message has to name what the user actually wrote.
///
/// jiff's own message is deliberately not wrapped: it names the "friendly
/// duration format" and suggests units — years, months — that batfiles rejects,
/// so re-emitting it would advertise a wider grammar than the one accepted here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DurationError {
    candidate: String,
    reason: Reason,
}

/// The four ways a duration string fails, kept apart because each one points at
/// a different mistake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    /// Not a friendly duration at all.
    Syntax,
    /// A well-formed duration, but a negative one.
    Negative,
    /// Written in months or years, which have no fixed length.
    Calendar,
    /// Well-formed and in range for a `Span`, but not for a duration.
    Range,
}

impl fmt::Display for DurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a valid duration: ", self.candidate)?;
        f.write_str(match self.reason {
            Reason::Syntax => "write a number and a unit, such as `30s`, `1h 30m`, or `1d`",
            Reason::Negative => "a duration cannot be negative",
            Reason::Calendar => {
                "months and years have no fixed length, so a duration is written in weeks or smaller"
            }
            Reason::Range => "it is too large to represent",
        })
    }
}

impl std::error::Error for DurationError {}

impl fmt::Display for FriendlyDuration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The alternate form is jiff's friendly rendering; the plain one is
        // ISO 8601, which the format does not accept.
        write!(f, "{:#}", self.0)
    }
}

impl TryFrom<String> for FriendlyDuration {
    type Error = DurationError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(&text)
    }
}

impl Serialize for FriendlyDuration {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A duration only ever appears as a field, and a bare value is not a TOML
    /// document, so the tests wrap it in one.
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Wrapper {
        value: FriendlyDuration,
    }

    /// Through TOML rather than through [`FriendlyDuration::new`]: deserializing
    /// is the seam every one of these values actually crosses.
    fn parse(text: &str) -> Result<FriendlyDuration, toml::de::Error> {
        toml::from_str::<Wrapper>(&format!("value = '{text}'")).map(|wrapper| wrapper.value)
    }

    fn rejection(text: &str) -> String {
        match parse(text) {
            Ok(duration) => panic!("`{text}` should not be a duration, parsed as {duration}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn every_spelling_the_format_advertises_parses() {
        // Milliseconds so the sub-second units are covered by the same table.
        // `1d` and `1w` are also the assertion that a day is 24 hours and a week
        // is 7 days.
        for (text, milliseconds) in [
            ("30s", 30_000),
            ("5m", 300_000),
            ("1h", 3_600_000),
            ("1d", 86_400_000),
            ("1w", 604_800_000),
            ("1h 30m", 5_400_000),
            ("90 minutes", 5_400_000),
            ("2 hrs", 7_200_000),
            ("3 mins, 30 secs", 210_000),
            ("1.5h", 5_400_000),
            ("1m 30.5s", 90_500),
            ("01:30:00", 5_400_000),
            ("500ms", 500),
        ] {
            let parsed = parse(text).unwrap_or_else(|error| panic!("`{text}`: {error}"));
            assert_eq!(parsed.as_signed().as_millis(), milliseconds, "`{text}`");
        }
    }

    #[test]
    fn zero_is_a_duration() {
        // Only negatives are rejected. A zero `cache` means "never fresh, always
        // refresh", which is a coherent thing to ask for.
        assert_eq!(parse("0s").expect("zero").as_signed(), SignedDuration::ZERO);
    }

    #[test]
    fn a_negative_duration_is_rejected_in_both_spellings() {
        // `cache = "-1h"` is the case this type exists for: it used to parse.
        for text in ["-1h", "1h ago"] {
            let error = rejection(text);
            assert!(error.contains("cannot be negative"), "{error}");
            assert!(error.contains(&format!("`{text}`")), "{error}");
        }
    }

    #[test]
    fn calendar_units_are_rejected() {
        // The friendly format accepts these; a duration compares two instants,
        // so batfiles does not.
        for text in ["1mo", "1y", "1 month"] {
            let error = rejection(text);
            assert!(
                error.contains("months and years have no fixed length"),
                "{error}"
            );
        }
    }

    #[test]
    fn nonsense_is_rejected() {
        // `1M` is not a month: the friendly format eschews it as confusable with
        // minutes, and nothing here widens the grammar to add it.
        for text in ["yesterday", "60", "1M", ""] {
            let error = rejection(text);
            assert!(error.contains("write a number and a unit"), "{error}");
        }
    }

    #[test]
    fn a_fraction_is_only_allowed_on_the_last_hours_or_smaller_unit() {
        // This test *is* the narrowed spec: the restriction is the friendly
        // grammar's, and this is the only place it is written down in code.
        for text in ["1.5d", "1.5w", "1.5h 30m"] {
            let error = rejection(text);
            assert!(error.contains("write a number and a unit"), "{error}");
        }
    }

    #[test]
    fn iso_8601_is_not_a_duration() {
        // Locks in the friendly-only parser: `Span`'s `FromStr` would take this.
        let error = rejection("PT1H");
        assert!(error.contains("write a number and a unit"), "{error}");
    }

    #[test]
    fn an_invalid_duration_fails_the_document_that_contains_it() {
        let error = toml::from_str::<Wrapper>("value = '-1h'\n").expect_err("negative");
        assert!(error.to_string().contains("`-1h`"), "{error}");
        assert!(error.to_string().contains("line 1"), "{error}");
    }

    #[test]
    fn a_duration_serializes_as_the_duration_it_means() {
        // Nothing writes a manifest, so `1d` coming back as `24h` costs nothing
        // and keeps one value with one spelling.
        let day = parse("1d").expect("one day");
        let document = toml::to_string(&Wrapper { value: day }).expect("serialize");

        assert_eq!(document, "value = \"24h\"\n");
        assert_eq!(
            toml::from_str::<Wrapper>(&document).expect("reparse").value,
            day
        );
    }
}
