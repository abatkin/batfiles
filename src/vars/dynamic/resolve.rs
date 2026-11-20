//! Resolve dynamic declarations using cache freshness and refresh policy, with warnings and
//! fallback values on failure. See [cache
//! behavior](../../../docs/state.md#freshness-and-refresh-behavior).

use std::fmt;
use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;

use super::cache::{CachedVar, DynamicVarCache};
use super::command::{CaptureError, CaptureOutcome, capture};
use crate::manifest::duration::FriendlyDuration;
use crate::manifest::item::ItemId;
use crate::manifest::remote::Tree;
use crate::manifest::vars::DynamicVarSpec;
use crate::output::Reporter;
use crate::vars::name::VarName;

/// `cache`'s default.
const DEFAULT_CACHE: Duration = Duration::from_secs(24 * 60 * 60);

/// What an age below a second reads as.
const JUST_NOW: &str = "just now";

/// Resolve distinct declarations, running commands as required by policy and updating `cache`
/// with successful captures.
///
/// Call `now` once to check cache freshness and again for each capture timestamp. Report
/// failures through `reporter`; quiet mode also suppresses child stderr.
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
        let key = identity.cache_key();
        let cached = cache.entries.get(&key).cloned();
        let cached_value = cached.as_ref().map(|entry| entry.value.clone());

        // Shadowed listing declarations never run, even under a force policy.
        if declaration.shadowed {
            resolution.push(identity, cached_value, DynamicValueState::Shadowed);
            continue;
        }

        let entry = match &cached {
            Some(entry) => {
                let age = age_of(entry.captured_at, judged_at);
                if is_fresh(entry.captured_at, declaration.spec.cache, judged_at) {
                    CacheEntryState::Fresh(age)
                } else {
                    CacheEntryState::Stale(age)
                }
            }
            None => CacheEntryState::Absent,
        };

        if !runs(declaration.policy, entry) {
            let state = match entry {
                CacheEntryState::Fresh(age) => DynamicValueState::Fresh { age },
                CacheEntryState::Stale(age) => DynamicValueState::Stale { age },
                CacheEntryState::Absent => {
                    DynamicValueState::Missing(MissingValueReason::NoCacheEntry)
                }
            };
            resolution.push(identity, cached_value, state);
            continue;
        }

        match capture(declaration.spec, declaration.cwd, reporter.is_quiet()) {
            CaptureOutcome::Captured(value) => {
                // Timestamp the capture after execution; commands can outlast the cache
                // duration.
                cache.entries.insert(
                    key,
                    CachedVar {
                        value: value.clone(),
                        captured_at: now(),
                    },
                );
                resolution.changed = true;
                resolution.push(identity, Some(value), DynamicValueState::Refreshed);
            }
            // A failed status-command launch overrides cached values for this run, without
            // updating the cache.
            CaptureOutcome::Assumed { value, reason } => {
                reporter.warn(&warning(identity, &reason, Fallback::Assumed));
                resolution.push(identity, Some(value), DynamicValueState::Assumed);
            }
            CaptureOutcome::Failed(error) => match entry {
                CacheEntryState::Fresh(age) | CacheEntryState::Stale(age) => {
                    reporter.warn(&warning(identity, &error, Fallback::Cached(age)));
                    resolution.push(identity, cached_value, DynamicValueState::Retained { age });
                }
                CacheEntryState::Absent => {
                    reporter.warn(&warning(identity, &error, Fallback::None));
                    resolution.push(
                        identity,
                        None,
                        DynamicValueState::Missing(MissingValueReason::CommandFailed),
                    );
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
    pub spec: &'a DynamicVarSpec,
    /// The declaring repository's root: the command's working directory.
    pub cwd: &'a Path,
    /// How this declaration treats its cache entry.
    pub policy: CachePolicy,
    /// Whether a higher-priority value prevents this command from running during `vars list`.
    pub shadowed: bool,
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
    fn push(&mut self, identity: &DynamicVarKey, value: Option<String>, state: DynamicValueState) {
        self.vars
            .push((identity.clone(), ResolvedDynamicVar { value, state }));
    }
}

/// One declaration's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDynamicVar {
    /// `None` when nothing produced a value and nothing was cached.
    pub value: Option<String>,
    pub state: DynamicValueState,
}

/// How a dynamic variable's value was obtained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DynamicValueState {
    /// Inside its `cache` duration; nothing ran.
    Fresh { age: Duration },
    /// Ran and succeeded; the entry was written.
    Refreshed,
    /// Ran and failed; the cached value, fresh or stale, is kept.
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
    /// How a listing describes the state, after the declaration's origin.
    pub fn describe(&self) -> String {
        match self {
            Self::Fresh { age } => ago(*age),
            Self::Refreshed => "command".to_owned(),
            Self::Retained { age } => format!("command failed, {}", ago(*age)),
            Self::Assumed => "command could not start".to_owned(),
            Self::Missing(MissingValueReason::CommandFailed) => "command failed".to_owned(),
            Self::Missing(MissingValueReason::NoCacheEntry) => "not cached".to_owned(),
            Self::Stale { age } => format!("stale, {}", ago(*age)),
            Self::Shadowed => "not run".to_owned(),
        }
    }
}

/// The two ways a declaration ends up with no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MissingValueReason {
    /// A command ran, or could not run, and nothing was cached.
    CommandFailed,
    /// The never policy found no entry.
    NoCacheEntry,
}

/// What the cache holds for one declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheEntryState {
    Fresh(Duration),
    Stale(Duration),
    Absent,
}

/// Whether `policy` runs the command over an entry in this state.
fn runs(policy: CachePolicy, entry: CacheEntryState) -> bool {
    match (policy, entry) {
        (CachePolicy::Never, _) | (CachePolicy::Auto, CacheEntryState::Fresh(_)) => false,
        (CachePolicy::Auto, CacheEntryState::Stale(_) | CacheEntryState::Absent)
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
fn warning(identity: &DynamicVarKey, error: &CaptureError, fallback: Fallback) -> String {
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
