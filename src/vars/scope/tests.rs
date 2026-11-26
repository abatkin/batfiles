use super::*;

/// Host namespaces with an empty environment.
fn host() -> HostNamespaces {
    HostNamespaces::capture(&Environment::from_pairs(std::iter::empty::<(&str, &str)>()))
}

/// The value of `name`, which must not be declared with no value.
fn get(scope: &Rc<VarScope>, name: &str) -> Option<String> {
    scope
        .lookup(name, &host())
        .map(|resolved| resolved.expect("a declared variable should have a value"))
}

/// Every line a listing of `scope` prints.
fn lines(scope: &Rc<VarScope>) -> Vec<String> {
    scope.lines_for(scope.names(), &host()).expect("no cycle")
}

fn name(name: &str) -> VarName {
    VarName::try_from(name.to_owned()).expect("valid name")
}

fn layer<const N: usize>(pairs: [(&str, &str); N]) -> BTreeMap<VarName, String> {
    pairs
        .into_iter()
        .map(|(key, value)| (name(key), value.to_owned()))
        .collect()
}

fn command_line<const N: usize>(pairs: [(&str, &str); N]) -> Vec<(VarName, String)> {
    pairs
        .into_iter()
        .map(|(key, value)| (name(key), value.to_owned()))
        .collect()
}

/// The four layers, in precedence order.
fn stacked<const A: usize, const B: usize, const C: usize, const D: usize>(
    manifest: [(&str, &str); A],
    machine: [(&str, &str); B],
    environment: [(&str, &str); C],
    cli: [(&str, &str); D],
) -> Rc<VarScope> {
    Rc::new(VarScope::stack(
        statics(layer(manifest)),
        layer(machine),
        layer(environment),
        &command_line(cli),
    ))
}

/// The value in force and where it came from.
fn winner(scope: &Rc<VarScope>, key: &str) -> (String, Origin) {
    let (origin, _) = scope
        .declaring(key)
        .next()
        .expect("some layer should declare it");
    let value = get(scope, key).expect("some layer should declare it");
    (value, origin.clone())
}

/// The layers that value overrode, highest first.
fn shadowed(scope: &Rc<VarScope>, key: &str) -> Vec<Origin> {
    scope
        .declaring(key)
        .skip(1)
        .map(|(origin, _)| origin.clone())
        .collect()
}

#[test]
fn each_layer_overrides_the_one_below_it() {
    let scope = stacked(
        [("a", "manifest"), ("b", "manifest"), ("c", "manifest")],
        [("a", "machine"), ("b", "machine")],
        [("a", "environment")],
        [],
    );
    assert_eq!(
        winner(&scope, "a"),
        ("environment".to_owned(), Origin::Environment)
    );
    assert_eq!(winner(&scope, "b"), ("machine".to_owned(), Origin::Machine));
    assert_eq!(
        winner(&scope, "c"),
        ("manifest".to_owned(), Origin::LeafManifest)
    );
}

#[test]
fn the_command_line_wins_over_every_other_layer() {
    let scope = stacked(
        [("a", "manifest")],
        [("a", "machine")],
        [("a", "environment")],
        [("a", "cli")],
    );
    assert_eq!(winner(&scope, "a"), ("cli".to_owned(), Origin::CommandLine));
}

#[test]
fn a_name_only_one_layer_declares_takes_that_layer_alone() {
    let scope = stacked([], [], [], [("only", "cli")]);
    assert_eq!(
        winner(&scope, "only"),
        ("cli".to_owned(), Origin::CommandLine)
    );
    assert!(shadowed(&scope, "only").is_empty());
}

#[test]
fn the_set_is_the_union_of_the_layers() {
    let scope = stacked([("a", "1")], [("b", "2")], [("c", "3")], [("d", "4")]);
    assert_eq!(
        scope
            .names()
            .into_iter()
            .map(VarName::to_string)
            .collect::<Vec<_>>(),
        ["a", "b", "c", "d"]
    );
}

#[test]
fn a_name_no_layer_declares_is_declared_by_no_layer() {
    let scope = stacked([("a", "1")], [], [], []);
    assert_eq!(scope.declaring("absent").count(), 0);
    assert_eq!(get(&scope, "absent").as_deref(), None);
}

