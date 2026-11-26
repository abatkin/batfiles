use std::rc::Rc;

use super::*;
use crate::manifest::condition::HostNamespaces;
use crate::output::Verbosity;
use crate::run::inclusion::Inclusion;
use crate::run::record::RunRecord;
use crate::vars::name::VarName;
use crate::vars::scope::VarScope;

fn quiet() -> Reporter {
    let mut reporter = Reporter::new(false);
    reporter.set_verbosity(Verbosity::Quiet);
    reporter
}

fn address(address: &str) -> ItemAddress {
    ItemAddress::try_from(address.to_owned()).expect("valid address")
}

/// A leaf `create-dir` with both an `id` and a `group`.
fn action(id: &str, group: &str) -> RunRecord {
    RunRecord::leaf(create_dir(id, group), 1)
}

/// Build an included `create-dir` record with an optional inclusion ID.
fn included(id: &str, group: &str, inclusion: Option<&str>) -> RunRecord {
    let named = match inclusion {
        Some(id) => format!("id = \"{id}\"\n"),
        None => String::new(),
    };
    let declaration = toml::from_str(&format!("{named}remote = \"corporate\"\n"))
        .expect("the record should parse");
    let by = Inclusion::at(&declaration, 1);
    RunRecord::contributed(create_dir(id, group), 1, by.contributor())
}

fn create_dir(id: &str, group: &str) -> Action {
    toml::from_str(&format!(
        "type = \"create-dir\"\nid = \"{id}\"\ngroup = \"{group}\"\ndest = \"~/x\"\n"
    ))
    .expect("the record should parse")
}

fn disabled(actions: &[&str], groups: &[&str]) -> DisabledItems {
    DisabledItems {
        actions: actions.iter().map(|name| address(name)).collect(),
        groups: groups.iter().map(|name| address(name)).collect(),
    }
}

/// A `sync`'s filter: it asks for everything, so it waives nothing.
fn selection(
    skip_actions: &[&str],
    skip_groups: &[&str],
    env: &[(&str, &str)],
    disabled: DisabledItems,
) -> Selection<'static> {
    filter(Target::Everything, skip_actions, skip_groups, env, disabled)
}

fn filter<'a>(
    target: Target<'a>,
    skip_actions: &[&str],
    skip_groups: &[&str],
    env: &[(&str, &str)],
    disabled: DisabledItems,
) -> Selection<'a> {
    Selection::new(
        target,
        &names_of(skip_actions),
        &names_of(skip_groups),
        &Environment::from_pairs(env.iter().copied()),
        disabled,
        &quiet(),
    )
}

fn names_of(values: &[&str]) -> Vec<String> {
    values.iter().copied().map(str::to_owned).collect()
}

/// The exclusion reason for `record`, with no variables bound.
fn reason(selection: &Selection, record: &RunRecord) -> Option<String> {
    decided(selection, record, &[]).map(|exclusion| exclusion.reason().to_owned())
}

/// The exclusion for `record` with `vars` bound.
fn decided(selection: &Selection, record: &RunRecord, vars: &[(&str, &str)]) -> Option<Exclusion> {
    decided_with(selection, record, vars, TargetWaivers::Apply)
}

/// Evaluate the record using `vars` and the specified target exemptions.
fn decided_with(
    selection: &Selection,
    record: &RunRecord,
    vars: &[(&str, &str)],
    waivers: TargetWaivers,
) -> Option<Exclusion> {
    let variables = Rc::new(VarScope::stack(
        vars.iter()
            .map(|(name, value)| {
                (
                    VarName::try_from((*name).to_owned()).expect("valid name"),
                    crate::vars::scope::VarValue::Static((*value).to_owned()),
                )
            })
            .collect(),
        BTreeMap::new(),
        BTreeMap::new(),
        &[],
    ));
    let empty = Environment::from_pairs(std::iter::empty::<(&str, &str)>());
    let host = HostNamespaces::capture(&empty);
    let bindings = Bindings::new(&variables, &host);
    selection
        .decide(record.subject(), &bindings, waivers, Some(NOT_INSTALLED))
        .expect("no cycle")
}

/// Return the reason for a [`Deliberate`](Exclusion::Deliberate) exclusion; panic on
/// evaluation failure.
fn expected_reason(
    selection: &Selection,
    record: &RunRecord,
    vars: &[(&str, &str)],
) -> Option<String> {
    match decided(selection, record, vars) {
        Some(Exclusion::Deliberate(reason)) => Some(reason),
        Some(other) => panic!("expected an ordinary exclusion, got {other:?}"),
        None => None,
    }
}

