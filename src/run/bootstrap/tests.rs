use super::*;
use crate::manifest::condition::HostNamespaces;
use crate::output::Verbosity;
use crate::vars::scope::VarScope;
use std::collections::BTreeMap;
use std::rc::Rc;

/// Create a reporter that suppresses informational output.
fn quiet() -> Reporter {
    let mut reporter = Reporter::new(false);
    reporter.set_verbosity(Verbosity::Quiet);
    reporter
}

fn options<const N: usize>(pairs: [(&str, &str); N]) -> BootstrapOptions {
    let mut options = BootstrapOptions {
        enable_actions: Vec::new(),
        disable_actions: Vec::new(),
        enable_groups: Vec::new(),
        disable_groups: Vec::new(),
    };
    for (option, value) in pairs {
        let list = match option {
            "--enable-action" => &mut options.enable_actions,
            "--disable-action" => &mut options.disable_actions,
            "--enable-group" => &mut options.enable_groups,
            "--disable-group" => &mut options.disable_groups,
            other => panic!("no such option: {other}"),
        };
        list.push(value.to_owned());
    }
    options
}

fn env<const N: usize>(pairs: [(&str, &str); N]) -> Environment {
    Environment::from_pairs(pairs)
}

fn read<const N: usize, const M: usize>(
    pairs: [(&str, &str); N],
    variables: [(&str, &str); M],
) -> BootstrapDecisions {
    BootstrapDecisions::read(&options(pairs), &env(variables), &quiet()).expect("valid addresses")
}

/// Format each decision as `origin name`.
fn decisions(bootstrap: &BootstrapDecisions) -> Vec<String> {
    bootstrap
        .decisions
        .iter()
        .map(|decision| format!("{} {}", decision.origin, decision.name))
        .collect()
}

/// Parse the `[default-disabled]` section from a complete manifest.
fn candidates(manifest: &str) -> DefaultDisabled {
    toml::from_str::<crate::manifest::Manifest>(manifest)
        .expect("valid candidates")
        .default_disabled
}

/// A machine whose state roots are a fresh temporary directory.
fn machine(dir: &tempfile::TempDir) -> StateRoots {
    StateRoots {
        config_dir: dir.path().to_path_buf(),
        cache_dir: dir.path().to_path_buf(),
    }
}

/// Adopt against the given variables, and answer with the document that was
/// left behind, or `None` where none was written.
fn adopt(
    bootstrap: &BootstrapDecisions,
    candidates: &DefaultDisabled,
    vars: &[(&str, &str)],
    dir: &tempfile::TempDir,
) -> Option<DisabledItems> {
    adopt_in(bootstrap, candidates, vars, dir, RunMode::Perform).1
}

/// Adopt in `mode`, answering with the state the run selects with and the document left
/// behind.
fn adopt_in(
    bootstrap: &BootstrapDecisions,
    candidates: &DefaultDisabled,
    vars: &[(&str, &str)],
    dir: &tempfile::TempDir,
    mode: RunMode,
) -> (DisabledItems, Option<DisabledItems>) {
    let values: BTreeMap<_, _> = vars
        .iter()
        .map(|(name, value)| {
            (
                crate::vars::name::VarName::try_from((*name).to_owned()).expect("valid name"),
                crate::vars::scope::VarValue::Static((*value).to_owned()),
            )
        })
        .collect();
    let variables = Rc::new(VarScope::stack(
        values,
        BTreeMap::new(),
        BTreeMap::new(),
        &[],
    ));
    let host = HostNamespaces::capture(&env([]));
    let roots = machine(dir);
    let adopted = bootstrap
        .adopt(
            candidates,
            &Bindings::new(&variables, &host),
            &roots,
            mode,
            &quiet(),
        )
        .expect("adoption should succeed");
    let written = roots
        .disabled_path()
        .exists()
        .then(|| DisabledItems::load(&roots.disabled_path()).expect("the written document"));
    (adopted, written)
}

fn temp() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn names(addresses: &std::collections::BTreeSet<ItemAddress>) -> Vec<String> {
    addresses.iter().map(ToString::to_string).collect()
}

#[test]
fn the_decisions_are_ordered_by_the_precedence_they_apply_in() {
    let bootstrap = read(
        [("--enable-action", "c"), ("--disable-action", "d")],
        [
            ("BATFILES_ENABLE_ACTIONS", "a"),
            ("BATFILES_DISABLE_ACTIONS", "b"),
        ],
    );
    assert_eq!(
        decisions(&bootstrap),
        [
            "BATFILES_DISABLE_ACTIONS b",
            "BATFILES_ENABLE_ACTIONS a",
            "--disable-action d",
            "--enable-action c",
        ]
    );
}

#[test]
fn both_namespaces_are_read_from_both_sources() {
    let bootstrap = read(
        [("--disable-group", "gui")],
        [("BATFILES_ENABLE_GROUPS", "shell")],
    );
    assert_eq!(
        decisions(&bootstrap),
        ["BATFILES_ENABLE_GROUPS shell", "--disable-group gui"]
    );
}

#[test]
fn an_unusable_variable_name_is_dropped_and_the_rest_are_kept() {
    let bootstrap = read([], [("BATFILES_DISABLE_ACTIONS", "p10k,my action,zshrc")]);
    assert_eq!(
        decisions(&bootstrap),
        [
            "BATFILES_DISABLE_ACTIONS p10k",
            "BATFILES_DISABLE_ACTIONS zshrc"
        ]
    );
}