#[test]
fn get_answers_with_the_value_in_force() {
    let scope = stacked(
        [("a", "manifest"), ("b", "manifest")],
        [("a", "machine")],
        [],
        [],
    );
    assert_eq!(get(&scope, "a").as_deref(), Some("machine"));
    assert_eq!(get(&scope, "b").as_deref(), Some("manifest"));
}

#[test]
fn get_tells_a_declared_empty_value_from_an_undeclared_name() {
    let scope = stacked([], [], [], [("empty", "")]);
    assert_eq!(get(&scope, "empty").as_deref(), Some(""));
    assert_eq!(get(&scope, "missing").as_deref(), None);
}

#[test]
fn get_accepts_text_that_is_not_a_valid_name() {
    let scope = stacked([("a", "1")], [], [], []);
    assert_eq!(get(&scope, "has-dash").as_deref(), None);
    assert_eq!(get(&scope, "").as_deref(), None);
}

#[test]
fn an_empty_value_overrides_like_any_other() {
    let scope = stacked([("profile", "personal")], [], [], [("profile", "")]);
    assert_eq!(
        winner(&scope, "profile"),
        (String::new(), Origin::CommandLine)
    );
}

#[test]
fn a_repeated_command_line_value_keeps_the_last_one_written() {
    let scope = stacked([], [], [], [("p", "first"), ("p", "second"), ("q", "only")]);
    assert_eq!(
        winner(&scope, "p"),
        ("second".to_owned(), Origin::CommandLine)
    );
    assert_eq!(
        winner(&scope, "q"),
        ("only".to_owned(), Origin::CommandLine)
    );
}

#[test]
fn a_repeated_command_line_value_is_one_layer_and_not_two() {
    let scope = stacked([("p", "manifest")], [], [], [("p", "first"), ("p", "last")]);
    assert_eq!(
        shadowed(&scope, "p"),
        [Origin::LeafManifest],
        "a repeated `--var` should not shadow itself"
    );
}

#[test]
fn a_variable_remembers_the_layers_it_overrode() {
    let scope = stacked(
        [("a", "manifest")],
        [("a", "machine")],
        [("a", "environment")],
        [("a", "cli")],
    );
    assert_eq!(
        shadowed(&scope, "a"),
        [Origin::Environment, Origin::Machine, Origin::LeafManifest],
        "the layers should be listed highest first"
    );
}

#[test]
fn a_name_is_case_sensitive_across_layers() {
    let scope = stacked([("editor", "nvim")], [("EDITOR", "vi")], [], []);
    assert_eq!(
        winner(&scope, "editor"),
        ("nvim".to_owned(), Origin::LeafManifest)
    );
    assert_eq!(winner(&scope, "EDITOR"), ("vi".to_owned(), Origin::Machine));
}

/// An inclusion label, as [`Inclusion::at`](crate::run::inclusion::Inclusion::at)
/// spells one.
const CORP: &str = "include-remote `corp`";

/// How a line names that inclusion's remote `[vars]`.
const CORP_MANIFEST: &str = "batfiles.toml of include-remote `corp`";

/// `base` derived for an inclusion with `overrides` and no remote `[vars]`.
fn derived<const N: usize>(base: &Rc<VarScope>, overrides: [(&str, &str); N]) -> Rc<VarScope> {
    derived_from(base, [], overrides)
}

/// `base` derived for an inclusion with remote `[vars]` and `overrides`.
fn derived_from<const M: usize, const N: usize>(
    base: &Rc<VarScope>,
    remote: [(&str, &str); M],
    overrides: [(&str, &str); N],
) -> Rc<VarScope> {
    let overrides = overrides
        .into_iter()
        .map(|(key, value)| (name(key), TemplateString::from(value.to_owned())))
        .collect();
    let scope = base
        .with_inclusion(&statics(layer(remote)), None, &overrides, CORP, &host())
        .expect("no cycle");
    Rc::new(scope)
}

/// The lines [`VarScope::report_inclusion`] would print.
fn override_lines(scope: &Rc<VarScope>) -> Vec<String> {
    match scope.inclusion_block() {
        Some((_, names)) => scope.lines_for(names, &host()).expect("no cycle"),
        None => Vec::new(),
    }
}