/// A `create-dir` carrying one condition, in the spelling named.
fn conditioned(spelling: &str, condition: &str) -> RunRecord {
    RunRecord::leaf(
        toml::from_str(&format!(
            "type = \"create-dir\"\nid = \"zshrc\"\ngroup = \"shell\"\n\
                 dest = \"~/x\"\n{spelling} = \"{condition}\"\n"
        ))
        .expect("the record should parse"),
        1,
    )
}

#[test]
fn an_action_named_by_nothing_runs() {
    let selection = selection(&["other"], &["other"], &[], DisabledItems::default());
    assert_eq!(reason(&selection, &action("zshrc", "shell")), None);
}

#[test]
fn either_namespace_can_name_an_action() {
    let by_action = selection(&["zshrc"], &[], &[], DisabledItems::default());
    assert_eq!(
        reason(&by_action, &action("zshrc", "shell")).as_deref(),
        Some("`zshrc` from --skip-action")
    );
    let by_group = selection(&[], &["shell"], &[], DisabledItems::default());
    assert_eq!(
        reason(&by_group, &action("zshrc", "shell")).as_deref(),
        Some("`shell` from --skip-group")
    );
}

#[test]
fn the_two_run_only_sources_union() {
    let selection = selection(
        &[],
        &[],
        &[(SKIP_ACTIONS, "zshrc"), (SKIP_GROUPS, "gui")],
        DisabledItems::default(),
    );
    assert_eq!(
        reason(&selection, &action("zshrc", "shell")).as_deref(),
        Some("`zshrc` from BATFILES_SKIP_ACTIONS")
    );
    assert_eq!(
        reason(&selection, &action("gtk", "gui")).as_deref(),
        Some("`gui` from BATFILES_SKIP_GROUPS")
    );
}

#[test]
fn the_option_is_credited_when_both_sources_name_one_thing() {
    let selection = selection(
        &["zshrc"],
        &[],
        &[(SKIP_ACTIONS, "zshrc")],
        DisabledItems::default(),
    );
    assert_eq!(
        reason(&selection, &action("zshrc", "shell")).as_deref(),
        Some("`zshrc` from --skip-action")
    );
}

#[test]
fn a_disable_is_reported_ahead_of_a_skip() {
    let selection = selection(
        &["zshrc"],
        &["shell"],
        &[],
        disabled(&["zshrc"], &["shell"]),
    );
    assert_eq!(
        reason(&selection, &action("zshrc", "shell")).as_deref(),
        Some("action `zshrc` is disabled")
    );
}

#[test]
fn a_disabled_group_is_reported_where_the_action_itself_is_not_disabled() {
    let selection = selection(&["zshrc"], &[], &[], disabled(&[], &["shell"]));
    assert_eq!(
        reason(&selection, &action("zshrc", "shell")).as_deref(),
        Some("group `shell` is disabled")
    );
}

#[test]
fn an_actions_own_name_is_reported_ahead_of_its_groups() {
    let selection = selection(&["zshrc"], &["shell"], &[], DisabledItems::default());
    assert_eq!(
        reason(&selection, &action("zshrc", "shell")).as_deref(),
        Some("`zshrc` from --skip-action")
    );
}

#[test]
fn asking_for_a_group_waives_that_group_and_no_other() {
    let work = address("work");
    let by_group = filter(
        Target::Group(&work),
        &[],
        &[],
        &[],
        disabled(&[], &["work", "corp.shell"]),
    );
    assert_eq!(reason(&by_group, &action("corp", "work")), None);
    assert_eq!(
        reason(&by_group, &included("zshrc", "shell", Some("corp"))).as_deref(),
        Some("group `corp.shell` is disabled")
    );
    assert_eq!(
        reason(&by_group, &included("p10k", "prompt", Some("corp"))),
        None
    );

    let corp_shell = address("corp.shell");
    let by_inner_group = filter(
        Target::Group(&corp_shell),
        &[],
        &[],
        &[],
        disabled(&[], &["work", "corp.shell"]),
    );
    assert_eq!(
        reason(&by_inner_group, &included("zshrc", "shell", Some("corp"))),
        None
    );
}

