use super::*;

const EXAMPLE: &str = r#"
generation = 4

[actions.mise]
last-changed = 4

[actions.mise-install]
last-success = "2026-06-19T12:00:00Z"
success-generation = 3

[actions."corp.rebuild-cache"]
last-changed = 2

[remotes.mise]
last-changed = 4
"#;

/// The earlier layout: `run` actions keyed directly by address.
const EARLIER: &str = r#"
[mise-install]
last-success = "2026-06-19T12:00:00Z"

["corp.rebuild-cache"]
last-success = "2026-06-19T12:00:00Z"
pending = true

[generation]
pending = true
"#;

fn parse(document: &str) -> Result<RunHistory, toml::de::Error> {
    toml::from_str::<Stored>(document).map(|stored| stored.0)
}

fn address(text: &str) -> ItemAddress {
    ItemAddress::try_from(text.to_owned()).expect("a valid address")
}

fn id(text: &str) -> ItemId {
    ItemId::try_from(text.to_owned()).expect("a valid ID")
}

fn at(text: &str) -> Timestamp {
    text.parse().expect("a valid timestamp")
}

#[test]
fn actions_and_remotes_keep_separate_records() {
    let history = parse(EXAMPLE).expect("parse");
    assert_eq!(history.generation, 4);
    assert_eq!(
        history.action(&address("mise-install")),
        ActionEntry {
            last_success: Some(at("2026-06-19T12:00:00Z")),
            success_generation: Some(3),
            last_changed: None,
        }
    );
    assert_eq!(history.action(&address("mise")).last_changed, Some(4));
    assert_eq!(
        history.action(&address("corp.rebuild-cache")).last_changed,
        Some(2)
    );
    assert_eq!(history.remote_changed(&id("mise")), Some(4));
    assert_eq!(history.remote_changed(&id("mise-install")), None);
}

#[test]
fn an_absent_entry_never_succeeded_or_changed() {
    let history = parse(EXAMPLE).expect("parse");
    assert_eq!(history.action(&address("other")), ActionEntry::default());
    assert_eq!(history.remote_changed(&id("other")), None);
}

#[test]
fn the_earlier_layout_keeps_its_successes_and_its_pending_firings() {
    let history = parse(EARLIER).expect("parse");
    assert_eq!(history.generation, 0);
    assert_eq!(
        history.action(&address("mise-install")),
        ActionEntry {
            last_success: Some(at("2026-06-19T12:00:00Z")),
            success_generation: Some(0),
            last_changed: None,
        }
    );
    // A pending firing keeps its time for `once` and `every`, and no generation for `after`.
    assert_eq!(
        history.action(&address("corp.rebuild-cache")),
        ActionEntry {
            last_success: Some(at("2026-06-19T12:00:00Z")),
            success_generation: None,
            last_changed: None,
        }
    );
    // A table named `generation` is an action in the earlier layout.
    assert_eq!(
        history.action(&address("generation")),
        ActionEntry::default()
    );
}

#[test]
fn unknown_fields_invalid_keys_and_local_datetimes_are_invalid() {
    let error = parse("generation = 1\n[actions.a]\nruns = 3\n").expect_err("closed record");
    assert!(
        error.to_string().contains("unknown field `runs`"),
        "{error}"
    );
    let error = parse("generation = 1\npending = true\n").expect_err("closed document");
    assert!(
        error.to_string().contains("unknown field `pending`"),
        "{error}"
    );
    let error =
        parse("generation = 1\n[actions.\"not an address\"]\nlast-changed = 1\n").expect_err("key");
    assert!(error.to_string().contains("not a valid address"), "{error}");
    let error = parse("[a]\npending = true\nruns = 3\n").expect_err("closed earlier record");
    assert!(
        error.to_string().contains("unknown field `runs`"),
        "{error}"
    );
    assert!(parse("generation = 'one'\n").is_err());
    assert!(parse("generation = -1\n").is_err());
    assert!(parse("generation = 1\n[actions.a]\nlast-success = 'yesterday'\n").is_err());
    assert!(parse("generation = 1\n[actions.a]\nlast-success = 2026-06-19T12:00:00\n").is_err());
    assert!(parse("[a]\npending = 'yes'\n").is_err());
}

#[test]
fn a_success_is_a_string_or_an_offset_datetime() {
    let written = |time: &str| {
        parse(&format!(
            "generation = 1\n[actions.a]\nlast-success = {time}\n"
        ))
        .expect("parse")
        .action(&address("a"))
        .last_success
    };
    assert_eq!(
        written("2026-06-19T12:00:00Z"),
        Some(at("2026-06-19T12:00:00Z"))
    );
    assert_eq!(
        written("\"2026-06-19T12:00:00Z\""),
        Some(at("2026-06-19T12:00:00Z"))
    );
}

#[test]
fn a_success_records_its_time_and_generation() {
    let mut history = RunHistory::default();
    let follower = address("mise-install");
    history.record_action_change(&follower, 2);
    history.record_success(&follower, 3, at("2026-06-19T12:00:00Z"));
    assert_eq!(
        history.action(&follower),
        ActionEntry {
            last_success: Some(at("2026-06-19T12:00:00Z")),
            success_generation: Some(3),
            last_changed: Some(2),
        }
    );
}

