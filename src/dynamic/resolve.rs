//! Resolving one layer's dynamic variable declarations against the cache.
//!
//! Four things live here and nothing else: **freshness**, **the cache policy**,
//! **the cache document in memory**, and **the warnings a failed refresh owes**.
//! Precedence between layers, `vars list`'s layout, and which declarations are
//! reachable at all belong elsewhere.
//!
//! Discovery in particular is not this module's. Which declarations exist
//! depends on which inclusions survive their conditions, so the resolver is
//! handed a declaration set — already deduped, already filtered to what may run,
//! each entry carrying its cache identity and working directory — and stays
//! indifferent to where the set came from. That is what lets one caller resolve
//! the leaf's declarations and the surviving remotes' declarations with the same
//! function.
#![allow(dead_code, reason = "no command dispatches to the cache resolver yet")]

use std::fmt;
use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;

use super::{Outcome, RunError, capture};
use crate::item::ItemId;
use crate::output::Reporter;
use crate::repo::{DynamicVar, FriendlyDuration};
use crate::state::{CachedVar, DynamicVarCache};
use crate::var::VarName;

/// `cache`'s default (`docs/state.md`).
///
/// A `std::time::Duration` rather than a `SignedDuration` for the same reason
/// the runner's `DEFAULT_TIMEOUT` is one: an age is unsigned by the time it is
/// compared, so the const is written in the unit it is compared in.
const DEFAULT_CACHE: Duration = Duration::from_secs(24 * 60 * 60);

/// What an age below a second reads as, and the one rendering that is a phrase
/// rather than a quantity.
const JUST_NOW: &str = "just now";

/// Resolve one layer's dynamic declarations against the cache.
///
/// `now` is read once for the instant every existing entry is judged against,
/// and once more per successful capture for that entry's `captured-at`.
/// `cache` is mutated in place; the caller writes the document, once, and only
/// if [`Resolution::changed`].
///
/// The set arrives deduped by identity. A duplicate would run its command twice
/// and last-write-wins, which is a bug in whoever built the set rather than a
/// state worth modelling here.
pub(crate) fn resolve(
    declarations: &[Declaration<'_>],
    policy: CachePolicy,
    cache: &mut DynamicVarCache,
    now: impl Fn() -> Timestamp,
    reporter: &Reporter,
) -> Resolution {
    // One instant judges every entry in the call, so a slow capture cannot make
    // a later declaration's fresh entry read as stale within the same run.
    let judged_at = now();
    let mut vars = Vec::with_capacity(declarations.len());
    let mut changed = false;

    for declaration in declarations {
        let identity = declaration.identity.clone();
        let key = identity.cache_key();
        let cached = cache.entries.get(&key).cloned();
        let cached_value = cached.as_ref().map(|entry| entry.value.clone());

        // The lazy exception short-circuits ahead of the policy rather than
        // inside it, so no policy argument can defeat it. The value is whatever
        // the cache holds, fresh or stale: this is the one outcome reporting a
        // value it did not produce.
        if declaration.shadowed {
            vars.push(Resolved {
                identity,
                value: cached_value,
                refresh: Refresh::Shadowed,
            });
            continue;
        }

        let entry = match &cached {
            Some(entry) => {
                let age = age_of(entry.captured_at, judged_at);
                if is_fresh(entry.captured_at, declaration.decl.cache, judged_at) {
                    Entry::Fresh(age)
                } else {
                    Entry::Stale(age)
                }
            }
            None => Entry::Absent,
        };

        if !runs(policy, entry) {
            vars.push(Resolved {
                identity,
                value: cached_value,
                refresh: match entry {
                    Entry::Fresh(age) => Refresh::Fresh { age },
                    Entry::Stale(age) => Refresh::Stale { age },
                    Entry::Absent => Refresh::Missing(Absence::NoCacheEntry),
                },
            });
            continue;
        }

        let resolved = match capture(declaration.decl, declaration.cwd, reporter.verbosity()) {
            Outcome::Captured(value) => {
                // A fresh read of the clock rather than `judged_at`:
                // `captured-at` is documented as when the value was captured,
                // and `cache = "500ms"` is a legal declaration, so a command
                // that outlasts its own cache duration must not be written back
                // already stale.
                cache.entries.insert(
                    key,
                    CachedVar {
                        value: value.clone(),
                        captured_at: now(),
                    },
                );
                changed = true;
                Resolved {
                    identity,
                    value: Some(value),
                    refresh: Refresh::Refreshed,
                }
            }
            // The assumed `"false"` is the runtime result even when a value is
            // cached: `docs/state.md`'s retain-a-cached-value rule is about a
            // refresh *failure*, and a command that could not be started is not
            // one. Leaving the cache alone is the point — a cached `"false"`
            // would answer "no `op` here" for a day after `op` is installed,
            // while re-attempting the spawn costs an ENOENT.
            Outcome::Assumed { value, reason } => {
                reporter.warn(&warning(&identity, &reason, Fallback::Assumed));
                Resolved {
                    identity,
                    value: Some(value),
                    refresh: Refresh::Assumed,
                }
            }
            Outcome::Failed(error) => match entry {
                Entry::Fresh(age) | Entry::Stale(age) => {
                    reporter.warn(&warning(&identity, &error, Fallback::Cached(age)));
                    Resolved {
                        identity,
                        value: cached_value,
                        refresh: Refresh::Retained { age },
                    }
                }
                Entry::Absent => {
                    reporter.warn(&warning(&identity, &error, Fallback::None));
                    Resolved {
                        identity,
                        value: None,
                        refresh: Refresh::Missing(Absence::CommandFailed),
                    }
                }
            },
        };
        vars.push(resolved);
    }

    Resolution { vars, changed }
}

/// One declaration to resolve, as the caller already holds it.
///
/// Borrowed, because the declarations and repository roots live in the loaded
/// model the caller is holding and outlive the call. The identity is owned
/// because it is computed rather than found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Declaration<'a> {
    pub identity: Identity,
    pub decl: &'a DynamicVar,
    /// The declaring repository's root — a dynamic command's working directory
    /// (`docs/environment.md`).
    pub cwd: &'a Path,
    /// `vars list` only: a machine-local value already wins, so the command is
    /// deliberately not run.
    ///
    /// A field the caller sets rather than a mode the resolver computes. The
    /// machine-local map is the caller's — `vars refresh` does not even read
    /// `vars.toml` — so handing the resolver a map it must sometimes ignore
    /// would put a policy in the one type that is supposed to have none. Every
    /// caller but `vars list` passes `false`, and is eager by construction
    /// rather than by remembering to be.
    pub shadowed: bool,
}

