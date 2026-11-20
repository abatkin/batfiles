//! Resolve dynamic declarations using cache freshness, provenance, and refresh policy, with
//! warnings and fallback values on failure. See [cache
//! behavior](../../../docs/state.md#freshness-and-refresh-behavior).

use std::fmt;
use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;

use super::cache::{CachedVar, DynamicVarCache, Provenance};
use super::command::{self, CaptureError, CaptureOutcome};
use super::lookup::{self, LookupError};
use crate::manifest::duration::FriendlyDuration;
use crate::manifest::item::ItemId;
use crate::manifest::remote::Tree;
use crate::manifest::vars::{CommandSource, GithubReleaseSource};
use crate::output::Reporter;
use crate::vars::name::VarName;

/// `cache`'s default.
const DEFAULT_CACHE: Duration = Duration::from_secs(24 * 60 * 60);

/// What an age below a second reads as.
const JUST_NOW: &str = "just now";

/// Resolve distinct declarations, capturing values as required by policy and updating `cache`
/// with successful captures.
///
/// An entry recorded for a different declaration counts as absent. Call `now` once to check
/// cache freshness and again for each capture timestamp. Report failures through `reporter`;
/// quiet mode also suppresses child stderr.
pub(crate) fn resolve(
    declarations: &[PendingDeclaration<'_>],
    cache: &mut DynamicVarCache,
    now: impl Fn() -> Timestamp,
    reporter: &Reporter,
) -> DynamicVarResolution {
    let judged_at = now();
    let mut resolution = DynamicVarResolution::default();
    for declaration in declarations {
        let identity = &declaration.identity;
        let kind = declaration.capture.kind();
        let key = identity.cache_key();
        let cached = cache.entries.get(&key).cloned();

        // Shadowed listing declarations never run, even under a force policy.
        if declaration.shadowed {
            let value = cached.map(|entry| entry.value);
            resolution.push(identity, kind, value, DynamicValueState::Shadowed);
            continue;
        }

        let provenance = match declaration.capture.provenance() {
            Ok(provenance) => provenance,
            // With no host to compare, no entry is this declaration's.
            Err(error) => {
                reporter.warn(&warning(identity, &error, Fallback::None));
                let state = DynamicValueState::Missing(MissingValueReason::CaptureFailed);
                resolution.push(identity, kind, None, state);
                continue;
            }
        };
        let (entry, cached) = match cached {
            Some(entry) if entry.from.as_ref().is_none_or(|from| *from == provenance) => {
                let age = age_of(entry.captured_at, judged_at);
                let state = if is_fresh(entry.captured_at, declaration.cache, judged_at) {
                    CacheEntryState::Fresh(age)
                } else {
                    CacheEntryState::Stale(age)
                };
                (state, Some(entry.value))
            }
            Some(_) => (CacheEntryState::OtherDeclaration, None),
            None => (CacheEntryState::Absent, None),
        };

        if !runs(declaration.policy, entry) {
            let state = match entry {
                CacheEntryState::Fresh(age) => DynamicValueState::Fresh { age },
                CacheEntryState::Stale(age) => DynamicValueState::Stale { age },
                CacheEntryState::Absent => {
                    DynamicValueState::Missing(MissingValueReason::NoCacheEntry)
                }
                CacheEntryState::OtherDeclaration => {
                    DynamicValueState::Missing(MissingValueReason::OtherDeclaration)
                }
            };
            resolution.push(identity, kind, cached, state);
            continue;
        }

        match declaration.capture.run(reporter.is_quiet()) {
            Outcome::Captured(value) => {
                // Timestamp the capture after it completes; a capture can outlast the cache
                // duration.
                cache.entries.insert(
                    key,
                    CachedVar {
                        value: value.clone(),
                        captured_at: now(),
                        from: Some(provenance),
                    },
                );
                resolution.changed = true;
                resolution.push(identity, kind, Some(value), DynamicValueState::Refreshed);
            }
            // A failed status-command launch overrides cached values for this run, without
            // updating the cache.
            Outcome::Assumed { value, reason } => {
                reporter.warn(&warning(identity, &reason, Fallback::Assumed));
                resolution.push(identity, kind, Some(value), DynamicValueState::Assumed);
            }
            Outcome::Failed(error) => match entry {
                CacheEntryState::Fresh(age) | CacheEntryState::Stale(age) => {
                    reporter.warn(&warning(identity, &error, Fallback::Cached(age)));
                    let state = DynamicValueState::Retained { age };
                    resolution.push(identity, kind, cached, state);
                }
                CacheEntryState::Absent | CacheEntryState::OtherDeclaration => {
                    reporter.warn(&warning(identity, &error, Fallback::None));
                    let state = DynamicValueState::Missing(MissingValueReason::CaptureFailed);
                    resolution.push(identity, kind, None, state);
                }
            },
        }
    }
    resolution
}

/// One declaration to resolve.
#[derive(Debug, Clone)]
pub(crate) struct PendingDeclaration<'a> {
    pub identity: DynamicVarKey,
    pub capture: Capture<'a>,
    /// The declaration's `cache` duration.
    pub cache: Option<FriendlyDuration>,
    /// How this declaration treats its cache entry.
    pub policy: CachePolicy,
    /// Whether a higher-priority value prevents this declaration from capturing during
    /// `vars list`.
    pub shadowed: bool,
}

