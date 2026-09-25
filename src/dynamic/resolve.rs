//! Resolving dynamic declarations against the cache: freshness, the cache
//! policy, what a failure falls back on, and the warning that says so.
//! See [`docs/state.md`](../../docs/state.md#freshness-and-refresh-behavior).

use std::fmt;
use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;

use super::cache::{CachedVar, DynamicVarCache};
use super::run::{CaptureError, CaptureOutcome, capture};
use crate::item::ItemId;
use crate::manifest::duration::FriendlyDuration;
use crate::manifest::vars::DynamicVarSpec;
use crate::output::Reporter;
use crate::var::VarName;

/// `cache`'s default.
const DEFAULT_CACHE: Duration = Duration::from_secs(24 * 60 * 60);

/// What an age below a second reads as.
const JUST_NOW: &str = "just now";

/// Resolve `declarations` against `cache` under `policy`, running commands the
/// policy calls for and writing each successful capture into `cache`.
///
/// `now` is read once to judge every existing entry, and again for each
/// capture's `captured-at`. Failures are warned about through `reporter`, whose
/// `--quiet` also disconnects each command's standard error. The declarations
/// must be distinct.
pub(crate) fn resolve(
    declarations: &[ScopedDeclaration<'_>],
    policy: CachePolicy,
    cache: &mut DynamicVarCache,
    now: impl Fn() -> Timestamp,
    reporter: &Reporter,
) -> Resolution {
    let judged_at = now();
    let mut resolution = Resolution::default();
    for declaration in declarations {
        let identity = &declaration.identity;
        let key = identity.cache_key();
        let cached = cache.entries.get(&key).cloned();
        let cached_value = cached.as_ref().map(|entry| entry.value.clone());

        // Ahead of the policy, so no policy can defeat the lazy exception.
        if declaration.shadowed {
            resolution.push(identity, cached_value, RefreshOutcome::Shadowed);
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

        if !runs(policy, entry) {
            let refresh = match entry {
                CacheEntryState::Fresh(age) => RefreshOutcome::Fresh { age },
                CacheEntryState::Stale(age) => RefreshOutcome::Stale { age },
                CacheEntryState::Absent => {
                    RefreshOutcome::Missing(MissingValueReason::NoCacheEntry)
                }
            };
            resolution.push(identity, cached_value, refresh);
            continue;
        }

        match capture(declaration.spec, declaration.cwd, reporter.is_quiet()) {
            CaptureOutcome::Captured(value) => {
                // A fresh reading: a command can outlast a short `cache`.
                cache.entries.insert(
                    key,
                    CachedVar {
                        value: value.clone(),
                        captured_at: now(),
                    },
                );
                resolution.changed = true;
                resolution.push(identity, Some(value), RefreshOutcome::Refreshed);
            }
            // Used even over a cached value, and never cached: a command that
            // could not be started is not a failed refresh.
            CaptureOutcome::Assumed { value, reason } => {
                reporter.warn(&warning(identity, &reason, Fallback::Assumed));
                resolution.push(identity, Some(value), RefreshOutcome::Assumed);
            }
            CaptureOutcome::Failed(error) => match entry {
                CacheEntryState::Fresh(age) | CacheEntryState::Stale(age) => {
                    reporter.warn(&warning(identity, &error, Fallback::Cached(age)));
                    resolution.push(identity, cached_value, RefreshOutcome::Retained { age });
                }
                CacheEntryState::Absent => {
                    reporter.warn(&warning(identity, &error, Fallback::None));
                    resolution.push(
                        identity,
                        None,
                        RefreshOutcome::Missing(MissingValueReason::CommandFailed),
                    );
                }
            },
        }
    }
    resolution
}

/// One declaration to resolve.
#[derive(Debug, Clone)]
pub(crate) struct ScopedDeclaration<'a> {
    pub identity: VarIdentity,
    pub spec: &'a DynamicVarSpec,
    /// The declaring repository's root: the command's working directory.
    pub cwd: &'a Path,
    /// A higher layer already wins and the command is deliberately not run.
    /// Only `vars list` sets it.
    pub shadowed: bool,
}

/// Which declaration a value came from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct VarIdentity {
    /// The declaring remote's key in the leaf's `[remotes]`, or `None` for a
    /// leaf declaration.
    pub remote: Option<ItemId>,
    pub name: VarName,
}