#[test]
fn asking_for_a_group_waives_the_group_level_exclusions_only() {
    let shell = address("shell");
    let by_skip = filter(
        Target::Group(&shell),
        &["zshrc"],
        &["shell"],
        &[(SKIP_GROUPS, "shell")],
        disabled(&[], &["shell"]),
    );
    assert_eq!(reason(&by_skip, &action("other", "shell")), None);
    assert_eq!(
        reason(&by_skip, &action("zshrc", "shell")).as_deref(),
        Some("`zshrc` from --skip-action")
    );

    let by_disable = filter(
        Target::Group(&shell),
        &[],
        &[],
        &[],
        disabled(&["zshrc"], &["shell"]),
    );
    assert_eq!(
        reason(&by_disable, &action("zshrc", "shell")).as_deref(),
        Some("action `zshrc` is disabled")
    );
}

#[test]
fn asking_for_an_action_waives_every_exclusion() {
    let zshrc = address("zshrc");
    let selection = filter(
        Target::Action(&zshrc),
        &["zshrc"],
        &["shell"],
        &[(SKIP_ACTIONS, "zshrc"), (SKIP_GROUPS, "shell")],
        disabled(&["zshrc"], &["shell"]),
    );
    assert_eq!(reason(&selection, &action("zshrc", "shell")), None);
}

#[test]
fn an_unwaived_decision_honors_every_list_the_selection_read() {
    // No-waiver decisions still use only the skip lists loaded for this command.
    let shell = address("corp.shell");
    let unwaived = |skip_actions: &[&str], skip_groups: &[&str], disabled| {
        let selection = filter(
            Target::Group(&shell),
            skip_actions,
            skip_groups,
            &[],
            disabled,
        );
        decided_with(
            &selection,
            &action("corp", "work"),
            &[],
            TargetWaivers::Ignore,
        )
        .map(|it| it.reason().to_owned())
    };
    assert_eq!(
        unwaived(&[], &[], disabled(&[], &["work"])).as_deref(),
        Some("group `work` is disabled")
    );
    assert_eq!(
        unwaived(&["corp"], &[], DisabledItems::default()).as_deref(),
        Some("`corp` from --skip-action")
    );
    assert_eq!(unwaived(&[], &["work"], DisabledItems::default()), None);

    let zshrc = address("corp.zshrc");
    let by_name = filter(
        Target::Action(&zshrc),
        &[],
        &[],
        &[],
        DisabledItems::default(),
    );
    let unwaived = decided_with(
        &by_name,
        &conditioned("when", "work"),
        &[("work", "false")],
        TargetWaivers::Ignore,
    );
    assert_eq!(
        unwaived.map(|it| it.reason().to_owned()).as_deref(),
        Some("when \"work\" is false")
    );
}

#[test]
fn a_condition_decides_the_record_it_is_written_on() {
    let selection = selection(&[], &[], &[], DisabledItems::default());
    let vars = &[("work", "false")];

    assert_eq!(
        expected_reason(&selection, &conditioned("when", "work"), vars).as_deref(),
        Some("when \"work\" is false")
    );
    assert_eq!(
        expected_reason(&selection, &conditioned("unless", "work"), vars),
        None
    );

    let vars = &[("work", "true")];
    assert_eq!(
        expected_reason(&selection, &conditioned("when", "work"), vars),
        None
    );
    assert_eq!(
        expected_reason(&selection, &conditioned("unless", "work"), vars).as_deref(),
        Some("unless \"work\" is true")
    );
}

#[test]
fn a_condition_is_consulted_only_where_nothing_else_excludes_the_record() {
    // `nowhere` is declared by no layer, so evaluating it would fail.
    let skipping = selection(&["zshrc"], &[], &[], DisabledItems::default());
    assert_eq!(
        expected_reason(&skipping, &conditioned("when", "nowhere"), &[]).as_deref(),
        Some("`zshrc` from --skip-action")
    );

    let plain = selection(&[], &[], &[], DisabledItems::default());
    assert!(matches!(
        decided(&plain, &conditioned("when", "nowhere"), &[]),
        Some(Exclusion::EvaluationFailed(_))
    ));
}

#[test]
fn a_condition_that_cannot_be_decided_closes_the_gate_in_either_spelling() {
    // An evaluation error must exclude even under `unless`.
    let selection = selection(&[], &[], &[], DisabledItems::default());

    for spelling in ["when", "unless"] {
        let Some(Exclusion::EvaluationFailed(why)) =
            decided(&selection, &conditioned(spelling, "nowhere"), &[])
        else {
            panic!("`{spelling}` should close on a condition nothing can decide");
        };
        assert!(why.starts_with(&format!("{spelling} \"nowhere\"")), "{why}");
        assert!(why.contains("cannot be evaluated"), "{why}");
        assert!(why.contains(NOT_INSTALLED), "{why}");
        assert!(why.contains("`nowhere` is not declared"), "{why}");
        assert!(why.contains("batfiles vars set nowhere"), "{why}");
    }
}

