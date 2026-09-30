//! Parse friendly duration strings for dynamic-variable cache and command timeouts. See
//! [duration values](../../docs/repoformat.md#duration-values).

use std::fmt;
use std::time::Duration;

use jiff::SpanRelativeTo;
use jiff::fmt::friendly::SpanParser;
use serde::Deserialize;

/// A validated, non-negative duration. Zero is valid; a field that needs a
/// positive one checks that itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct FriendlyDuration(Duration);

/// Parser for friendly durations; ISO 8601 durations are not accepted.
static PARSER: SpanParser = SpanParser::new();

impl FriendlyDuration {
    /// Validate `text` as a duration, or report why it was rejected.
    pub fn new(text: &str) -> Result<Self, DurationError> {
        let rejected = |reason| DurationError {
            candidate: text.to_owned(),
            reason,
        };
        // A `Span`, not a `SignedDuration`: the latter refuses `1d` outright.
        let span = PARSER
            .parse_span(text)
            .map_err(|_| rejected(Reason::Syntax))?;
        // Checked first, so `-1y` reports the rule true of the whole value.
        if span.is_negative() {
            return Err(rejected(Reason::Negative));
        }
        if span.get_years() != 0 || span.get_months() != 0 {
            return Err(rejected(Reason::Calendar));
        }
        span.to_duration(SpanRelativeTo::days_are_24_hours())
            .ok()
            .and_then(|signed| Duration::try_from(signed).ok())
            .map(Self)
            .ok_or_else(|| rejected(Reason::Range))
    }

    /// The duration itself.
    pub fn get(self) -> Duration {
        self.0
    }
}

/// A rejected duration string and its validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DurationError {
    candidate: String,
    reason: Reason,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reason {
    /// Not a friendly duration at all.
    Syntax,
    /// Well-formed, but negative.
    Negative,
    /// Written in months or years, which have no fixed length.
    Calendar,
    /// Too large to represent.
    Range,
}

impl fmt::Display for DurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a valid duration: ", self.candidate)?;
        // The upstream error suggests units this parser rejects.
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

impl TryFrom<String> for FriendlyDuration {
    type Error = DurationError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::new(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Wrapper {
        value: FriendlyDuration,
    }

    /// Parse a duration through a TOML field.
    fn parse(text: &str) -> Result<FriendlyDuration, toml::de::Error> {
        toml::from_str::<Wrapper>(&format!("value = '{text}'")).map(|wrapper| wrapper.value)
    }

    fn rejection(text: &str) -> String {
        match parse(text) {
            Ok(duration) => panic!("`{text}` should not be a duration, parsed as {duration:?}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn every_spelling_the_format_advertises_parses() {
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
            assert_eq!(parsed.get().as_millis(), milliseconds, "`{text}`");
        }
    }

    #[test]
    fn zero_is_a_duration() {
        assert_eq!(parse("0s").expect("zero").get(), Duration::ZERO);
    }

    #[test]
    fn a_negative_duration_is_rejected_in_both_spellings() {
        for text in ["-1h", "1h ago"] {
            let error = rejection(text);
            assert!(error.contains("cannot be negative"), "{error}");
            assert!(error.contains(&format!("`{text}`")), "{error}");
        }
    }

    #[test]
    fn calendar_units_are_rejected() {
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
        // `1M` is not a month: the friendly format refuses it as confusable
        // with minutes.
        for text in ["yesterday", "60", "1M", ""] {
            let error = rejection(text);
            assert!(error.contains("write a number and a unit"), "{error}");
        }
    }

    #[test]
    fn a_fraction_is_only_allowed_on_the_last_hours_or_smaller_unit() {
        for text in ["1.5d", "1.5w", "1.5h 30m"] {
            let error = rejection(text);
            assert!(error.contains("write a number and a unit"), "{error}");
        }
    }

    #[test]
    fn iso_8601_is_not_a_duration() {
        let error = rejection("PT1H");
        assert!(error.contains("write a number and a unit"), "{error}");
    }

    #[test]
    fn an_invalid_duration_fails_the_document_that_contains_it() {
        let error = toml::from_str::<Wrapper>("value = '-1h'\n").expect_err("negative");
        assert!(error.to_string().contains("`-1h`"), "{error}");
        assert!(error.to_string().contains("line 1"), "{error}");
    }
}
