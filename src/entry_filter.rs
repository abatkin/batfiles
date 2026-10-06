//! `include` and `exclude`: glob patterns choosing which entries of a tree an action installs,
//! and `executable`, choosing which of an archive's files are made executable. Entries are
//! named by their `/`-separated path from the tree's root. See [entry
//! filters](../docs/repoformat.md#entry-filters).

use std::ffi::OsStr;
use std::fmt;
use std::path::Path;

use globset::{Candidate, GlobBuilder, GlobMatcher};
use serde::de::{SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use thiserror::Error;

use crate::output::Reporter;

/// One glob, checked and compiled as the manifest is read. It is matched a `/`-separated
/// segment at a time, so nothing but `**` reaches past one segment; matching is
/// case-sensitive, and `?` and classes match one byte.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Pattern {
    written: String,
    segments: Vec<Segment>,
}

/// One `/`-separated segment of a [`Pattern`].
#[derive(Debug, Clone)]
enum Segment {
    /// `**`: any number of whole path segments, none included.
    AnyDepth,
    /// A glob matched against exactly one path segment.
    One(GlobMatcher),
}

/// Why a written pattern was refused.
#[derive(Debug, Error)]
pub(crate) enum PatternError {
    #[error("a pattern is empty; write `*` to match every entry")]
    Empty,

    #[error(
        "pattern `{0}` starts with `/`; patterns are already matched from the root of what \
         they filter, so write it without the leading `/`"
    )]
    Anchored(String),

    #[error(
        "pattern `{0}` has an empty, `.`, or `..` segment, which no entry's path has; \
         write each segment as a name or a glob, and no trailing `/`"
    )]
    Segment(String),

    #[error("pattern `{written}` is not a glob batfiles can read: {source}")]
    Glob {
        written: String,
        source: globset::Error,
    },
}

impl TryFrom<String> for Pattern {
    type Error = PatternError;

    fn try_from(written: String) -> Result<Self, Self::Error> {
        if written.is_empty() {
            return Err(PatternError::Empty);
        }
        if written.starts_with('/') {
            return Err(PatternError::Anchored(written));
        }
        if written
            .split('/')
            .any(|segment| matches!(segment, "" | "." | ".."))
        {
            return Err(PatternError::Segment(written));
        }
        // A class or alternative containing `/` is split here, and refused as unclosed.
        let segments = written
            .split('/')
            .map(|segment| {
                if segment == "**" {
                    return Ok(Segment::AnyDepth);
                }
                GlobBuilder::new(segment)
                    .backslash_escape(false)
                    .build()
                    .map(|glob| Segment::One(glob.compile_matcher()))
            })
            .collect::<Result<_, _>>();
        match segments {
            Ok(segments) => Ok(Self { written, segments }),
            Err(source) => Err(PatternError::Glob { written, source }),
        }
    }
}

impl Pattern {
    /// The pattern as the manifest wrote it.
    pub fn as_str(&self) -> &str {
        &self.written
    }

    /// Whether the pattern matches the path whose segments are `path`.
    fn matches(&self, path: &[&OsStr]) -> bool {
        matches_segments(&self.segments, path)
    }
}

/// Whether `pattern` matches `path` segment for segment, `**` taking any number of them.
fn matches_segments(pattern: &[Segment], path: &[&OsStr]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((Segment::AnyDepth, rest)) => {
            (0..=path.len()).any(|skipped| matches_segments(rest, &path[skipped..]))
        }
        Some((Segment::One(glob), rest)) => match path.split_first() {
            Some((name, below)) => {
                glob.is_match_candidate(&Candidate::new(Path::new(name)))
                    && matches_segments(rest, below)
            }
            None => false,
        },
    }
}

/// The value of an `include` or `exclude` field: one pattern or a list of them.
#[derive(Debug, Clone)]
pub(crate) struct GlobFilter(Vec<Pattern>);

impl GlobFilter {
    /// The patterns as written, in the order they were written.
    pub fn as_slice(&self) -> &[Pattern] {
        &self.0
    }

    /// Whether some pattern contains `/`, and so could only match below a direct child.
    pub fn first_nested(&self) -> Option<&Pattern> {
        self.0.iter().find(|pattern| pattern.written.contains('/'))
    }
}