impl VarIdentity {
    /// The entry's key in `dynamic-vars.toml`: the bare name for a leaf
    /// declaration, `remote:<remote-id>.<name>` for a remote's.
    pub fn cache_key(&self) -> String {
        match &self.remote {
            Some(remote) => format!("remote:{remote}.{}", self.name),
            None => self.name.to_string(),
        }
    }
}

/// How a message names the declaration: `name`, or `remote.name`.
impl fmt::Display for VarIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.remote {
            Some(remote) => write!(f, "{remote}.{}", self.name),
            None => self.name.fmt(f),
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

/// What one call resolved.
#[derive(Debug, Default)]
pub(crate) struct Resolution {
    /// In the order the declarations arrived.
    pub vars: Vec<(VarIdentity, ResolvedDynamicVar)>,
    /// Whether any entry was written, even with an unchanged value: a capture
    /// always moves `captured-at`.
    pub changed: bool,
}

impl Resolution {
    fn push(&mut self, identity: &VarIdentity, value: Option<String>, refresh: RefreshOutcome) {
        self.vars
            .push((identity.clone(), ResolvedDynamicVar { value, refresh }));
    }
}

/// One declaration's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDynamicVar {
    /// `None` when nothing produced a value and nothing was cached.
    pub value: Option<String>,
    pub refresh: RefreshOutcome,
}

/// How a declaration's value was arrived at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RefreshOutcome {
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