/// Which declaration a value came from, in both the spellings it needs.
///
/// The cache key and the name a user types are deliberately different — the
/// cache namespaces remote declarations with a `remote:` prefix, and
/// `vars refresh` rejects that syntax — so one type carries both and no call
/// site has to remember which it is holding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Identity {
    /// The declaring remote, or `None` for a leaf declaration.
    pub remote: Option<ItemId>,
    pub name: VarName,
}

impl Identity {
    /// The key this declaration's entry has in `dynamic-vars.toml`.
    pub fn cache_key(&self) -> String {
        match &self.remote {
            Some(remote) => DynamicVarCache::remote_key(remote, &self.name),
            None => DynamicVarCache::leaf_key(&self.name),
        }
    }
}

/// What a *user* types: a bare name for a leaf declaration, `<remote-id>.<name>`
/// for a remote one. Naming the cache key in a diagnostic would teach a syntax
/// that `vars refresh` then rejects.
impl fmt::Display for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.remote {
            Some(remote) => write!(f, "{remote}.{}", self.name),
            None => self.name.fmt(f),
        }
    }
}

/// How a resolve treats entries that are fresh, stale, or absent
/// (`docs/state.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CachePolicy {
    /// Use a fresh entry, run a stale or absent one. Ordinary plan-building.
    Auto,
    /// Run regardless of freshness. `--refresh-vars` and `vars refresh`.
    Force,
    /// Run nothing and write nothing. `vars list --no-refresh`.
    Never,
}

/// What one call resolved.
///
/// [`Default`] is the empty resolution, which is what a caller with no
/// declarations in a layer has: no outcomes, and nothing written. It exists so
/// skipping a layer entirely — without loading the cache — is one expression
/// rather than a call with an empty slice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Resolution {
    /// In the order the declarations arrived. The caller built the set, so any
    /// ordering here would be the printer's opinion arriving early.
    pub vars: Vec<Resolved>,
    /// Whether any entry was written. The caller's answer to "is there anything
    /// to save", and therefore to "should this file exist at all".
    ///
    /// This means an entry was *written*, not that a value differs, which is a
    /// deliberate divergence from `vars set`'s idempotence rule: a successful
    /// force refresh that captures the identical string still moves
    /// `captured-at`, and moving it is the whole point of the command.
    pub changed: bool,
}