/// The heading that block is printed under.
fn block_heading(scope: &Rc<VarScope>) -> Option<String> {
    scope.inclusion_block().map(|(heading, _)| heading)
}

/// Every layer's origin, lowest precedence first.
fn origins(scope: &Rc<VarScope>) -> Vec<Origin> {
    scope
        .layers
        .iter()
        .map(|layer| layer.origin.clone())
        .collect()
}

#[test]
fn a_derived_scope_places_each_of_its_layers_by_origin() {
    let scope = derived_from(&stacked([], [], [], []), [("a", "remote")], [("b", "corp")]);
    assert_eq!(
        origins(&scope),
        [
            Origin::IncludedManifest(CORP.to_owned()),
            Origin::LeafManifest,
            Origin::InclusionOverrides(CORP.to_owned()),
            Origin::Machine,
            Origin::Environment,
            Origin::CommandLine,
        ]
    );
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "derived from the run's base scope")]
fn a_scope_is_not_derived_from_another_scope() {
    let corp = derived(&stacked([], [], [], []), [("profile", "work")]);
    let _ = derived(&corp, [("profile", "lab")]);
}

#[test]
fn an_override_beats_the_leaf_and_loses_to_the_machine() {
    let base = stacked(
        [("a", "leaf"), ("b", "leaf"), ("c", "leaf"), ("d", "leaf")],
        [("b", "machine")],
        [("c", "environment")],
        [("d", "cli")],
    );
    let scope = derived(
        &base,
        [("a", "corp"), ("b", "corp"), ("c", "corp"), ("d", "corp")],
    );
    assert_eq!(
        winner(&scope, "a"),
        (
            "corp".to_owned(),
            Origin::InclusionOverrides(CORP.to_owned())
        )
    );
    assert_eq!(winner(&scope, "b"), ("machine".to_owned(), Origin::Machine));
    assert_eq!(
        winner(&scope, "c"),
        ("environment".to_owned(), Origin::Environment)
    );
    assert_eq!(winner(&scope, "d"), ("cli".to_owned(), Origin::CommandLine));
}

#[test]
fn an_override_adds_a_name_no_other_layer_declares() {
    let scope = derived(&stacked([], [], [], []), [("profile", "work")]);
    assert_eq!(get(&scope, "profile").as_deref(), Some("work"));
    assert!(shadowed(&scope, "profile").is_empty());
}

#[test]
fn deriving_a_scope_leaves_the_runs_own_set_alone() {
    let base = stacked([("profile", "personal")], [], [], []);
    let work = derived(&base, [("profile", "work")]);
    let other = derived(&base, [("profile", "lab")]);
    assert_eq!(get(&base, "profile").as_deref(), Some("personal"));
    assert_eq!(get(&work, "profile").as_deref(), Some("work"));
    assert_eq!(get(&other, "profile").as_deref(), Some("lab"));
}

#[test]
fn an_override_block_lists_the_names_the_inclusion_declared() {
    let base = stacked([("editor", "vi"), ("profile", "personal")], [], [], []);
    let scope = derived(&base, [("profile", "work")]);
    assert_eq!(
        override_lines(&scope),
        ["profile = \"work\" (include-remote `corp`; over batfiles.toml)"]
    );
}

#[test]
fn an_override_a_higher_layer_beat_is_listed_with_the_layer_that_won() {
    let base = stacked([("profile", "personal")], [("profile", "machine")], [], []);
    let scope = derived(&base, [("profile", "work")]);
    assert_eq!(
        override_lines(&scope),
        ["profile = \"machine\" (vars.toml; over include-remote `corp`, batfiles.toml)"]
    );
}

#[test]
fn a_set_no_inclusion_derived_has_no_block_of_its_own() {
    assert!(override_lines(&stacked([("a", "1")], [], [], [])).is_empty());
}

#[test]
fn an_override_map_declaring_nothing_has_nothing_to_report() {
    let scope = derived(&stacked([("a", "1")], [], [], []), []);
    assert!(override_lines(&scope).is_empty());
    assert_eq!(get(&scope, "a").as_deref(), Some("1"));
}