impl<'de> Deserialize<'de> for GlobFilter {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct GlobFilterVisitor;

        impl<'de> Visitor<'de> for GlobFilterVisitor {
            type Value = GlobFilter;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a glob or a list of globs")
            }

            /// Parse a single pattern as a one-pattern list.
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<GlobFilter, E> {
                let pattern = Pattern::try_from(value.to_owned()).map_err(E::custom)?;
                Ok(GlobFilter(vec![pattern]))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<GlobFilter, A::Error> {
                let mut patterns = Vec::with_capacity(seq.size_hint().unwrap_or_default());
                while let Some(pattern) = seq.next_element()? {
                    patterns.push(pattern);
                }
                Ok(GlobFilter(patterns))
            }
        }

        deserializer.deserialize_any(GlobFilterVisitor)
    }
}

/// What a filter decides about one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// Installed: it or an ancestor is included, and neither is excluded.
    Selected,
    /// Not installed, and nothing beneath it can be.
    Excluded,
    /// Not installed itself, though something beneath it may be.
    NotIncluded,
}

/// One action's `include` and `exclude`, applied to entry paths relative to the root they
/// filter. Remembers which patterns have matched something, for [`Self::report_unmatched`].
#[derive(Debug)]
pub(crate) struct EntryFilter<'a> {
    include: Option<&'a [Pattern]>,
    exclude: &'a [Pattern],
    /// One flag per include pattern, then one per exclude pattern.
    matched: Vec<bool>,
}

impl<'a> EntryFilter<'a> {
    /// The filter the two fields describe, or `None` where neither is written.
    pub fn new(include: Option<&'a GlobFilter>, exclude: Option<&'a GlobFilter>) -> Option<Self> {
        if include.is_none() && exclude.is_none() {
            return None;
        }
        let include = include.map(GlobFilter::as_slice);
        let exclude = exclude.map(GlobFilter::as_slice).unwrap_or_default();
        Some(Self {
            matched: vec![false; include.unwrap_or_default().len() + exclude.len()],
            include,
            exclude,
        })
    }

    /// Decide the entry at `path`, a relative path whose components are the entry's
    /// segments. An absent `include` includes everything; an exclude beats an include; a
    /// pattern matching an ancestor decides for everything beneath it.
    pub fn verdict(&mut self, path: &Path) -> Verdict {
        let segments: Vec<&OsStr> = path.iter().collect();
        let (include_matched, exclude_matched) = self
            .matched
            .split_at_mut(self.include.unwrap_or_default().len());
        let included = self
            .include
            .is_none_or(|include| matches_at_or_above(include, &segments, include_matched));
        let excluded = matches_at_or_above(self.exclude, &segments, exclude_matched);
        match (excluded, included) {
            (true, _) => Verdict::Excluded,
            (false, true) => Verdict::Selected,
            (false, false) => Verdict::NotIncluded,
        }
    }

    /// Whether the entry at `path` is installed. See [`Self::verdict`].
    pub fn selects(&mut self, path: &Path) -> bool {
        self.verdict(path) == Verdict::Selected
    }

    /// Say at `-v` which patterns matched none of the entries decided so far, naming the tree
    /// they filtered as `within`.
    pub fn report_unmatched(&self, within: &str, reporter: &Reporter) {
        let include = self.include.unwrap_or_default();
        let fields = include
            .iter()
            .map(|pattern| ("include", pattern))
            .chain(self.exclude.iter().map(|pattern| ("exclude", pattern)));
        for ((field, pattern), matched) in fields.zip(&self.matched) {
            if !matched {
                reporter.detail(
                    1,
                    &format!(
                        "{field} pattern `{}` matched nothing in {within}",
                        pattern.written
                    ),
                );
            }
        }
    }
}

/// One action's `executable`, applied to the paths of the files a tree installs, relative to
/// its root. Remembers which patterns have marked a file, for [`Self::report_unmatched`].
#[derive(Debug)]
pub(crate) struct Executable<'a> {
    patterns: &'a [Pattern],
    matched: Vec<bool>,
}

impl<'a> Executable<'a> {
    /// The marks the field describes, or `None` where it is not written.
    pub fn new(executable: Option<&'a GlobFilter>) -> Option<Self> {
        let patterns = executable?.as_slice();
        Some(Self {
            patterns,
            matched: vec![false; patterns.len()],
        })
    }

