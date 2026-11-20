use super::*;
use crate::manifest::duration::FriendlyDuration;

fn at(text: &str) -> Timestamp {
    text.parse().expect("a valid timestamp")
}

/// An entry that last succeeded at `at_time`, in generation 3.
fn succeeded(at_time: &str) -> ActionEntry {
    ActionEntry {
        last_success: Some(at(at_time)),
        success_generation: Some(3),
        last_changed: None,
    }
}

fn every(text: &str) -> Trigger {
    Trigger::Every(FriendlyDuration::new(text).expect("valid"))
}

fn after() -> Trigger {
    Trigger::After(vec![AfterTarget::Action(
        ItemId::try_from("mise".to_owned()).expect("valid"),
    )])
}

const NOW: &str = "2026-06-19T12:00:00Z";

#[test]
fn always_fires_and_bootstrap_fires_only_in_a_bootstrap() {
    let never = ActionEntry::default();
    assert_eq!(
        fire(&Trigger::Always, &never, None, false, at(NOW)),
        Fires::Yes
    );
    assert_eq!(
        fire(&Trigger::Bootstrap, &never, None, true, at(NOW)),
        Fires::Yes
    );
    assert_eq!(
        fire(&Trigger::Bootstrap, &never, None, false, at(NOW)),
        Fires::No("not a bootstrap run".to_owned())
    );
}

#[test]
fn once_fires_until_it_succeeds() {
    assert_eq!(
        fire(
            &Trigger::Once,
            &ActionEntry::default(),
            None,
            false,
            at(NOW)
        ),
        Fires::Yes
    );
    assert_eq!(
        fire(&Trigger::Once, &succeeded(NOW), None, false, at(NOW)),
        Fires::No("already ran".to_owned())
    );
}

#[test]
fn every_fires_once_its_interval_has_passed() {
    let trigger = every("24h");
    let fires = |entry: &ActionEntry| fire(&trigger, entry, None, false, at(NOW));
    assert_eq!(fires(&ActionEntry::default()), Fires::Yes);
    assert_eq!(
        fires(&succeeded("2026-06-19T09:00:00Z")),
        Fires::No("ran 3h ago".to_owned())
    );
    assert_eq!(fires(&succeeded("2026-06-18T12:00:00Z")), Fires::Yes);
    // A success recorded in the future is age zero, and so fresh.
    assert_eq!(
        fires(&succeeded("2026-06-20T12:00:00Z")),
        Fires::No("ran just now".to_owned())
    );
    // A zero interval always fires.
    assert_eq!(
        fire(&every("0s"), &succeeded(NOW), None, false, at(NOW)),
        Fires::Yes
    );
}

#[test]
fn after_fires_until_it_succeeds_and_then_on_a_later_change() {
    let fires = |entry: &ActionEntry, change| fire(&after(), entry, change, false, at(NOW));
    assert_eq!(fires(&ActionEntry::default(), None), Fires::Yes);
    // An earlier layout's pending firing has a time but no generation.
    let pending = ActionEntry {
        success_generation: None,
        ..succeeded(NOW)
    };
    assert_eq!(fires(&pending, Some(1)), Fires::Yes);
    let unchanged = Fires::No("nothing it follows changed".to_owned());
    assert_eq!(fires(&succeeded(NOW), None), unchanged);
    assert_eq!(fires(&succeeded(NOW), Some(2)), unchanged);
    // A change in the success's own run came before it.
    assert_eq!(fires(&succeeded(NOW), Some(3)), unchanged);
    assert_eq!(fires(&succeeded(NOW), Some(4)), Fires::Yes);
}