#[test]
fn an_included_remotes_vars_lose_to_every_other_layer() {
    let base = stacked(
        [("b", "leaf")],
        [("c", "machine")],
        [("d", "environment")],
        [("e", "cli")],
    );
    let scope = derived_from(
        &base,
        [
            ("a", "remote"),
            ("b", "remote"),
            ("c", "remote"),
            ("d", "remote"),
            ("e", "remote"),
        ],
        [("a", "corp")],
    );
    assert_eq!(
        winner(&scope, "a"),
        (
            "corp".to_owned(),
            Origin::InclusionOverrides(CORP.to_owned())
        )
    );
    assert_eq!(
        winner(&scope, "b"),
        ("leaf".to_owned(), Origin::LeafManifest)
    );
    assert_eq!(winner(&scope, "c"), ("machine".to_owned(), Origin::Machine));
    assert_eq!(
        winner(&scope, "d"),
        ("environment".to_owned(), Origin::Environment)
    );
    assert_eq!(winner(&scope, "e"), ("cli".to_owned(), Origin::CommandLine));
}

#[test]
fn an_included_remotes_declaration_stands_where_nothing_overrides_it() {
    let scope = derived_from(&stacked([], [], [], []), [("profile", "work")], []);
    assert_eq!(
        winner(&scope, "profile"),
        ("work".to_owned(), Origin::IncludedManifest(CORP.to_owned()))
    );
    assert!(shadowed(&scope, "profile").is_empty());
}

#[test]
fn an_included_remotes_vars_stay_inside_the_scope_they_were_read_into() {
    let base = stacked([], [], [], []);
    let corp = derived_from(&base, [("profile", "work")], []);
    let other = derived_from(&base, [("profile", "lab")], []);
    assert_eq!(get(&base, "profile").as_deref(), None);
    assert_eq!(get(&corp, "profile").as_deref(), Some("work"));
    assert_eq!(get(&other, "profile").as_deref(), Some("lab"));
}

#[test]
fn a_remotes_declaration_is_listed_under_the_inclusion_that_opened_it() {
    let scope = derived_from(&stacked([], [], [], []), [("profile", "work")], []);
    assert_eq!(
        override_lines(&scope),
        [format!("profile = \"work\" ({CORP_MANIFEST})")]
    );
}

#[test]
fn a_remotes_declaration_the_leaf_overrode_reads_as_having_lost() {
    let base = stacked([("profile", "personal")], [], [], []);
    let scope = derived_from(&base, [("profile", "work"), ("editor", "vi")], []);
    assert_eq!(
        override_lines(&scope),
        [
            format!("editor  = \"vi\" ({CORP_MANIFEST})"),
            format!("profile = \"personal\" (batfiles.toml; over {CORP_MANIFEST})"),
        ]
    );
}

#[test]
fn the_block_is_headed_by_the_inclusion_and_not_by_either_document() {
    let scope = derived_from(&stacked([], [], [], []), [("a", "remote")], [("b", "corp")]);
    assert_eq!(block_heading(&scope), Some(format!("{CORP} variables:")));
    assert_eq!(block_heading(&stacked([("a", "1")], [], [], [])), None);
}

#[test]
fn an_inclusion_overriding_nothing_still_reports_what_its_remote_declared() {
    let scope = derived_from(&stacked([], [], [], []), [("profile", "work")], []);
    assert_eq!(scope.derived_layers().count(), 2);
    assert!(!override_lines(&scope).is_empty());
}

#[test]
fn a_line_gives_the_value_its_layer_and_what_it_overrode() {
    let scope = stacked(
        [("editor", "vi"), ("profile", "personal")],
        [("editor", "nvim")],
        [],
        [("editor", "emacs")],
    );
    assert_eq!(
        lines(&scope),
        [
            "editor  = \"emacs\" (--var; over vars.toml, batfiles.toml)",
            "profile = \"personal\" (batfiles.toml)",
        ]
    );
}

#[test]
fn an_empty_value_is_visible_in_a_line() {
    let scope = stacked([], [], [], [("profile", "")]);
    assert_eq!(lines(&scope), ["profile = \"\" (--var)"]);
}

#[test]
fn an_empty_set_has_nothing_to_show() {
    assert!(lines(&stacked([], [], [], [])).is_empty());
}