impl RefreshOutcome {
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

/// How old an entry is. A `captured-at` in the future has no age.
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
fn warning(identity: &VarIdentity, error: &CaptureError, fallback: Fallback) -> String {
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
fn age(age: Duration) -> String {
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
mod tests {
    use super::*;
    use jiff::SignedDuration;

    fn judged_at() -> Timestamp {
        "2026-06-19T12:00:00Z".parse().expect("valid timestamp")
    }

    fn name(text: &str) -> VarName {
        VarName::try_from(text.to_owned()).expect("valid name")
    }

    fn leaf(text: &str) -> VarIdentity {
        VarIdentity {
            remote: None,
            name: name(text),
        }
    }

    fn remote(id: &str, text: &str) -> VarIdentity {
        VarIdentity {
            remote: Some(ItemId::try_from(id.to_owned()).expect("valid id")),
            name: name(text),
        }
    }

    fn duration(text: &str) -> FriendlyDuration {
        FriendlyDuration::new(text).expect("valid duration")
    }

    #[test]
    fn freshness_is_strict_so_an_entry_exactly_at_its_duration_is_stale() {
        let now = judged_at();
        let hour = Some(duration("1h"));
        assert!(is_fresh(now - SignedDuration::from_secs(3599), hour, now));
        assert!(!is_fresh(now - SignedDuration::from_secs(3600), hour, now));
    }

    #[test]
    fn an_omitted_cache_duration_is_one_day() {
        let now = judged_at();
        assert!(is_fresh(now - SignedDuration::from_hours(23), None, now));
        assert!(!is_fresh(now - SignedDuration::from_hours(25), None, now));
    }

    #[test]
    fn a_zero_cache_is_never_fresh_including_against_a_future_timestamp() {
        let now = judged_at();
        let zero = Some(duration("0s"));
        assert!(!is_fresh(now, zero, now));
        assert!(!is_fresh(now + SignedDuration::from_mins(5), zero, now));
    }

    #[test]
    fn a_future_timestamp_has_no_age_and_stays_fresh_under_a_real_duration() {
        let now = judged_at();
        let ahead = now + SignedDuration::from_mins(5);
        assert_eq!(age_of(ahead, now), Duration::ZERO);
        assert!(is_fresh(ahead, Some(duration("1h")), now));
    }

    #[test]
    fn an_identity_keys_as_the_cache_spells_it_and_displays_as_a_user_types_it() {
        assert_eq!(remote("core", "email").cache_key(), "remote:core.email");
        assert_eq!(remote("core", "email").to_string(), "core.email");
        assert_eq!(leaf("email").cache_key(), "email");
        assert_eq!(leaf("email").to_string(), "email");
    }

    #[test]
    fn a_warning_names_the_declaration_and_what_the_run_fell_back_on() {
        assert_eq!(
            warning(
                &remote("core", "email"),
                &CaptureError::TimedOut(Duration::from_secs(5)),
                Fallback::Cached(Duration::from_secs(3 * 60 * 60)),
            ),
            "dynamic variable `core.email` could not be refreshed: timed out after 5s; \
             using the value cached 3h ago"
        );
        assert_eq!(
            warning(&leaf("email"), &CaptureError::NotUtf8, Fallback::None),
            "dynamic variable `email` could not be refreshed: produced output that is not \
             valid UTF-8; it has no value, and reads as empty"
        );
        let absent = std::io::Error::new(std::io::ErrorKind::NotFound, "not found");
        assert_eq!(
            warning(
                &leaf("has_op"),
                &CaptureError::NotStarted(absent),
                Fallback::Assumed
            ),
            "dynamic variable `has_op` could not be started: not found; \
             assuming `false` for this run"
        );
    }

    #[test]
    fn an_age_is_one_truncated_unit() {
        assert_eq!(age(Duration::from_millis(999)), "just now");
        assert_eq!(age(Duration::from_secs(45)), "45s");
        assert_eq!(age(Duration::from_secs(90)), "1m");
        assert_eq!(age(Duration::from_secs(3 * 3600 + 59 * 60)), "3h");
        assert_eq!(age(Duration::from_secs(2 * 86_400 + 23 * 3600)), "2d");
        assert_eq!(ago(Duration::ZERO), "cached just now");
        assert_eq!(ago(Duration::from_secs(45)), "cached 45s ago");
    }

    #[test]
    fn each_state_describes_itself_for_a_listing() {
        let hours = Duration::from_secs(3 * 3600);
        assert_eq!(RefreshOutcome::Refreshed.describe(), "command");
        assert_eq!(
            RefreshOutcome::Fresh { age: hours }.describe(),
            "cached 3h ago"
        );
        assert_eq!(
            RefreshOutcome::Retained { age: hours }.describe(),
            "command failed, cached 3h ago"
        );
        assert_eq!(
            RefreshOutcome::Assumed.describe(),
            "command could not start"
        );
        assert_eq!(
            RefreshOutcome::Missing(MissingValueReason::CommandFailed).describe(),
            "command failed"
        );
        assert_eq!(
            RefreshOutcome::Missing(MissingValueReason::NoCacheEntry).describe(),
            "not cached"
        );
        assert_eq!(
            RefreshOutcome::Stale { age: hours }.describe(),
            "stale, cached 3h ago"
        );
    }

    /// The policy matrix over real commands. Each command touches a marker,
    /// which is how a test tells a cache hit from a run.
    #[cfg(unix)]
    mod policy {
        use super::*;
        use crate::manifest::vars::{CaptureMode, CommandSpec};
        use crate::output::Verbosity;
        use std::cell::Cell;
        use tempfile::TempDir;

        const MARKER: &str = "ran";

        fn ran(dir: &TempDir) -> bool {
            dir.path().join(MARKER).exists()
        }

        fn shell(line: &str, capture: CaptureMode) -> DynamicVarSpec {
            DynamicVarSpec {
                command: CommandSpec::Shell(line.to_owned()),
                capture,
                cache: None,
                command_timeout: None,
            }
        }

        fn succeeds(value: &str) -> DynamicVarSpec {
            shell(
                &format!("touch {MARKER}; printf %s {value}"),
                CaptureMode::Stdout,
            )
        }

        fn fails() -> DynamicVarSpec {
            shell(&format!("touch {MARKER}; exit 1"), CaptureMode::Stdout)
        }

        fn declaration<'a>(
            identity: &VarIdentity,
            spec: &'a DynamicVarSpec,
            dir: &'a TempDir,
        ) -> ScopedDeclaration<'a> {
            ScopedDeclaration {
                identity: identity.clone(),
                spec,
                cwd: dir.path(),
                shadowed: false,
            }
        }

        fn seed(cache: &mut DynamicVarCache, identity: &VarIdentity, value: &str, ago: &str) {
            cache.entries.insert(
                identity.cache_key(),
                CachedVar {
                    value: value.to_owned(),
                    captured_at: judged_at()
                        - SignedDuration::try_from(duration(ago).get()).expect("in range"),
                },
            );
        }

        fn entry(cache: &DynamicVarCache, identity: &VarIdentity) -> CachedVar {
            cache.entries[&identity.cache_key()].clone()
        }

        fn quiet() -> Reporter {
            let mut reporter = Reporter::new(false);
            reporter.set_verbosity(Verbosity::Quiet);
            reporter
        }

        fn resolve_at(
            declarations: &[ScopedDeclaration<'_>],
            policy: CachePolicy,
            cache: &mut DynamicVarCache,
        ) -> Resolution {
            resolve(declarations, policy, cache, judged_at, &quiet())
        }

        fn only(resolution: &Resolution) -> &ResolvedDynamicVar {
            let [(_, resolved)] = resolution.vars.as_slice() else {
                panic!("expected one outcome, got {:?}", resolution.vars);
            };
            resolved
        }

        /// One declaration of `spec` as `email`, over a cache seeded as asked.
        fn one(
            spec: &DynamicVarSpec,
            seeded: Option<(&str, &str)>,
            policy: CachePolicy,
        ) -> (Resolution, DynamicVarCache, bool) {
            let dir = TempDir::new().expect("temp dir");
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();
            if let Some((value, ago)) = seeded {
                seed(&mut cache, &email, value, ago);
            }
            let resolution = resolve_at(&[declaration(&email, spec, &dir)], policy, &mut cache);
            let ran = ran(&dir);
            (resolution, cache, ran)
        }

        #[test]
        fn auto_over_a_fresh_entry_uses_the_cache_and_runs_nothing() {
            let (resolution, _, ran) =
                one(&succeeds("new"), Some(("cached", "1h")), CachePolicy::Auto);
            assert_eq!(only(&resolution).value.as_deref(), Some("cached"));
            assert_eq!(
                only(&resolution).refresh,
                RefreshOutcome::Fresh {
                    age: Duration::from_secs(3600)
                }
            );
            assert!(!resolution.changed);
            assert!(!ran);
        }

        #[test]
        fn auto_over_a_stale_or_absent_entry_runs_and_writes_it() {
            for seeded in [Some(("old", "2d")), None] {
                let (resolution, cache, ran) = one(&succeeds("new"), seeded, CachePolicy::Auto);
                assert_eq!(only(&resolution).refresh, RefreshOutcome::Refreshed);
                assert_eq!(only(&resolution).value.as_deref(), Some("new"));
                assert!(resolution.changed && ran);
                assert_eq!(
                    entry(&cache, &leaf("email")),
                    CachedVar {
                        value: "new".to_owned(),
                        captured_at: judged_at(),
                    }
                );
            }
        }

        #[test]
        fn force_over_a_fresh_entry_runs_anyway() {
            let (resolution, _, ran) =
                one(&succeeds("new"), Some(("cached", "1m")), CachePolicy::Force);
            assert_eq!(only(&resolution).refresh, RefreshOutcome::Refreshed);
            assert_eq!(only(&resolution).value.as_deref(), Some("new"));
            assert!(resolution.changed && ran);
        }

        #[test]
        fn never_runs_nothing_and_reports_each_state() {
            let (resolution, _, ran) =
                one(&succeeds("new"), Some(("old", "2d")), CachePolicy::Never);
            assert_eq!(only(&resolution).value.as_deref(), Some("old"));
            assert_eq!(
                only(&resolution).refresh,
                RefreshOutcome::Stale {
                    age: Duration::from_secs(2 * 86_400)
                }
            );
            assert!(!resolution.changed && !ran);

            let (resolution, _, ran) = one(&succeeds("new"), None, CachePolicy::Never);
            assert_eq!(only(&resolution).value, None);
            assert_eq!(
                only(&resolution).refresh,
                RefreshOutcome::Missing(MissingValueReason::NoCacheEntry)
            );
            assert!(!ran);

            let (resolution, _, ran) =
                one(&succeeds("new"), Some(("cached", "1h")), CachePolicy::Never);
            assert!(matches!(
                only(&resolution).refresh,
                RefreshOutcome::Fresh { .. }
            ));
            assert!(!ran);
        }

        #[test]
        fn a_shadowed_declaration_runs_nothing_under_any_policy() {
            let dir = TempDir::new().expect("temp dir");
            let spec = succeeds("new");
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();
            let resolution = resolve_at(
                &[ScopedDeclaration {
                    shadowed: true,
                    ..declaration(&email, &spec, &dir)
                }],
                CachePolicy::Force,
                &mut cache,
            );
            assert_eq!(only(&resolution).refresh, RefreshOutcome::Shadowed);
            assert!(!ran(&dir));
        }

        #[test]
        fn each_capture_is_stamped_with_its_own_instant() {
            let dir = TempDir::new().expect("temp dir");
            let spec = succeeds("x");
            let (first, second) = (leaf("first"), leaf("second"));
            let mut cache = DynamicVarCache::default();
            let clock = Cell::new(judged_at());
            let tick = || {
                let current = clock.get();
                clock.set(current + SignedDuration::from_secs(1));
                current
            };
            resolve(
                &[
                    declaration(&first, &spec, &dir),
                    declaration(&second, &spec, &dir),
                ],
                CachePolicy::Auto,
                &mut cache,
                tick,
                &quiet(),
            );
            let earlier = entry(&cache, &first).captured_at;
            let later = entry(&cache, &second).captured_at;
            assert!(earlier > judged_at());
            assert!(later > earlier);
        }

        #[test]
        fn a_failed_refresh_retains_a_cached_value_fresh_or_stale() {
            for (policy, ago, seconds) in [
                (CachePolicy::Auto, "2d", 2 * 86_400),
                (CachePolicy::Force, "1m", 60),
            ] {
                let (resolution, cache, ran) = one(&fails(), Some(("old", ago)), policy);
                assert!(ran);
                assert_eq!(only(&resolution).value.as_deref(), Some("old"));
                assert_eq!(
                    only(&resolution).refresh,
                    RefreshOutcome::Retained {
                        age: Duration::from_secs(seconds)
                    }
                );
                assert!(!resolution.changed);
                assert_eq!(entry(&cache, &leaf("email")).value, "old");
            }
        }

        #[test]
        fn a_failed_refresh_with_nothing_cached_has_no_value() {
            let (resolution, cache, ran) = one(&fails(), None, CachePolicy::Auto);
            assert!(ran);
            assert_eq!(only(&resolution).value, None);
            assert_eq!(
                only(&resolution).refresh,
                RefreshOutcome::Missing(MissingValueReason::CommandFailed)
            );
            assert!(cache.entries.is_empty());
        }

        #[test]
        fn an_assumed_false_beats_a_cached_value_and_is_not_written_back() {
            let spec = DynamicVarSpec {
                command: CommandSpec::Args(vec!["batfiles-no-such-program-exists".to_owned()]),
                capture: CaptureMode::Status,
                cache: None,
                command_timeout: None,
            };
            let (resolution, cache, _) = one(&spec, Some(("true", "2d")), CachePolicy::Auto);
            assert_eq!(only(&resolution).value.as_deref(), Some("false"));
            assert_eq!(only(&resolution).refresh, RefreshOutcome::Assumed);
            assert!(!resolution.changed);
            assert_eq!(entry(&cache, &leaf("email")).value, "true");
        }
    }
}