/// How one declaration captures its value.
#[derive(Debug, Clone)]
pub(crate) enum Capture<'a> {
    /// Run a command in `cwd`, the declaring repository's root.
    Command {
        spec: &'a CommandSource,
        cwd: &'a Path,
    },
    /// Look up a release, asking `host`: the resolved `github-url`, or why it has none.
    Lookup {
        spec: &'a GithubReleaseSource,
        host: Result<String, String>,
    },
}

impl Capture<'_> {
    /// The field naming the source, as listings and messages call it.
    fn kind(&self) -> &'static str {
        match self {
            Self::Command { .. } => "command",
            Self::Lookup { .. } => "github-release",
        }
    }

    /// What an entry captured by this declaration records, or why there is nothing to record.
    fn provenance(&self) -> Result<Provenance, Failure> {
        match self {
            Self::Command { spec, .. } => Ok(Provenance::command(spec)),
            Self::Lookup {
                spec,
                host: Ok(host),
            } => Ok(Provenance::release(&spec.repository, host)),
            Self::Lookup {
                host: Err(error), ..
            } => Err(Failure::Host(error.clone())),
        }
    }

    /// Capture the value. `quiet` suppresses a command's stderr.
    fn run(&self, quiet: bool) -> Outcome {
        match self {
            Self::Command { spec, cwd } => match command::capture(spec, cwd, quiet) {
                CaptureOutcome::Captured(value) => Outcome::Captured(value),
                CaptureOutcome::Assumed { value, reason } => Outcome::Assumed { value, reason },
                CaptureOutcome::Failed(error) => Outcome::Failed(Failure::Command(error)),
            },
            Self::Lookup {
                host: Err(error), ..
            } => Outcome::Failed(Failure::Host(error.clone())),
            Self::Lookup {
                spec,
                host: Ok(host),
            } => {
                let timeout = spec
                    .timeout
                    .map_or(lookup::DEFAULT_TIMEOUT, FriendlyDuration::get);
                match lookup::lookup(&spec.repository, host, timeout) {
                    Ok(tag) => Outcome::Captured(tag),
                    Err(error) => Outcome::Failed(Failure::Lookup(error)),
                }
            }
        }
    }
}

/// What one capture produced.
enum Outcome {
    Captured(String),
    /// A status command that could not start: `value` for this run, never cached.
    Assumed {
        value: String,
        reason: CaptureError,
    },
    Failed(Failure),
}

/// Why a declaration produced no value. Displays as a clause following its name.
#[derive(Debug)]
enum Failure {
    Command(CaptureError),
    Lookup(LookupError),
    /// `github-url` failed to evaluate or resolved to an invalid URL.
    Host(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Command(error) => error.fmt(f),
            Self::Lookup(error) => error.fmt(f),
            Self::Host(error) => f.write_str(error),
        }
    }
}

/// Which declaration a value came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct DynamicVarKey {
    /// The declaring tree: the leaf, or a remote by its key in the leaf's `[remotes]`.
    pub tree: Tree<ItemId>,
    pub name: VarName,
}

impl DynamicVarKey {
    /// The entry's key in `dynamic-vars.toml`: the bare name for a leaf
    /// declaration, `remote:<remote-id>.<name>` for a remote's.
    pub fn cache_key(&self) -> String {
        match &self.tree {
            Tree::Remote(remote) => format!("remote:{remote}.{}", self.name),
            Tree::Leaf => self.name.to_string(),
        }
    }
}

/// How a message names the declaration: `name`, or `remote.name`.
impl fmt::Display for DynamicVarKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.tree {
            Tree::Remote(remote) => write!(f, "{remote}.{}", self.name),
            Tree::Leaf => self.name.fmt(f),
        }
    }
}

/// How entries that are fresh, stale, or absent are treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CachePolicy {
    /// Use a fresh entry; run a stale or absent one.
    Auto,
    /// Run regardless of freshness.
    Force,
    /// Run nothing and write nothing.
    Never,
}

/// Resolved variables and whether the cache changed.
#[derive(Debug, Default)]
pub(crate) struct DynamicVarResolution {
    /// In the order the declarations arrived.
    pub vars: Vec<(DynamicVarKey, ResolvedDynamicVar)>,
    /// Whether any cache entry was updated, including captures with unchanged values.
    pub changed: bool,
}

impl DynamicVarResolution {
    fn push(
        &mut self,
        identity: &DynamicVarKey,
        kind: &'static str,
        value: Option<String>,
        state: DynamicValueState,
    ) {
        self.vars
            .push((identity.clone(), ResolvedDynamicVar { value, kind, state }));
    }
}