/// A leaf scope whose manifest values are `templates` and whose command line sets `cli`.
fn templated<const A: usize, const B: usize>(
    templates: [(&str, &str); A],
    cli: [(&str, &str); B],
) -> Rc<VarScope> {
    let manifest = templates
        .into_iter()
        .map(|(key, value)| {
            let template = TemplateString::from(value.to_owned());
            (name(key), VarValue::written(&template))
        })
        .collect();
    Rc::new(VarScope::stack(
        manifest,
        BTreeMap::new(),
        BTreeMap::new(),
        &command_line(cli),
    ))
}

/// Why `name` has no value, which it must not have.
fn reason(scope: &Rc<VarScope>, name: &str) -> String {
    match scope.lookup(name, &host()) {
        Some(Err(no_value)) => no_value.to_string(),
        other => panic!("`{name}` should have no value, not {other:?}"),
    }
}

/// The message of the cycle `scope` found.
fn cycle(scope: &VarScope) -> String {
    scope
        .check_reads()
        .expect_err("a cycle should have been found")
        .to_string()
}

#[test]
fn a_template_reads_a_variable_declared_after_it() {
    let scope = templated(
        [("email", "me@${profile}.example.com"), ("profile", "home")],
        [],
    );
    assert_eq!(get(&scope, "email").as_deref(), Some("me@home.example.com"));
}

#[test]
fn a_template_reads_the_final_value_of_what_it_names() {
    let scope = templated(
        [("email", "me@${profile}"), ("profile", "home")],
        [("profile", "lab")],
    );
    assert_eq!(get(&scope, "email").as_deref(), Some("me@lab"));
}

#[test]
fn an_overridden_template_is_never_evaluated() {
    let scope = templated([("b", "${a}"), ("a", "${b}")], [("b", "x")]);
    assert_eq!(get(&scope, "a").as_deref(), Some("x"));
    assert!(scope.check_reads().is_ok());
}

#[test]
fn a_literal_value_is_static_and_an_escape_is_applied() {
    let scope = templated([("plain", "text"), ("escaped", "a$${b}")], []);
    assert_eq!(get(&scope, "escaped").as_deref(), Some("a${b}"));
    assert_eq!(
        lines(&scope),
        [
            "escaped = \"a${b}\" (batfiles.toml)",
            "plain   = \"text\" (batfiles.toml)",
        ]
    );
}

#[test]
fn a_template_naming_an_undeclared_variable_has_no_value() {
    let scope = templated([("x", "${nope}")], []);
    assert_eq!(reason(&scope, "x"), "`nope` is not declared");
    assert!(scope.check_reads().is_ok());
}

#[test]
fn a_template_reading_a_variable_with_no_value_has_no_value_and_says_which() {
    let scope = templated([("x", "${y}"), ("y", "${nope}")], []);
    assert_eq!(reason(&scope, "x"), "reads `y`: `nope` is not declared");
}

#[test]
fn a_vars_lookup_of_a_variable_with_no_value_reads_as_empty() {
    let scope = templated([("x", "[${vars.y}]"), ("y", "${nope}")], []);
    assert_eq!(get(&scope, "x").as_deref(), Some("[]"));
}

#[test]
fn a_self_reference_is_a_cycle() {
    let scope = templated([("path", "${path}:x")], []);
    assert!(scope.lookup("path", &host()).is_some_and(|it| it.is_err()));
    assert!(
        cycle(&scope).contains("cycle: path -> path;"),
        "{}",
        cycle(&scope)
    );
}

#[test]
fn a_cycle_through_other_variables_names_them_in_the_order_read() {
    let scope = templated([("a", "${b}"), ("b", "${vars.c}"), ("c", "${a}")], []);
    let _ = scope.lookup("a", &host());
    assert!(
        cycle(&scope).contains("cycle: a -> b -> c -> a;"),
        "{}",
        cycle(&scope)
    );
}

#[test]
fn a_cycle_no_read_reaches_goes_unnoticed() {
    let scope = templated([("a", "${b}"), ("b", "${a}"), ("c", "fine")], []);
    assert_eq!(get(&scope, "c").as_deref(), Some("fine"));
    assert!(scope.check_reads().is_ok());
}