#[test]
fn asking_for_one_action_waives_its_condition_too() {
    let zshrc = address("zshrc");
    let by_name = filter(
        Target::Action(&zshrc),
        &[],
        &[],
        &[],
        DisabledItems::default(),
    );
    assert_eq!(
        decided(&by_name, &conditioned("when", "work"), &[("work", "false")]),
        None
    );
    assert_eq!(
        decided(&by_name, &conditioned("when", "nowhere"), &[]),
        None
    );

    let shell = address("shell");
    let by_group = filter(
        Target::Group(&shell),
        &[],
        &[],
        &[],
        DisabledItems::default(),
    );
    assert_eq!(
        expected_reason(
            &by_group,
            &conditioned("when", "work"),
            &[("work", "false")]
        )
        .as_deref(),
        Some("when \"work\" is false")
    );
}

#[test]
fn a_record_with_no_condition_is_decided_by_the_lists_alone() {
    let selection = selection(&[], &[], &[], DisabledItems::default());
    assert_eq!(decided(&selection, &action("zshrc", "shell"), &[]), None);
}

#[test]
fn a_condition_is_repeated_back_as_written_and_cannot_forge_a_line() {
    let selection = selection(&[], &[], &[], DisabledItems::default());
    let record = conditioned("when", "work && vars['a\\nb']");
    let why = expected_reason(&selection, &record, &[("work", "true")])
        .expect("the condition should be false");
    assert!(!why.contains('\n'), "{why}");
    assert!(why.contains("\\n"), "{why}");
}

#[test]
fn a_name_that_is_not_an_address_is_dropped_rather_than_kept() {
    let mut skips = SkipList::default();
    skips.extend(&names_of(&["a..b", "zshrc"]), "--skip-action", &quiet());
    assert_eq!(skips.origin(&address("zshrc")), Some("--skip-action"));
    assert_eq!(skips.names.len(), 1);
}

#[test]
fn a_qualified_skip_is_kept_and_names_no_leaf_record() {
    let by_skip = selection(
        &["core.zshrc"],
        &["core.shell"],
        &[],
        DisabledItems::default(),
    );
    assert_eq!(reason(&by_skip, &action("zshrc", "shell")), None);

    let by_disable = selection(&[], &[], &[], disabled(&["core.zshrc"], &["core.shell"]));
    assert_eq!(reason(&by_disable, &action("zshrc", "shell")), None);
}

#[test]
fn an_included_record_answers_to_its_qualified_name_alone() {
    let contributed = included("zshrc", "shell", Some("core"));

    for (skips, groups, expected) in [
        (["core.zshrc"], [""; 1], "`core.zshrc` from --skip-action"),
        ([""; 1], ["core.shell"], "`core.shell` from --skip-group"),
    ] {
        let selection = selection(&skips, &groups, &[], DisabledItems::default());
        assert_eq!(reason(&selection, &contributed).as_deref(), Some(expected));
    }

    let unqualified = selection(
        &["zshrc"],
        &["shell"],
        &[],
        disabled(&["zshrc"], &["shell"]),
    );
    assert_eq!(reason(&unqualified, &contributed), None);
}

#[test]
fn a_record_from_an_unnamed_inclusion_answers_to_nothing() {
    let contributed = included("zshrc", "shell", None);
    let selection = selection(
        &["zshrc", "core.zshrc"],
        &["shell", "core.shell"],
        &[],
        disabled(&["zshrc", "core.zshrc"], &["shell", "core.shell"]),
    );
    assert_eq!(reason(&selection, &contributed), None);
}

#[test]
fn a_target_names_an_included_record_by_its_qualified_address() {
    let contributed = included("zshrc", "shell", Some("core"));
    let leaf = action("zshrc", "shell");

    let qualified = address("core.zshrc");
    let by_action = filter(
        Target::Action(&qualified),
        &[],
        &[],
        &[],
        DisabledItems::default(),
    );
    assert!(by_action.wants(contributed.subject()));
    assert!(!by_action.wants(leaf.subject()));

    let qualified = address("core.shell");
    let by_group = filter(
        Target::Group(&qualified),
        &[],
        &[],
        &[],
        DisabledItems::default(),
    );
    assert!(by_group.wants(contributed.subject()));
    assert!(!by_group.wants(leaf.subject()));
}

