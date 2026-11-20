use super::*;
use jiff::SignedDuration;

fn judged_at() -> Timestamp {
    "2026-06-19T12:00:00Z".parse().expect("valid timestamp")
}

fn name(text: &str) -> VarName {
    VarName::try_from(text.to_owned()).expect("valid name")
}

fn leaf(text: &str) -> DynamicVarKey {
    DynamicVarKey {
        tree: Tree::Leaf,
        name: name(text),
    }
}

fn remote(id: &str, text: &str) -> DynamicVarKey {
    DynamicVarKey {
        tree: Tree::Remote(ItemId::try_from(id.to_owned()).expect("valid id")),
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
    let command = |state: DynamicValueState| state.describe("command");
    assert_eq!(command(DynamicValueState::Refreshed), "command");
    assert_eq!(
        DynamicValueState::Refreshed.describe("github-release"),
        "github-release"
    );
    assert_eq!(
        command(DynamicValueState::Fresh { age: hours }),
        "cached 3h ago"
    );
    assert_eq!(
        command(DynamicValueState::Retained { age: hours }),
        "command failed, cached 3h ago"
    );
    assert_eq!(
        DynamicValueState::Retained { age: hours }.describe("github-release"),
        "github-release failed, cached 3h ago"
    );
    assert_eq!(
        command(DynamicValueState::Assumed),
        "command could not start"
    );
    assert_eq!(
        command(DynamicValueState::Missing(
            MissingValueReason::CaptureFailed
        )),
        "command failed"
    );
    assert_eq!(
        command(DynamicValueState::Missing(MissingValueReason::NoCacheEntry)),
        "not cached"
    );
    assert_eq!(
        command(DynamicValueState::Missing(
            MissingValueReason::OtherDeclaration
        )),
        "cached for a different declaration"
    );
    assert_eq!(
        command(DynamicValueState::Stale { age: hours }),
        "stale, cached 3h ago"
    );
}

/// The policy matrix over real commands. Each command touches a marker,
/// which is how a test tells a cache hit from a run.
#[cfg(unix)]
mod policy {
    use super::*;
    use crate::manifest::vars::{CaptureMode, CommandSpec, GithubRepository};
    use crate::output::Verbosity;
    use std::cell::Cell;
    use tempfile::TempDir;

    const MARKER: &str = "ran";

    fn ran(dir: &TempDir) -> bool {
        dir.path().join(MARKER).exists()
    }

    fn shell(line: &str, capture: CaptureMode) -> CommandSource {
        CommandSource {
            command: CommandSpec::Shell(line.to_owned()),
            capture,
            command_timeout: None,
        }
    }

    fn succeeds(value: &str) -> CommandSource {
        shell(
            &format!("touch {MARKER}; printf %s {value}"),
            CaptureMode::Stdout,
        )
    }

    fn fails() -> CommandSource {
        shell(&format!("touch {MARKER}; exit 1"), CaptureMode::Stdout)
    }

    fn declaration<'a>(
        identity: &DynamicVarKey,
        spec: &'a CommandSource,
        dir: &'a TempDir,
    ) -> PendingDeclaration<'a> {
        PendingDeclaration {
            identity: identity.clone(),
            capture: Capture::Command {
                spec,
                cwd: dir.path(),
            },
            cache: None,
            policy: CachePolicy::Auto,
            shadowed: false,
        }
    }

    fn seed(cache: &mut DynamicVarCache, identity: &DynamicVarKey, value: &str, ago: &str) {
        cache.entries.insert(
            identity.cache_key(),
            CachedVar {
                value: value.to_owned(),
                captured_at: judged_at()
                    - SignedDuration::try_from(duration(ago).get()).expect("in range"),
                from: None,
            },
        );
    }

    fn entry(cache: &DynamicVarCache, identity: &DynamicVarKey) -> CachedVar {
        cache.entries[&identity.cache_key()].clone()
    }

    fn quiet() -> Reporter {
        let mut reporter = Reporter::new(false);
        reporter.set_verbosity(Verbosity::Quiet);
        reporter
    }

    fn resolve_at(
        declarations: &[PendingDeclaration<'_>],
        policy: CachePolicy,
        cache: &mut DynamicVarCache,
    ) -> DynamicVarResolution {
        let declarations: Vec<PendingDeclaration<'_>> = declarations
            .iter()
            .map(|declaration| PendingDeclaration {
                policy,
                ..declaration.clone()
            })
            .collect();
        resolve(&declarations, cache, judged_at, &quiet())
    }

    fn only(resolution: &DynamicVarResolution) -> &ResolvedDynamicVar {
        let [(_, resolved)] = resolution.vars.as_slice() else {
            panic!("expected one outcome, got {:?}", resolution.vars);
        };
        resolved
    }

    /// Resolve `spec` as the leaf variable `email` with the supplied cache state.
    fn one(
        spec: &CommandSource,
        seeded: Option<(&str, &str)>,
        policy: CachePolicy,
    ) -> (DynamicVarResolution, DynamicVarCache, bool) {
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
        let (resolution, _, ran) = one(&succeeds("new"), Some(("cached", "1h")), CachePolicy::Auto);
        assert_eq!(only(&resolution).value.as_deref(), Some("cached"));
        assert_eq!(
            only(&resolution).state,
            DynamicValueState::Fresh {
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
            assert_eq!(only(&resolution).state, DynamicValueState::Refreshed);
            assert_eq!(only(&resolution).value.as_deref(), Some("new"));
            assert!(resolution.changed && ran);
            assert_eq!(
                entry(&cache, &leaf("email")),
                CachedVar {
                    value: "new".to_owned(),
                    captured_at: judged_at(),
                    from: Some(Provenance::command(&succeeds("new"))),
                }
            );
        }
    }

    #[test]
    fn force_over_a_fresh_entry_runs_anyway() {
        let (resolution, _, ran) =
            one(&succeeds("new"), Some(("cached", "1m")), CachePolicy::Force);
        assert_eq!(only(&resolution).state, DynamicValueState::Refreshed);
        assert_eq!(only(&resolution).value.as_deref(), Some("new"));
        assert!(resolution.changed && ran);
    }

    #[test]
    fn never_runs_nothing_and_reports_each_state() {
        let (resolution, _, ran) = one(&succeeds("new"), Some(("old", "2d")), CachePolicy::Never);
        assert_eq!(only(&resolution).value.as_deref(), Some("old"));
        assert_eq!(
            only(&resolution).state,
            DynamicValueState::Stale {
                age: Duration::from_secs(2 * 86_400)
            }
        );
        assert!(!resolution.changed && !ran);

        let (resolution, _, ran) = one(&succeeds("new"), None, CachePolicy::Never);
        assert_eq!(only(&resolution).value, None);
        assert_eq!(
            only(&resolution).state,
            DynamicValueState::Missing(MissingValueReason::NoCacheEntry)
        );
        assert!(!ran);

        let (resolution, _, ran) =
            one(&succeeds("new"), Some(("cached", "1h")), CachePolicy::Never);
        assert!(matches!(
            only(&resolution).state,
            DynamicValueState::Fresh { .. }
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
            &[PendingDeclaration {
                shadowed: true,
                ..declaration(&email, &spec, &dir)
            }],
            CachePolicy::Force,
            &mut cache,
        );
        assert_eq!(only(&resolution).state, DynamicValueState::Shadowed);
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
                only(&resolution).state,
                DynamicValueState::Retained {
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
            only(&resolution).state,
            DynamicValueState::Missing(MissingValueReason::CaptureFailed)
        );
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn an_assumed_false_beats_a_cached_value_and_is_not_written_back() {
        let spec = CommandSource {
            command: CommandSpec::Args(vec!["batfiles-no-such-program-exists".to_owned()]),
            capture: CaptureMode::Status,
            command_timeout: None,
        };
        let (resolution, cache, _) = one(&spec, Some(("true", "2d")), CachePolicy::Auto);
        assert_eq!(only(&resolution).value.as_deref(), Some("false"));
        assert_eq!(only(&resolution).state, DynamicValueState::Assumed);
        assert!(!resolution.changed);
        assert_eq!(entry(&cache, &leaf("email")).value, "true");
    }

    /// Put an entry captured by `by` in the cache for `email`, an hour ago.
    fn seed_from(cache: &mut DynamicVarCache, by: &CommandSource) {
        seed(cache, &leaf("email"), "theirs", "1h");
        cache
            .entries
            .get_mut(&leaf("email").cache_key())
            .expect("seeded")
            .from = Some(Provenance::command(by));
    }

    #[test]
    fn an_entry_another_declaration_captured_is_captured_again() {
        let dir = TempDir::new().expect("temp dir");
        let spec = succeeds("mine");
        let email = leaf("email");
        let mut cache = DynamicVarCache::default();
        seed_from(&mut cache, &shell("printf theirs", CaptureMode::Stdout));
        let resolution = resolve_at(
            &[declaration(&email, &spec, &dir)],
            CachePolicy::Auto,
            &mut cache,
        );
        assert_eq!(only(&resolution).state, DynamicValueState::Refreshed);
        assert_eq!(only(&resolution).value.as_deref(), Some("mine"));
        assert_eq!(entry(&cache, &email).from, Some(Provenance::command(&spec)));
    }

    #[test]
    fn a_capture_mode_is_part_of_what_captured_an_entry() {
        let dir = TempDir::new().expect("temp dir");
        let spec = succeeds("mine");
        let status = CommandSource {
            capture: CaptureMode::Status,
            ..spec.clone()
        };
        let email = leaf("email");
        let mut cache = DynamicVarCache::default();
        seed_from(&mut cache, &status);
        let resolution = resolve_at(
            &[declaration(&email, &spec, &dir)],
            CachePolicy::Auto,
            &mut cache,
        );
        assert_eq!(only(&resolution).state, DynamicValueState::Refreshed);
    }

    #[test]
    fn a_failure_never_falls_back_on_another_declarations_value() {
        let dir = TempDir::new().expect("temp dir");
        let spec = fails();
        let email = leaf("email");
        let mut cache = DynamicVarCache::default();
        seed_from(&mut cache, &succeeds("theirs"));
        let resolution = resolve_at(
            &[declaration(&email, &spec, &dir)],
            CachePolicy::Auto,
            &mut cache,
        );
        assert_eq!(only(&resolution).value, None);
        assert_eq!(
            only(&resolution).state,
            DynamicValueState::Missing(MissingValueReason::CaptureFailed)
        );
        assert_eq!(entry(&cache, &email).value, "theirs");
    }

    #[test]
    fn never_reports_an_entry_another_declaration_captured() {
        let dir = TempDir::new().expect("temp dir");
        let spec = succeeds("mine");
        let email = leaf("email");
        let mut cache = DynamicVarCache::default();
        seed_from(&mut cache, &succeeds("theirs"));
        let resolution = resolve_at(
            &[declaration(&email, &spec, &dir)],
            CachePolicy::Never,
            &mut cache,
        );
        assert_eq!(only(&resolution).value, None);
        assert_eq!(
            only(&resolution).state,
            DynamicValueState::Missing(MissingValueReason::OtherDeclaration)
        );
        assert!(!ran(&dir));
    }

    #[test]
    fn an_entry_recording_nothing_serves_any_declaration() {
        let (resolution, _, ran) = one(&succeeds("new"), Some(("cached", "1h")), CachePolicy::Auto);
        assert_eq!(only(&resolution).value.as_deref(), Some("cached"));
        assert!(!ran);
    }

    #[test]
    fn a_release_with_no_host_has_no_value_whatever_is_cached() {
        let repository = GithubRepository::try_from("o/r".to_owned()).expect("valid");
        let spec = GithubReleaseSource {
            repository: repository.clone(),
            github_url: None,
            timeout: None,
        };
        let tag = leaf("tag");
        let mut cache = DynamicVarCache::default();
        seed(&mut cache, &tag, "v1", "1h");
        for policy in [CachePolicy::Auto, CachePolicy::Never] {
            let resolution = resolve_at(
                &[PendingDeclaration {
                    identity: tag.clone(),
                    capture: Capture::Lookup {
                        spec: &spec,
                        host: Err("`github-url` is broken".to_owned()),
                    },
                    cache: None,
                    policy,
                    shadowed: false,
                }],
                policy,
                &mut cache,
            );
            assert_eq!(only(&resolution).value, None);
            assert_eq!(only(&resolution).kind, "github-release");
            assert_eq!(
                only(&resolution).state,
                DynamicValueState::Missing(MissingValueReason::CaptureFailed)
            );
        }
    }
}