#[test]
fn a_listing_fails_on_a_cycle() {
    let scope = templated([("a", "${b}"), ("b", "${a}")], []);
    let error = scope
        .lines_for(scope.names(), &host())
        .expect_err("a cycle");
    assert!(error.to_string().contains("a -> b -> a"), "{error}");
}

#[test]
fn a_listing_shows_a_template_beside_its_value_and_a_reason_for_no_value() {
    let scope = templated(
        [
            ("email", "me@${profile}"),
            ("profile", "home"),
            ("broken", "${nope}"),
        ],
        [],
    );
    assert_eq!(
        lines(&scope),
        [
            "broken  = no value (batfiles.toml, template \"${nope}\", `nope` is not declared)",
            "email   = \"me@home\" (batfiles.toml, template \"me@${profile}\")",
            "profile = \"home\" (batfiles.toml)",
        ]
    );
}

#[test]
fn an_override_is_evaluated_in_the_leaf_scope() {
    let base = templated([("profile", "personal")], []);
    let scope = derived(&base, [("profile", "${profile}-corp")]);
    assert_eq!(get(&scope, "profile").as_deref(), Some("personal-corp"));
    assert_eq!(
        override_lines(&scope),
        [
            "profile = \"personal-corp\" (include-remote `corp`, template \"${profile}-corp\"; \
             over batfiles.toml)"
        ]
    );
}

#[test]
fn each_scope_evaluates_a_leaf_template_against_its_own_values() {
    let base = templated([("email", "me@${profile}"), ("profile", "personal")], []);
    let scope = derived(&base, [("profile", "work")]);
    assert_eq!(get(&base, "email").as_deref(), Some("me@personal"));
    assert_eq!(get(&scope, "email").as_deref(), Some("me@work"));
}

#[test]
fn a_cycle_an_override_reads_fails_the_inclusion() {
    let base = templated([("a", "${b}"), ("b", "${a}")], []);
    let overrides = BTreeMap::from([(name("x"), TemplateString::from("${a}".to_owned()))]);
    let error = base
        .with_inclusion(&BTreeMap::new(), None, &overrides, CORP, &host())
        .expect_err("a cycle");
    assert!(error.to_string().contains("a -> b -> a"), "{error}");
}

#[test]
fn an_override_a_higher_layer_overrides_is_never_evaluated() {
    let base = templated([("a", "${b}"), ("b", "${a}")], [("pick", "valid")]);
    let overrides = BTreeMap::from([(name("pick"), TemplateString::from("${a}".to_owned()))]);
    let scope = Rc::new(
        base.with_inclusion(&BTreeMap::new(), None, &overrides, CORP, &host())
            .expect("the override is not evaluated"),
    );
    assert_eq!(get(&scope, "pick").as_deref(), Some("valid"));
    assert_eq!(
        override_lines(&scope),
        ["pick = \"valid\" (--var; over include-remote `corp`)"]
    );
}

/// A leaf whose `v1` reads `v2` and so on, through `templates` templates, ending at a literal.
fn chained(templates: usize) -> Rc<VarScope> {
    let manifest = (1..=templates + 1)
        .map(|index| {
            let value = if index > templates {
                "end".to_owned()
            } else {
                format!("${{v{}}}", index + 1)
            };
            (
                name(&format!("v{index}")),
                VarValue::written(&TemplateString::from(value)),
            )
        })
        .collect();
    Rc::new(VarScope::stack(
        manifest,
        BTreeMap::new(),
        BTreeMap::new(),
        &[],
    ))
}

#[test]
fn a_read_may_nest_as_many_templates_as_the_limit() {
    let scope = chained(MAX_DEPTH);
    assert_eq!(get(&scope, "v1").as_deref(), Some("end"));
    assert!(scope.check_reads().is_ok());
}

#[test]
fn a_read_nesting_more_templates_than_the_limit_fails() {
    let scope = chained(MAX_DEPTH + 1);
    assert!(scope.lookup("v1", &host()).is_some_and(|it| it.is_err()));
    let error = scope.check_reads().expect_err("too deep").to_string();
    assert!(
        error.contains("too deeply: v1 -> v2 -> ")
            && error.contains(&format!(" -> v{}; ", MAX_DEPTH + 1))
            && error.contains(&format!("at most {MAX_DEPTH} templated")),
        "{error}"
    );
}