#[test]
fn a_target_reaches_into_the_inclusion_its_address_is_qualified_by() {
    let core = address("core");
    let everything = selection(&[], &[], &[], DisabledItems::default());
    assert!(everything.reaches_into(Some(&core)));
    assert!(everything.reaches_into(None));

    let qualified = address("core.zshrc");
    let named = filter(
        Target::Action(&qualified),
        &[],
        &[],
        &[],
        DisabledItems::default(),
    );
    assert!(named.reaches_into(Some(&core)));
    assert!(!named.reaches_into(Some(&address("work"))));
    assert!(!named.reaches_into(None));

    // An inclusion's own address does not reach its children.
    let record = address("core");
    let inclusion = filter(
        Target::Action(&record),
        &[],
        &[],
        &[],
        DisabledItems::default(),
    );
    assert!(!inclusion.reaches_into(Some(&core)));
}

#[test]
fn only_an_action_target_reaches_into_a_clone_list() {
    let list = address("core.plugins");
    let entry = address("core.plugins.p10k");
    let by_action = filter(
        Target::Action(&entry),
        &[],
        &[],
        &[],
        DisabledItems::default(),
    );
    assert!(by_action.reaches_into_list(Some(&list)));
    assert!(!by_action.reaches_into_list(Some(&address("plugins"))));
    assert!(!by_action.reaches_into_list(None));

    let by_group = filter(
        Target::Group(&entry),
        &[],
        &[],
        &[],
        DisabledItems::default(),
    );
    assert!(!by_group.reaches_into_list(Some(&list)));
    let everything = selection(&[], &[], &[], DisabledItems::default());
    assert!(!everything.reaches_into_list(Some(&list)));
}

/// An entry subject at `address`, under a `when = "work"` gate.
fn entry_disposition(
    selection: &Selection,
    address: &ItemAddress,
    list: &Disposition,
) -> Disposition {
    let condition = crate::manifest::condition::Condition::new("work").expect("a condition");
    let entry = Subject {
        address: Some(address),
        group_address: None,
        gate: Some(Gate::When(&condition)),
    };
    let variables = Rc::new(VarScope::stack(
        BTreeMap::from([(
            VarName::try_from("work".to_owned()).expect("valid name"),
            crate::vars::scope::VarValue::Static("false".to_owned()),
        )]),
        BTreeMap::new(),
        BTreeMap::new(),
        &[],
    ));
    let empty = Environment::from_pairs(std::iter::empty::<(&str, &str)>());
    let host = HostNamespaces::capture(&empty);
    selection
        .entry_disposition(entry, list, &Bindings::new(&variables, &host))
        .expect("no cycle")
}

fn reason_of(disposition: Disposition) -> Option<String> {
    match disposition {
        Disposition::Excluded(exclusion) => Some(exclusion.reason().to_owned()),
        Disposition::Allowed => None,
        Disposition::AllowedInPart => Some("allowed in part".to_owned()),
        Disposition::NotRequested => Some("not requested".to_owned()),
    }
}

#[test]
fn an_entry_named_by_the_target_waives_its_own_exclusions() {
    let entry = address("plugins.p10k");
    let named = filter(
        Target::Action(&entry),
        &[],
        &[],
        &[],
        disabled(&["plugins.p10k"], &[]),
    );
    assert_eq!(
        reason_of(entry_disposition(
            &named,
            &entry,
            &Disposition::AllowedInPart
        )),
        None
    );
    let other = address("plugins.zsh-z");
    assert_eq!(
        reason_of(entry_disposition(
            &named,
            &other,
            &Disposition::AllowedInPart
        ))
        .as_deref(),
        Some("not requested")
    );
}

#[test]
fn naming_a_list_does_not_waive_its_entries_exclusions() {
    let list = address("plugins");
    let named = filter(
        Target::Action(&list),
        &[],
        &[],
        &[],
        disabled(&["plugins.p10k"], &[]),
    );
    assert_eq!(
        reason_of(entry_disposition(
            &named,
            &address("plugins.p10k"),
            &Disposition::Allowed
        ))
        .as_deref(),
        Some("action `plugins.p10k` is disabled")
    );
    assert_eq!(
        reason_of(entry_disposition(
            &named,
            &address("plugins.zsh-z"),
            &Disposition::Allowed
        ))
        .as_deref(),
        Some("when \"work\" is false")
    );
}

#[test]
fn a_skipped_entry_is_not_asked_its_condition() {
    let sync = selection(&["plugins.p10k"], &[], &[], DisabledItems::default());
    assert_eq!(
        reason_of(entry_disposition(
            &sync,
            &address("plugins.p10k"),
            &Disposition::Allowed
        ))
        .as_deref(),
        Some("`plugins.p10k` from --skip-action")
    );
}