    /// Whether the file at `path` is made executable: a pattern matches it or a directory
    /// holding it.
    pub fn marks(&mut self, path: &Path) -> bool {
        let segments: Vec<&OsStr> = path.iter().collect();
        matches_at_or_above(self.patterns, &segments, &mut self.matched)
    }

    /// Say at `-v` which patterns marked none of the files decided so far, naming the tree
    /// they were matched in as `within`.
    pub fn report_unmatched(&self, within: &str, reporter: &Reporter) {
        for (pattern, matched) in self.patterns.iter().zip(&self.matched) {
            if !matched {
                reporter.detail(
                    1,
                    &format!(
                        "executable pattern `{}` matched no file in {within}",
                        pattern.written
                    ),
                );
            }
        }
    }
}

/// Whether a pattern matches the path whose segments are `segments` or one of the
/// directories holding it, flagging in `matched` each pattern that does.
fn matches_at_or_above(patterns: &[Pattern], segments: &[&OsStr], matched: &mut [bool]) -> bool {
    let mut any = false;
    for depth in 1..=segments.len() {
        for (pattern, matched) in patterns.iter().zip(matched.iter_mut()) {
            if pattern.matches(&segments[..depth]) {
                *matched = true;
                any = true;
            }
        }
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(patterns: &[&str]) -> GlobFilter {
        GlobFilter(
            patterns
                .iter()
                .map(|it| Pattern::try_from((*it).to_owned()).expect("a valid pattern"))
                .collect(),
        )
    }

    fn verdict(include: Option<&[&str]>, exclude: Option<&[&str]>, path: &str) -> Verdict {
        let include = include.map(filter);
        let exclude = exclude.map(filter);
        EntryFilter::new(include.as_ref(), exclude.as_ref())
            .expect("a filter")
            .verdict(Path::new(path))
    }

    #[test]
    fn no_fields_is_no_filter() {
        assert!(EntryFilter::new(None, None).is_none());
    }

    #[test]
    fn a_pattern_is_anchored_at_the_root() {
        let include = Some(&["*.toml"][..]);
        assert_eq!(verdict(include, None, "a.toml"), Verdict::Selected);
        assert_eq!(verdict(include, None, "dir/a.toml"), Verdict::NotIncluded);
        let anywhere = Some(&["**/*.toml"][..]);
        assert_eq!(verdict(anywhere, None, "a.toml"), Verdict::Selected);
        assert_eq!(verdict(anywhere, None, "dir/sub/a.toml"), Verdict::Selected);
    }

    #[test]
    fn a_pattern_matching_a_directory_decides_its_subtree() {
        let include = Some(&["bin"][..]);
        assert_eq!(verdict(include, None, "bin/tool"), Verdict::Selected);
        assert_eq!(verdict(include, None, "bin/sub/tool"), Verdict::Selected);
        assert_eq!(verdict(include, None, "binary"), Verdict::NotIncluded);
        let exclude = Some(&["private"][..]);
        assert_eq!(verdict(None, exclude, "private/key"), Verdict::Excluded);
        assert_eq!(verdict(None, exclude, "public/key"), Verdict::Selected);
    }

    #[test]
    fn an_exclude_beats_an_include_at_any_depth() {
        assert_eq!(
            verdict(Some(&["bin"]), Some(&["bin/secret"]), "bin/secret/key"),
            Verdict::Excluded
        );
        assert_eq!(
            verdict(Some(&["bin/tool"]), Some(&["bin"]), "bin/tool"),
            Verdict::Excluded
        );
    }

    #[test]
    fn a_star_matches_a_leading_dot_and_stays_in_its_segment() {
        assert_eq!(verdict(Some(&["*"]), None, ".hidden"), Verdict::Selected);
        assert_eq!(verdict(None, Some(&["*rc"]), "a/zshrc"), Verdict::Selected);
        assert_eq!(verdict(None, Some(&["*rc"]), ".zshrc"), Verdict::Excluded);
    }

    #[test]
    fn a_class_never_matches_a_separator() {
        for pattern in ["a[!x]b", "a[^x]b", "a?b"] {
            assert_eq!(
                verdict(Some(&[pattern]), None, "a/b"),
                Verdict::NotIncluded,
                "`{pattern}`"
            );
            assert_eq!(verdict(Some(&[pattern]), None, "a-b"), Verdict::Selected);
        }
        assert_eq!(verdict(None, Some(&["a[!x]b"]), "a/b/c"), Verdict::Selected);
    }

    #[test]
    fn a_double_star_takes_any_number_of_whole_segments() {
        let include = Some(&["a/**/z"][..]);
        assert_eq!(verdict(include, None, "a/z"), Verdict::Selected);
        assert_eq!(verdict(include, None, "a/b/c/z"), Verdict::Selected);
        assert_eq!(verdict(include, None, "a/bz"), Verdict::NotIncluded);
        assert_eq!(verdict(include, None, "b/a/z"), Verdict::NotIncluded);
        assert_eq!(verdict(Some(&["**"]), None, "a/b"), Verdict::Selected);
    }

    #[test]
    fn a_question_mark_or_a_class_matches_one_byte_and_a_star_any_run_of_them() {
        // The documented limit: `é` is two bytes in UTF-8.
        assert_eq!(
            verdict(Some(&["?.txt"]), None, "é.txt"),
            Verdict::NotIncluded
        );
        assert_eq!(
            verdict(Some(&["[é].txt"]), None, "é.txt"),
            Verdict::NotIncluded
        );
        assert_eq!(verdict(Some(&["*.txt"]), None, "é.txt"), Verdict::Selected);
        assert_eq!(verdict(Some(&["é.txt"]), None, "é.txt"), Verdict::Selected);
    }

    #[test]
    fn an_empty_include_selects_nothing() {
        assert_eq!(verdict(Some(&[]), None, "anything"), Verdict::NotIncluded);
    }

    #[test]
    fn matching_is_case_sensitive() {
        assert_eq!(
            verdict(Some(&["README"]), None, "readme"),
            Verdict::NotIncluded
        );
    }

    #[test]
    fn a_pattern_remembers_whether_it_matched() {
        let include = filter(&["bin", "lib"]);
        let exclude = filter(&["*.md"]);
        let mut entries = EntryFilter::new(Some(&include), Some(&exclude)).expect("a filter");
        entries.verdict(Path::new("bin/tool"));
        assert_eq!(entries.matched, [true, false, false]);
    }

    #[test]
    fn an_executable_pattern_marks_what_it_matches_and_what_is_under_it() {
        let patterns = filter(&["bin", "lib/*.so", "doc"]);
        let mut executable = Executable::new(Some(&patterns)).expect("marks");
        assert!(executable.marks(Path::new("bin/tool")));
        assert!(executable.marks(Path::new("bin/sub/tool")));
        assert!(executable.marks(Path::new("lib/libtool.so")));
        assert!(!executable.marks(Path::new("lib/libtool.a")));
        assert!(!executable.marks(Path::new("README.md")));
        assert_eq!(executable.matched, [true, true, false]);
        assert!(Executable::new(None).is_none());
    }

    #[test]
    fn a_pattern_no_entry_could_have_is_refused() {
        for (written, expected) in [
            ("", "is empty"),
            ("/bin", "starts with `/`"),
            ("bin/", "empty, `.`, or `..` segment"),
            ("a//b", "empty, `.`, or `..` segment"),
            ("./bin", "empty, `.`, or `..` segment"),
            ("../bin", "empty, `.`, or `..` segment"),
            ("[bin", "not a glob"),
            ("a[!/]b", "not a glob"),
            ("{a,b/c}", "not a glob"),
        ] {
            let error = Pattern::try_from(written.to_owned()).expect_err(written);
            assert!(error.to_string().contains(expected), "`{written}`: {error}");
        }
    }

    #[test]
    fn a_filter_is_one_pattern_or_a_list() {
        #[derive(Deserialize)]
        struct Fields {
            include: GlobFilter,
        }
        let one: Fields = toml::from_str("include = \"bin\"").expect("one pattern");
        assert_eq!(one.include.as_slice()[0].as_str(), "bin");
        let many: Fields = toml::from_str("include = [\"bin\", \"lib\"]").expect("a list");
        assert_eq!(many.include.as_slice().len(), 2);
    }
}