/// One declaration's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDynamicVar {
    /// `None` when nothing produced a value and nothing was cached.
    pub value: Option<String>,
    /// The field naming the declaration's source: `command` or `github-release`.
    pub kind: &'static str,
    pub state: DynamicValueState,
}

impl ResolvedDynamicVar {
    /// How a listing describes the value, after the declaration's origin.
    pub fn describe(&self) -> String {
        self.state.describe(self.kind)
    }
}

/// How a dynamic variable's value was obtained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DynamicValueState {
    /// Inside its `cache` duration; nothing ran.
    Fresh { age: Duration },
    /// Captured; the entry was written.
    Refreshed,
    /// The capture failed; the cached value, fresh or stale, is kept.
    Retained { age: Duration },
    /// A status capture that could not be started, so `"false"` for this run.
    Assumed,
    /// No value.
    Missing(MissingValueReason),
    /// The never policy over a stale entry.
    Stale { age: Duration },
    /// A higher layer wins, so nothing ran.
    Shadowed,
}

impl DynamicValueState {
    /// How a listing describes the state of a declaration whose source is `kind`, after the
    /// declaration's origin.
    fn describe(&self, kind: &str) -> String {
        match self {
            Self::Fresh { age } => ago(*age),
            Self::Refreshed => kind.to_owned(),
            Self::Retained { age } => format!("{kind} failed, {}", ago(*age)),
            Self::Assumed => format!("{kind} could not start"),
            Self::Missing(MissingValueReason::CaptureFailed) => format!("{kind} failed"),
            Self::Missing(MissingValueReason::NoCacheEntry) => "not cached".to_owned(),
            Self::Missing(MissingValueReason::OtherDeclaration) => {
                "cached for a different declaration".to_owned()
            }
            Self::Stale { age } => format!("stale, {}", ago(*age)),
            Self::Shadowed => "not run".to_owned(),
        }
    }
}

/// The ways a declaration ends up with no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissingValueReason {
    /// A capture failed, or could not be made, and nothing usable was cached.
    CaptureFailed,
    /// The never policy found no entry.
    NoCacheEntry,
    /// The never policy found only an entry another declaration captured.
    OtherDeclaration,
}

/// What the cache holds for one declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheEntryState {
    Fresh(Duration),
    Stale(Duration),
    Absent,
    /// An entry recorded for a different declaration, which counts as absent.
    OtherDeclaration,
}

/// Whether `policy` captures over an entry in this state.
fn runs(policy: CachePolicy, entry: CacheEntryState) -> bool {
    match (policy, entry) {
        (CachePolicy::Never, _) | (CachePolicy::Auto, CacheEntryState::Fresh(_)) => false,
        (
            CachePolicy::Auto,
            CacheEntryState::Stale(_) | CacheEntryState::Absent | CacheEntryState::OtherDeclaration,
        )
        | (CachePolicy::Force, _) => true,
    }
}

/// Return the cache entry's age, clamped to zero for future timestamps.
fn age_of(captured_at: Timestamp, now: Timestamp) -> Duration {
    Duration::try_from(now.duration_since(captured_at)).unwrap_or(Duration::ZERO)
}

/// Whether an entry is strictly inside its `cache` duration, so `0s` is never
/// fresh.
fn is_fresh(captured_at: Timestamp, cache: Option<FriendlyDuration>, now: Timestamp) -> bool {
    let limit = cache.map_or(DEFAULT_CACHE, FriendlyDuration::get);
    age_of(captured_at, now) < limit
}

/// What the run uses instead of the value it failed to capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fallback {
    Cached(Duration),
    None,
    Assumed,
}

/// The warning one failed capture prints.
fn warning(identity: &DynamicVarKey, error: &dyn fmt::Display, fallback: Fallback) -> String {
    match fallback {
        Fallback::Cached(age) => format!(
            "dynamic variable `{identity}` could not be refreshed: {error}; using the value {}",
            ago(age)
        ),
        Fallback::None => format!(
            "dynamic variable `{identity}` could not be refreshed: {error}; \
             it has no value, and reads as empty"
        ),
        Fallback::Assumed => {
            format!("dynamic variable `{identity}` {error}; assuming `false` for this run")
        }
    }
}

/// An age in its largest whole unit, truncated: `just now`, `45s`, `3h`, `2d`.
pub(crate) fn age(age: Duration) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    let seconds = age.as_secs();
    if seconds < 1 {
        JUST_NOW.to_owned()
    } else if seconds < MINUTE {
        format!("{seconds}s")
    } else if seconds < HOUR {
        format!("{}m", seconds / MINUTE)
    } else if seconds < DAY {
        format!("{}h", seconds / HOUR)
    } else {
        format!("{}d", seconds / DAY)
    }
}

/// `cached 3h ago`, or `cached just now`.
fn ago(value: Duration) -> String {
    match age(value) {
        rendered if rendered == JUST_NOW => format!("cached {rendered}"),
        rendered => format!("cached {rendered} ago"),
    }
}

#[cfg(test)]
mod tests;