#[test]
fn a_future_success_is_age_zero() {
    let entry = ActionEntry {
        last_success: Some(at("2026-06-19T12:00:00Z")),
        ..ActionEntry::default()
    };
    assert_eq!(
        entry.age(at("2026-06-19T15:00:00Z")),
        Some(Duration::from_secs(3 * 60 * 60))
    );
    assert_eq!(entry.age(at("2026-06-19T11:00:00Z")), Some(Duration::ZERO));
    assert_eq!(ActionEntry::default().age(at("2026-06-19T11:00:00Z")), None);
}

#[test]
fn the_document_round_trips_in_the_current_layout() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join(RunHistory::FILE_NAME);
    for document in [EXAMPLE, EARLIER] {
        let history = parse(document).expect("parse");
        history.save(&path).expect("save");
        assert_eq!(RunHistory::load(&path).expect("load"), history);
    }
    let written = toml::to_string(&parse(EARLIER).expect("parse")).expect("serialize");
    assert!(written.starts_with("generation = 0\n"), "{written}");
    assert!(!written.contains("pending"), "{written}");
}

#[test]
fn a_missing_document_is_an_empty_one() {
    let dir = tempfile::tempdir().expect("temp dir");
    assert_eq!(
        RunHistory::load(&dir.path().join(RunHistory::FILE_NAME)).expect("absent is empty"),
        RunHistory::default()
    );
}

/// A document of tables of integers, which `runs.toml` does not allow.
type Foreign = BTreeMap<String, BTreeMap<String, i64>>;

fn foreign() -> Foreign {
    BTreeMap::from([("a".to_owned(), BTreeMap::from([("runs".to_owned(), 3)]))])
}

#[test]
fn a_malformed_document_fails_and_is_left_alone() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join(RunHistory::FILE_NAME);
    tomlfile::write(&path, &foreign()).expect("a malformed document");
    assert!(RecordedRuns::load(dir.path(), RunMode::Perform).is_err());
    assert_eq!(
        tomlfile::read::<Foreign>(&path).expect("still there"),
        foreign()
    );
}

#[test]
fn a_run_records_under_the_next_generation_and_saves_once() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join("state");
    let path = state.join(RunHistory::FILE_NAME);
    let reporter = Reporter::new(false);

    let mut runs = RecordedRuns::load(&state, RunMode::Perform).expect("load");
    runs.save(&reporter);
    assert!(
        !state.exists(),
        "a run that recorded nothing writes nothing"
    );

    runs.record_remote_change(&id("tool"));
    runs.record_action_change(&address("link"));
    runs.record_success(&address("follower"), at("2026-06-19T12:00:00Z"));
    assert!(!path.exists(), "nothing is written before the run ends");
    assert_eq!(runs.action_changed(&address("link")), Some(1));
    runs.save(&reporter);
    let saved = RunHistory::load(&path).expect("saved");
    assert_eq!(saved, runs.history);
    assert_eq!(saved.generation, 1);
    assert_eq!(saved.remote_changed(&id("tool")), Some(1));
    assert_eq!(
        saved.action(&address("follower")).success_generation,
        Some(1)
    );

    let mut next = RecordedRuns::load(&state, RunMode::Perform).expect("load");
    next.record_remote_change(&id("tool"));
    next.save(&reporter);
    let saved = RunHistory::load(&path).expect("saved");
    assert_eq!(saved.generation, 2);
    assert_eq!(saved.remote_changed(&id("tool")), Some(2));
    assert_eq!(saved.action(&address("link")).last_changed, Some(1), "kept");
}

#[test]
fn a_dry_run_records_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("temp dir");
    let reporter = Reporter::new(false);
    let mut runs = RecordedRuns::load(dir.path(), RunMode::DryRun).expect("load");
    runs.record_remote_change(&id("tool"));
    runs.record_action_change(&address("link"));
    runs.record_success(&address("other"), at("2026-06-19T12:00:00Z"));
    runs.save(&reporter);
    assert!(!dir.path().join(RunHistory::FILE_NAME).exists());
    assert_eq!(runs.action(&address("other")), ActionEntry::default());
    assert_eq!(runs.remote_changed(&id("tool")), None);
}

#[test]
fn a_failed_write_leaves_what_was_there() {
    let dir = tempfile::tempdir().expect("temp dir");
    let state = dir.path().join("state");
    let reporter = Reporter::new(false);
    let mut runs = RecordedRuns::load(&state, RunMode::Perform).expect("load");
    // The state directory cannot be created once a file holds its path.
    tomlfile::write(&state, &foreign()).expect("a file");
    runs.record_action_change(&address("link"));
    runs.save(&reporter);
    assert_eq!(runs.action_changed(&address("link")), Some(1));
    assert_eq!(
        tomlfile::read::<Foreign>(&state).expect("the file"),
        foreign()
    );
}