#[test]
fn an_unusable_option_value_fails_the_command() {
    let error = BootstrapDecisions::read(
        &options([("--disable-action", "core..p10k")]),
        &env([]),
        &quiet(),
    )
    .expect_err("an empty segment is not an address");
    assert!(error.to_string().contains("`core..p10k`"), "{error}");
}

#[test]
fn a_fresh_machine_takes_the_candidates_the_repository_declared() {
    let dir = temp();
    let document = adopt(
        &read([], []),
        &candidates(
            "[[default-disabled.actions]]\nid = \"p10k\"\n\n\
                 [[default-disabled.groups]]\ngroup = \"gui\"\n",
        ),
        &[],
        &dir,
    )
    .expect("a document");
    assert_eq!(names(&document.actions), ["p10k"]);
    assert_eq!(names(&document.groups), ["gui"]);
}

#[test]
fn a_machine_that_already_has_a_document_is_not_offered_them() {
    let dir = temp();
    let roots = machine(&dir);
    DisabledItems::default()
        .save(&roots.disabled_path())
        .expect("an existing document");

    let document = adopt(
        &read([("--disable-group", "gui")], []),
        &candidates("[[default-disabled.actions]]\nid = \"p10k\"\n"),
        &[],
        &dir,
    )
    .expect("a document");

    assert!(document.actions.is_empty());
    assert_eq!(names(&document.groups), ["gui"]);
}

#[test]
fn deciding_nothing_leaves_no_document_behind() {
    let dir = temp();
    assert!(adopt(&read([], []), &candidates(""), &[], &dir).is_none());
}

#[test]
fn an_explicit_enable_outranks_the_candidate_that_named_it() {
    let dir = temp();
    let document = adopt(
        &read([("--enable-action", "p10k")], []),
        &candidates("[[default-disabled.actions]]\nid = \"p10k\"\n"),
        &[],
        &dir,
    );
    assert!(document.is_none_or(|document| document.actions.is_empty()));
}

#[test]
fn the_command_line_outranks_the_environment() {
    let dir = temp();
    let document = adopt(
        &read(
            [("--disable-action", "p10k")],
            [("BATFILES_ENABLE_ACTIONS", "p10k")],
        ),
        &candidates("[[default-disabled.actions]]\nid = \"p10k\"\n"),
        &[],
        &dir,
    )
    .expect("a document");
    assert_eq!(names(&document.actions), ["p10k"]);
}

#[test]
fn enable_outranks_disable_within_one_source() {
    let dir = temp();
    let document = adopt(
        &read(
            [("--disable-action", "p10k"), ("--enable-action", "p10k")],
            [],
        ),
        &candidates(""),
        &[],
        &dir,
    );
    assert!(document.is_none_or(|document| document.actions.is_empty()));
}

#[test]
fn a_candidate_is_offered_only_where_its_condition_admits_it() {
    let declared = candidates(
        "[[default-disabled.actions]]\nid = \"p10k\"\nwhen = \"slow\"\n\n\
             [[default-disabled.actions]]\nid = \"nvim\"\nunless = \"slow\"\n",
    );

    let slow = temp();
    let document =
        adopt(&read([], []), &declared, &[("slow", "true")], &slow).expect("the `when` candidate");
    assert_eq!(names(&document.actions), ["p10k"]);

    let quick = temp();
    let document = adopt(&read([], []), &declared, &[("slow", "false")], &quick)
        .expect("the `unless` candidate");
    assert_eq!(names(&document.actions), ["nvim"]);
}

#[test]
fn a_condition_this_machine_cannot_decide_leaves_the_candidate_alone() {
    let dir = temp();
    let document = adopt(
        &read([], []),
        &candidates("[[default-disabled.actions]]\nid = \"p10k\"\nwhen = \"nowhere\"\n"),
        &[],
        &dir,
    );
    assert!(document.is_none(), "an undecidable candidate was adopted");
}

#[test]
fn a_dry_run_adopts_for_the_run_and_writes_nothing() {
    let dir = temp();
    let candidates = candidates("[[default-disabled.actions]]\nid = \"p10k\"\n");
    let bootstrap =
        BootstrapDecisions::read(&options([]), &env([]), &quiet()).expect("no decisions");
    let (adopted, written) = adopt_in(&bootstrap, &candidates, &[], &dir, RunMode::DryRun);
    assert_eq!(names(&adopted.actions), ["p10k"]);
    assert!(written.is_none(), "a dry run wrote disabled.toml");
}

#[test]
fn every_line_names_what_asked_for_the_change() {
    let name = ItemAddress::try_from("p10k".to_owned()).expect("valid address");
    assert_eq!(
        line(
            ItemKind::Action,
            Change::Disable,
            &name,
            true,
            DECLARED,
            RunMode::Perform
        ),
        "default-disabled: disabled action `p10k`"
    );
    assert_eq!(
        line(
            ItemKind::Action,
            Change::Enable,
            &name,
            false,
            "--enable-action",
            RunMode::Perform
        ),
        "--enable-action: action `p10k` was already enabled"
    );
    assert_eq!(
        line(
            ItemKind::Action,
            Change::Disable,
            &name,
            true,
            DECLARED,
            RunMode::DryRun
        ),
        "default-disabled: would disable action `p10k`"
    );
    assert_eq!(
        line(
            ItemKind::Group,
            Change::Enable,
            &name,
            true,
            "--enable-group",
            RunMode::DryRun
        ),
        "--enable-group: would enable group `p10k`, which is disabled"
    );
    assert_eq!(heading(ItemKind::Group, &name), "candidate group `p10k`");
}