/// One declaration's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Resolved {
    pub identity: Identity,
    /// The declaration's value, absent when nothing produced one and nothing
    /// was cached: every [`Refresh::Missing`], and a [`Refresh::Shadowed`] over
    /// an empty cache.
    pub value: Option<String>,
    pub refresh: Refresh,
}

/// How a declaration's value was arrived at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refresh {
    /// Inside its `cache` duration; no command ran.
    Fresh { age: Duration },
    /// A command ran and succeeded; the entry was written.
    Refreshed,
    /// A command ran and failed; the cached value is kept. Warned.
    ///
    /// Deliberately not `StaleRetained`: under the force policy a command runs
    /// against an entry that was *fresh*, so the value kept can be a fresh one,
    /// and the carried `age` says which.
    Retained { age: Duration },
    /// A status capture whose command could not be started, so `"false"` is
    /// assumed. Warned, used for this run, never cached.
    Assumed,
    /// No value at all. The variant carries which of the two ways.
    Missing(Absence),
    /// Never policy over a stale entry: the value is usable but out of date.
    ///
    /// The only outcome that exists solely to be displayed — it is what lets
    /// `--no-refresh` distinguish a trustworthy cached value from an
    /// out-of-date one, which is the only thing that flag is for.
    Stale { age: Duration },
    /// `vars list` only: a machine-local value wins, so nothing ran.
    Shadowed,
}

/// The two ways a declaration ends up with no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Absence {
    /// A command ran, or could not run, and nothing was cached.
    CommandFailed,
    /// The never policy found no entry.
    NoCacheEntry,
}

/// What the cache holds for one declaration: the columns of the policy matrix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    Fresh(Duration),
    Stale(Duration),
    Absent,
}

/// Whether the policy runs the command over an entry in this state.
///
/// The whole of the policy's decision surface, and `docs/state.md`'s three
/// policies as one expression.
fn runs(policy: CachePolicy, entry: Entry) -> bool {
    match (policy, entry) {
        (CachePolicy::Never, _) | (CachePolicy::Auto, Entry::Fresh(_)) => false,
        (CachePolicy::Auto, Entry::Stale(_) | Entry::Absent) | (CachePolicy::Force, _) => true,
    }
}

/// How old an entry is, floored at zero. A `captured-at` in the future is a
/// skewed clock, not a negative age.
///
/// The floor lives here rather than at each comparison so that every age
/// leaving this module is non-negative by type.
fn age_of(captured_at: Timestamp, now: Timestamp) -> Duration {
    Duration::try_from(now.duration_since(captured_at)).unwrap_or(Duration::ZERO)
}

/// Whether an entry is still inside its declaration's `cache` duration.
///
/// Strictly `<` over a non-negative age, which is what makes `cache = "0s"`
/// never fresh (`docs/repoformat.md`) — including against a `captured-at` in
/// the future, where a signed age would give `-5m < 0s` and read as fresh. A
/// future timestamp does stay fresh under any non-zero duration, deliberately:
/// the alternative is that a skewed clock re-runs every dynamic command on
/// every invocation, and the value in the cache really was captured.
fn is_fresh(captured_at: Timestamp, cache: Option<FriendlyDuration>, now: Timestamp) -> bool {
    // `unsigned_abs` is total: `FriendlyDuration` rejects negatives.
    let limit = cache.map_or(DEFAULT_CACHE, |cache| cache.as_signed().unsigned_abs());
    age_of(captured_at, now) < limit
}

/// What the run is using instead of the value it failed to capture.
///
/// An enum rather than an `Option<Duration>` because the assumed-`false` line
/// is a different sentence with a different verb, not the retained line with a
/// field missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fallback {
    /// A cached value, captured this long ago.
    Cached(Duration),
    /// Nothing: the variable has no value at all.
    None,
    /// An assumed `"false"`, for a status capture that could not start.
    Assumed,
}

/// The warning line one failed capture prints.
///
/// Pure, because [`Reporter::warn`] writes straight to standard error with no
/// seam a unit test can reach — the same shape `vars`' `describe` takes, and
/// for the same reason. [`RunError`]'s `Display` is a clause written to follow
/// a qualified name, which is exactly the position it occupies here, and the
/// fallback is the second half of the same sentence rather than a second line
/// so a per-variable failure stays one line in a list of many.
fn warning(identity: &Identity, error: &RunError, fallback: Fallback) -> String {
    match fallback {
        Fallback::Cached(cached) => {
            format!(
                "`{identity}` could not be refreshed: {error}; using the value {}",
                ago(cached)
            )
        }
        Fallback::None => {
            format!("`{identity}` could not be refreshed: {error}; no cached value to fall back on")
        }
        // `RunError::NotStarted`'s own text supplies "could not be started",
        // which is why this line has no lead-in of its own.
        Fallback::Assumed => format!("`{identity}` {error}; assuming `false` for this run"),
    }
}

