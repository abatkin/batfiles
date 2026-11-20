use super::*;
use crate::manifest::actions::Action;

/// The trigger of a `run` record written with `trigger`, or the error reading it.
fn trigger(trigger: &str) -> Result<Trigger, String> {
    let record = format!("type = \"run\"\nid = \"r\"\ncommand = \"true\"\ntrigger = {trigger}\n");
    match toml::from_str::<Action>(&record) {
        Ok(Action::Run(action)) => Ok(action.trigger),
        Ok(_) => panic!("a run record"),
        Err(error) => Err(error.to_string()),
    }
}

fn id(text: &str) -> ItemId {
    ItemId::try_from(text.to_owned()).expect("a valid ID")
}

#[test]
fn a_run_action_runs_always_unless_it_says_otherwise() {
    let Action::Run(action) =
        toml::from_str("type = \"run\"\ncommand = \"true\"\n").expect("parse")
    else {
        panic!("a run record");
    };
    assert_eq!(action.trigger, Trigger::Always);
    assert!(!action.allow_failure);
    assert_eq!(action.command_timeout, None);
}

#[test]
fn every_trigger_kind_is_read() {
    assert_eq!(trigger("\"always\""), Ok(Trigger::Always));
    assert_eq!(trigger("\"bootstrap\""), Ok(Trigger::Bootstrap));
    assert_eq!(trigger("\"once\""), Ok(Trigger::Once));
    assert_eq!(
        trigger("{ every = \"24h\" }"),
        Ok(Trigger::Every(FriendlyDuration::new("24h").expect("valid")))
    );
    assert_eq!(
        trigger("{ after = \"mise\" }"),
        Ok(Trigger::After(vec![AfterTarget::Action(id("mise"))]))
    );
    assert_eq!(
        trigger("{ after = [\"mise\", \"@tools\"] }"),
        Ok(Trigger::After(vec![
            AfterTarget::Action(id("mise")),
            AfterTarget::Remote(id("tools")),
        ]))
    );
}

#[test]
fn a_zero_interval_is_valid() {
    assert!(matches!(
        trigger("{ every = \"0s\" }"),
        Ok(Trigger::Every(_))
    ));
}

#[test]
fn malformed_triggers_are_refused() {
    for (written, expected) in [
        ("\"daily\"", "`daily` is not a trigger"),
        ("{ after = [] }", "an empty `after` list"),
        ("{ every = \"1h\", after = \"a\" }", "not both"),
        ("{}", "writes `every` or `after`"),
        ("{ every = \"soon\" }", "not a valid duration"),
        ("{ before = \"a\" }", "unknown field `before`"),
        ("{ after = \"@\" }", "`after` target `@`"),
        ("3", "expected `\"always\"`"),
    ] {
        let error = trigger(written).expect_err(written);
        assert!(error.contains(expected), "{written}: {error}");
    }
}

#[test]
fn only_always_and_bootstrap_keep_no_state() {
    assert!(!Trigger::Always.keeps_state());
    assert!(!Trigger::Bootstrap.keeps_state());
    assert!(Trigger::Once.keeps_state());
    assert!(Trigger::After(vec![AfterTarget::Remote(id("a"))]).keeps_state());
}

#[test]
fn a_command_is_shown_as_written() {
    let shown = |command: &str| {
        let Action::Run(action) =
            toml::from_str(&format!("type = \"run\"\ncommand = {command}\n")).expect("parse")
        else {
            panic!("a run record");
        };
        action.command.to_string()
    };
    assert_eq!(shown("\"make ${vars.target}\""), "make ${vars.target}");
    assert_eq!(
        shown("[\"~/.local/bin/mise\", \"install\", \"a b\", \"\"]"),
        "~/.local/bin/mise install \"a b\" \"\""
    );
}

#[test]
fn an_empty_command_list_and_a_zero_timeout_are_refused() {
    let error = toml::from_str::<Action>("type = \"run\"\ncommand = []\n").expect_err("empty");
    assert!(error.to_string().contains("nothing to run"), "{error}");
    let error =
        toml::from_str::<Action>("type = \"run\"\ncommand = \"true\"\ncommand-timeout = \"0s\"\n")
            .expect_err("zero");
    assert!(error.to_string().contains("greater than zero"), "{error}");
}

#[test]
fn a_run_action_reads_its_directory_and_environment() {
    let Action::Run(action) = toml::from_str(
        "type = \"run\"\ncommand = \"true\"\ncwd = \"~/src\"\nenv = { TOOL_HOME = \"~/t\", PATH = \"${env.PATH}\" }\n",
    )
    .expect("parse") else {
        panic!("a run record");
    };
    assert_eq!(
        action.cwd.as_ref().map(TemplateString::source),
        Some("~/src")
    );
    let names: Vec<&str> = action.env.keys().map(EnvName::as_str).collect();
    assert_eq!(names, ["PATH", "TOOL_HOME"]);
}

#[test]
fn an_environment_name_is_not_empty_and_holds_no_equals_sign() {
    for written in ["{ \"\" = \"x\" }", "{ \"A=B\" = \"x\" }"] {
        let error = toml::from_str::<Action>(&format!(
            "type = \"run\"\ncommand = \"true\"\nenv = {written}\n"
        ))
        .expect_err(written);
        assert!(
            error
                .to_string()
                .contains("is not an environment variable name"),
            "{error}"
        );
    }
    let error = toml::from_str::<Action>("type = \"run\"\ncommand = \"true\"\nenv = { A = 1 }\n")
        .expect_err("not a string");
    assert!(error.to_string().contains("invalid type"), "{error}");
}