/// A cache entry's age, as a warning or a listing says it: `just now`, `45s`,
/// `3h`, `2d`. Largest whole unit, truncated.
///
/// jiff's own renderings are both wrong for this position — the default is the
/// ISO 8601 `PT3H17M` and the alternate is `3h 17m 412ms` — and truncating
/// rather than rounding is deliberate: "cached 3h ago" understating an age of
/// `3h 59m` is better than "cached 4h ago" implying a capture that never
/// happened.
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

/// An age as the trailing clause of a sentence: `cached 3h ago`.
///
/// [`age`] renders the sub-second case as a phrase rather than a quantity, and
/// "cached just now ago" is not a sentence, so the two spellings are composed
/// here rather than at every caller.
fn ago(value: Duration) -> String {
    match age(value) {
        rendered if rendered == JUST_NOW => format!("cached {rendered}"),
        rendered => format!("cached {rendered} ago"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Verbosity;
    use jiff::SignedDuration;

    /// The instant every test judges its entries against.
    fn judged_at() -> Timestamp {
        "2026-06-19T12:00:00Z".parse().expect("valid timestamp")
    }

    fn name(text: &str) -> VarName {
        VarName::new(text).expect("valid name")
    }

    fn item(text: &str) -> ItemId {
        ItemId::new(text).expect("valid id")
    }

    fn leaf(text: &str) -> Identity {
        Identity {
            remote: None,
            name: name(text),
        }
    }

    fn duration(text: &str) -> FriendlyDuration {
        FriendlyDuration::new(text).expect("valid duration")
    }

    #[test]
    fn freshness_is_strict_so_an_entry_exactly_at_its_duration_is_stale() {
        // The assertion that makes the comparison `<` rather than `<=`, and
        // therefore the one that keeps `cache = "0s"` meaningful.
        let now = judged_at();
        let hour = Some(duration("1h"));
        assert!(is_fresh(now - SignedDuration::from_secs(3599), hour, now));
        assert!(!is_fresh(now - SignedDuration::from_secs(3600), hour, now));
        assert!(!is_fresh(now - SignedDuration::from_secs(3601), hour, now));
    }

    #[test]
    fn an_omitted_cache_duration_is_one_day() {
        assert_eq!(DEFAULT_CACHE, Duration::from_secs(24 * 60 * 60));
        assert_eq!(duration("1d").as_signed().unsigned_abs(), DEFAULT_CACHE);

        let now = judged_at();
        assert!(is_fresh(now - SignedDuration::from_hours(23), None, now));
        assert!(!is_fresh(now - SignedDuration::from_hours(25), None, now));
    }

    #[test]
    fn a_zero_cache_is_never_fresh_including_against_a_future_timestamp() {
        let now = judged_at();
        let zero = Some(duration("0s"));
        assert!(!is_fresh(now, zero, now));
        // The half a signed age gets wrong: `-5m < 0s` holds, so an entry
        // stamped in the future would read as fresh under a duration
        // `docs/repoformat.md` says is never fresh.
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
        // Asserted together so the two spellings cannot drift: the `remote:`
        // prefix is the cache's namespace and is never a syntax a user types.
        let remote = Identity {
            remote: Some(item("ohmyzsh")),
            name: name("email"),
        };
        assert_eq!(remote.cache_key(), "remote:ohmyzsh.email");
        assert_eq!(remote.to_string(), "ohmyzsh.email");

        assert_eq!(leaf("email").cache_key(), "email");
        assert_eq!(leaf("email").to_string(), "email");
    }

    #[test]
    fn a_warning_names_the_user_facing_key_and_what_the_run_fell_back_on() {
        let remote = Identity {
            remote: Some(item("ohmyzsh")),
            name: name("email"),
        };
        assert_eq!(
            warning(
                &remote,
                &RunError::TimedOut(Duration::from_secs(5)),
                Fallback::Cached(Duration::from_secs(3 * 60 * 60)),
            ),
            "`ohmyzsh.email` could not be refreshed: timed out after 5s; \
             using the value cached 3h ago"
        );
        assert_eq!(
            warning(
                &leaf("email"),
                &RunError::NotUtf8,
                Fallback::Cached(Duration::ZERO),
            ),
            "`email` could not be refreshed: produced output that is not valid UTF-8; \
             using the value cached just now",
            "an age that renders as a phrase does not get an `ago`"
        );
        assert_eq!(
            warning(&leaf("email"), &RunError::NotUtf8, Fallback::None),
            "`email` could not be refreshed: produced output that is not valid UTF-8; \
             no cached value to fall back on"
        );
        let absent = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No such file or directory (os error 2)",
        );
        assert_eq!(
            warning(
                &leaf("has_op"),
                &RunError::NotStarted(absent),
                Fallback::Assumed,
            ),
            "`has_op` could not be started: No such file or directory (os error 2); \
             assuming `false` for this run"
        );
    }

    #[test]
    fn an_age_is_one_truncated_unit() {
        // The test that stops jiff's `PT3H59M` — and the friendly `3h 59m` —
        // from reaching a user.
        assert_eq!(age(Duration::ZERO), "just now");
        assert_eq!(age(Duration::from_millis(999)), "just now");
        assert_eq!(age(Duration::from_secs(45)), "45s");
        assert_eq!(age(Duration::from_secs(90)), "1m");
        assert_eq!(age(Duration::from_secs(3 * 3600 + 59 * 60)), "3h");
        assert_eq!(age(Duration::from_secs(2 * 86_400 + 23 * 3600)), "2d");
    }

    /// The policy matrix over real commands, which `docs/architecture.md` names
    /// as the seam for process behavior — the same one `run.rs`'s tests use.
    #[cfg(unix)]
    mod policy {
        use super::*;
        use std::cell::Cell;
        use std::path::PathBuf;
        use tempfile::TempDir;

        use crate::repo::{Capture, CommandSpec};

        /// The file every command in these tests touches.
        ///
        /// It is the seam for "nothing ran", which no return value can prove: a
        /// resolver that ran the command and got the same string back is
        /// indistinguishable from one that used the cache, until the marker is
        /// there to look for.
        const MARKER: &str = "ran";

        fn workspace() -> TempDir {
            TempDir::new().expect("temp dir")
        }

        fn ran(dir: &TempDir) -> bool {
            dir.path().join(MARKER).exists()
        }

        fn shell(line: &str, capture: Capture) -> DynamicVar {
            DynamicVar {
                command: CommandSpec::Shell(line.to_owned()),
                capture,
                cache: None,
                command_timeout: None,
            }
        }

        /// A command that records having run and then prints `value`.
        fn succeeds(value: &str) -> DynamicVar {
            shell(
                &format!("touch {MARKER}; printf %s {value}"),
                Capture::Stdout,
            )
        }

        /// A command that records having run and then fails.
        fn fails() -> DynamicVar {
            shell(&format!("touch {MARKER}; exit 1"), Capture::Stdout)
        }

        fn declaration<'a>(
            identity: &Identity,
            decl: &'a DynamicVar,
            dir: &'a TempDir,
        ) -> Declaration<'a> {
            Declaration {
                identity: identity.clone(),
                decl,
                cwd: dir.path(),
                shadowed: false,
            }
        }

        fn shadowed<'a>(
            identity: &Identity,
            decl: &'a DynamicVar,
            dir: &'a TempDir,
        ) -> Declaration<'a> {
            Declaration {
                shadowed: true,
                ..declaration(identity, decl, dir)
            }
        }

        fn seed(cache: &mut DynamicVarCache, identity: &Identity, value: &str, ago: &str) {
            cache.entries.insert(
                identity.cache_key(),
                CachedVar {
                    value: value.to_owned(),
                    captured_at: judged_at() - duration(ago).as_signed(),
                },
            );
        }

        fn entry(cache: &DynamicVarCache, identity: &Identity) -> CachedVar {
            cache.entries[&identity.cache_key()].clone()
        }

        /// `Quiet` so a test that expects a failing command does not spray the
        /// harness's own stderr with the child's; `run.rs` does the same.
        fn resolve_with(
            declarations: &[Declaration<'_>],
            policy: CachePolicy,
            cache: &mut DynamicVarCache,
            now: impl Fn() -> Timestamp,
        ) -> Resolution {
            resolve(
                declarations,
                policy,
                cache,
                now,
                &Reporter::new(false, Verbosity::Quiet),
            )
        }

        /// The common case: a clock stopped at [`judged_at`].
        fn resolve_at(
            declarations: &[Declaration<'_>],
            policy: CachePolicy,
            cache: &mut DynamicVarCache,
        ) -> Resolution {
            resolve_with(declarations, policy, cache, judged_at)
        }

        /// The single outcome of a one-declaration resolve.
        fn only(resolution: &Resolution) -> &Resolved {
            let [resolved] = resolution.vars.as_slice() else {
                panic!("expected one outcome, got {:?}", resolution.vars);
            };
            resolved
        }

        #[test]
        fn auto_over_a_fresh_entry_uses_the_cache_and_runs_nothing() {
            let dir = workspace();
            let decl = succeeds("new");
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();
            seed(&mut cache, &email, "cached", "1h");

            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Auto,
                &mut cache,
            );

            assert_eq!(only(&resolution).value.as_deref(), Some("cached"));
            assert_eq!(
                only(&resolution).refresh,
                Refresh::Fresh {
                    age: Duration::from_secs(3600)
                }
            );
            assert!(!resolution.changed);
            assert!(!ran(&dir));
        }

        #[test]
        fn auto_over_a_stale_entry_runs_and_rewrites_it() {
            let dir = workspace();
            let decl = succeeds("new");
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();
            seed(&mut cache, &email, "old", "2d");

            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Auto,
                &mut cache,
            );

            assert_eq!(only(&resolution).refresh, Refresh::Refreshed);
            assert_eq!(only(&resolution).value.as_deref(), Some("new"));
            assert!(resolution.changed);
            assert!(ran(&dir));
            assert_eq!(
                entry(&cache, &email),
                CachedVar {
                    value: "new".to_owned(),
                    captured_at: judged_at(),
                }
            );
        }

        #[test]
        fn auto_over_an_absent_entry_runs_and_caches() {
            let dir = workspace();
            let decl = succeeds("new");
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();

            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Auto,
                &mut cache,
            );

            assert_eq!(only(&resolution).refresh, Refresh::Refreshed);
            assert!(resolution.changed);
            assert!(ran(&dir));
            assert_eq!(entry(&cache, &email).value, "new");
        }

        #[test]
        fn force_over_a_fresh_entry_runs_anyway() {
            // The one case that separates force from auto, and what
            // `vars refresh` is for.
            let dir = workspace();
            let decl = succeeds("new");
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();
            seed(&mut cache, &email, "cached", "1m");

            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Force,
                &mut cache,
            );

            assert_eq!(only(&resolution).refresh, Refresh::Refreshed);
            assert_eq!(only(&resolution).value.as_deref(), Some("new"));
            assert!(resolution.changed);
            assert!(ran(&dir));
        }

        #[test]
        fn never_over_a_stale_entry_reports_it_stale_and_leaves_it_alone() {
            let dir = workspace();
            let decl = succeeds("new");
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();
            seed(&mut cache, &email, "old", "2d");
            let before = entry(&cache, &email);

            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Never,
                &mut cache,
            );

            assert_eq!(only(&resolution).value.as_deref(), Some("old"));
            assert_eq!(
                only(&resolution).refresh,
                Refresh::Stale {
                    age: Duration::from_secs(2 * 86_400)
                }
            );
            assert!(!resolution.changed);
            assert!(!ran(&dir));
            assert_eq!(entry(&cache, &email), before);
        }

        #[test]
        fn never_reports_an_absent_entry_missing_and_a_fresh_one_fresh() {
            let dir = workspace();
            let decl = succeeds("new");
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();

            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Never,
                &mut cache,
            );
            assert_eq!(
                only(&resolution).refresh,
                Refresh::Missing(Absence::NoCacheEntry)
            );
            assert_eq!(only(&resolution).value, None);

            seed(&mut cache, &email, "cached", "1h");
            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Never,
                &mut cache,
            );
            assert_eq!(
                only(&resolution).refresh,
                Refresh::Fresh {
                    age: Duration::from_secs(3600)
                }
            );
            assert_eq!(only(&resolution).value.as_deref(), Some("cached"));
            assert!(!resolution.changed);
            assert!(!ran(&dir));
        }

        #[test]
        fn each_capture_is_stamped_with_its_own_instant() {
            // A single `Timestamp` threaded through the call would stamp both
            // entries with an instant from before either command ran, which is
            // why `cache = "500ms"` would otherwise be born stale.
            let dir = workspace();
            let decl = succeeds("x");
            let first = leaf("first");
            let second = leaf("second");
            let mut cache = DynamicVarCache::default();

            // One tick per read: the first is `judged_at`, and each capture
            // takes the next.
            let clock = Cell::new(judged_at());
            let tick = || {
                let current = clock.get();
                clock.set(current + SignedDuration::from_secs(1));
                current
            };

            let resolution = resolve_with(
                &[
                    declaration(&first, &decl, &dir),
                    declaration(&second, &decl, &dir),
                ],
                CachePolicy::Auto,
                &mut cache,
                tick,
            );

            assert!(resolution.changed);
            let earlier = entry(&cache, &first).captured_at;
            let later = entry(&cache, &second).captured_at;
            assert!(
                earlier > judged_at(),
                "{earlier} is not after the judging instant"
            );
            assert!(later > earlier, "{later} is not after {earlier}");
        }

        #[test]
        fn a_failed_refresh_retains_a_stale_cached_value() {
            // This cannot assert that a warning was printed: `Reporter::warn`
            // writes to standard error with no seam a unit test can reach, so
            // the outcome variant is the proof that the warning path was taken,
            // the text is asserted through `warning()` above, and the emission
            // itself waits for the command-level tests.
            let dir = workspace();
            let decl = fails();
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();
            seed(&mut cache, &email, "old", "2d");
            let before = entry(&cache, &email);

            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Auto,
                &mut cache,
            );

            assert!(ran(&dir));
            assert_eq!(only(&resolution).value.as_deref(), Some("old"));
            assert_eq!(
                only(&resolution).refresh,
                Refresh::Retained {
                    age: Duration::from_secs(2 * 86_400)
                }
            );
            assert!(!resolution.changed);
            assert_eq!(entry(&cache, &email), before);
        }

        #[test]
        fn a_failed_force_refresh_retains_a_value_that_was_still_fresh() {
            // `vars refresh`'s main failure path, and why the variant is
            // `Retained` rather than `StaleRetained`: the value kept here was
            // never stale.
            let dir = workspace();
            let decl = fails();
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();
            seed(&mut cache, &email, "cached", "1m");
            let before = entry(&cache, &email);

            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Force,
                &mut cache,
            );

            assert!(ran(&dir));
            assert_eq!(only(&resolution).value.as_deref(), Some("cached"));
            assert_eq!(
                only(&resolution).refresh,
                Refresh::Retained {
                    age: Duration::from_secs(60)
                }
            );
            assert!(!resolution.changed);
            assert_eq!(entry(&cache, &email), before);
        }

        #[test]
        fn a_failed_refresh_with_nothing_cached_is_missing() {
            let dir = workspace();
            let decl = fails();
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();

            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Auto,
                &mut cache,
            );

            assert!(ran(&dir));
            assert_eq!(only(&resolution).value, None);
            assert_eq!(
                only(&resolution).refresh,
                Refresh::Missing(Absence::CommandFailed)
            );
            assert!(!resolution.changed);
            assert!(cache.entries.is_empty());
        }

        #[test]
        fn an_assumed_false_beats_a_cached_value_and_is_not_written_back() {
            // The rule an implementation naturally gets backwards by preferring
            // the cache. The argument-vector form is what makes the case narrow:
            // the shell form spawns `sh` successfully and produces an ordinary
            // captured `"false"`.
            let dir = workspace();
            let decl = DynamicVar {
                command: CommandSpec::Args(vec!["batfiles-no-such-program-exists".to_owned()]),
                capture: Capture::Status,
                cache: None,
                command_timeout: None,
            };
            let has_op = leaf("has_op");
            let mut cache = DynamicVarCache::default();
            seed(&mut cache, &has_op, "true", "2d");
            let before = entry(&cache, &has_op);

            let resolution = resolve_at(
                &[declaration(&has_op, &decl, &dir)],
                CachePolicy::Auto,
                &mut cache,
            );

            assert_eq!(only(&resolution).refresh, Refresh::Assumed);
            assert_eq!(only(&resolution).value.as_deref(), Some("false"));
            assert!(!resolution.changed);
            assert_eq!(
                entry(&cache, &has_op),
                before,
                "the old value returns when the program does"
            );
        }

        #[test]
        fn a_shadowed_declaration_never_runs_under_any_policy() {
            let decl = succeeds("new");
            let email = leaf("email");

            for policy in [CachePolicy::Auto, CachePolicy::Force] {
                let dir = workspace();
                let mut cache = DynamicVarCache::default();
                seed(&mut cache, &email, "cached", "2d");

                let resolution = resolve_at(&[shadowed(&email, &decl, &dir)], policy, &mut cache);

                assert_eq!(only(&resolution).refresh, Refresh::Shadowed);
                assert_eq!(
                    only(&resolution).value.as_deref(),
                    Some("cached"),
                    "a stale cached value is still the value the cache holds"
                );
                assert!(!resolution.changed);
                assert!(!ran(&dir), "{policy:?} defeated the lazy exception");
            }

            let dir = workspace();
            let mut cache = DynamicVarCache::default();
            let resolution = resolve_at(
                &[shadowed(&email, &decl, &dir)],
                CachePolicy::Auto,
                &mut cache,
            );
            assert_eq!(only(&resolution).value, None);
            assert!(!ran(&dir));
        }

        #[test]
        fn nothing_written_means_nothing_changed() {
            // The property `vars refresh` needs in order not to create a cache
            // file — or its directory — that was not there before.
            let dir = workspace();
            let fresh_decl = succeeds("new");
            let failing = fails();
            let fresh = leaf("fresh");
            let hidden = leaf("hidden");
            let broken = leaf("broken");

            let mut cache = DynamicVarCache::default();
            seed(&mut cache, &fresh, "cached", "1h");
            seed(&mut cache, &hidden, "cached", "1h");
            seed(&mut cache, &broken, "cached", "2d");
            let before = cache.clone();

            let resolution = resolve_at(
                &[
                    declaration(&fresh, &fresh_decl, &dir),
                    shadowed(&hidden, &fresh_decl, &dir),
                    declaration(&broken, &failing, &dir),
                ],
                CachePolicy::Auto,
                &mut cache,
            );

            assert_eq!(resolution.vars.len(), 3);
            assert!(!resolution.changed);
            assert_eq!(cache, before);
        }

        #[test]
        fn an_empty_declaration_set_resolves_to_nothing() {
            // The degenerate case, and the one `vars refresh` over a leaf that
            // declares no dynamic variables actually takes.
            let mut cache = DynamicVarCache::default();
            let resolution = resolve_at(&[], CachePolicy::Force, &mut cache);

            assert!(resolution.vars.is_empty());
            assert!(!resolution.changed);
            assert_eq!(cache, DynamicVarCache::default());
        }

        #[test]
        fn an_entry_the_set_does_not_name_is_retained() {
            // The resolver sees one layer at a time and, under conditional
            // reachability, never sees the whole set, so it cannot tell an
            // orphan from a declaration a `when` excluded today.
            let dir = workspace();
            let decl = succeeds("new");
            let email = leaf("email");
            let orphan = leaf("orphan");
            let mut cache = DynamicVarCache::default();
            seed(&mut cache, &orphan, "stale", "9d");
            let before = entry(&cache, &orphan);

            resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Force,
                &mut cache,
            );

            assert_eq!(entry(&cache, &email).value, "new");
            assert_eq!(entry(&cache, &orphan), before);
        }

        /// A remote declaration resolves against the key the cache spells with
        /// its `remote:` prefix, not the bare name a leaf declaration uses.
        #[test]
        fn a_remote_declaration_reads_and_writes_its_own_namespace() {
            let dir = workspace();
            let decl = succeeds("remote-value");
            let remote = Identity {
                remote: Some(item("ohmyzsh")),
                name: name("email"),
            };
            let mut cache = DynamicVarCache::default();
            seed(&mut cache, &leaf("email"), "leaf-value", "1h");

            let resolution = resolve_at(
                &[declaration(&remote, &decl, &dir)],
                CachePolicy::Auto,
                &mut cache,
            );

            assert_eq!(only(&resolution).refresh, Refresh::Refreshed);
            assert_eq!(
                cache.entries["remote:ohmyzsh.email"].value, "remote-value",
                "the remote's capture landed in the remote's key"
            );
            assert_eq!(
                cache.entries["email"].value, "leaf-value",
                "and left the leaf's alone"
            );
        }

        /// `PathBuf` is named so the working directory a declaration carries is
        /// the one the command actually runs in.
        #[test]
        fn the_working_directory_is_the_declarations_own() {
            let dir = workspace();
            let decl = shell("touch ran; pwd", Capture::Stdout);
            let email = leaf("email");
            let mut cache = DynamicVarCache::default();

            let resolution = resolve_at(
                &[declaration(&email, &decl, &dir)],
                CachePolicy::Auto,
                &mut cache,
            );

            let captured = only(&resolution).value.clone().expect("a captured path");
            assert_eq!(
                std::fs::canonicalize(PathBuf::from(captured)).expect("the captured path exists"),
                std::fs::canonicalize(dir.path()).expect("the temp dir exists")
            );
        }
    }
}
