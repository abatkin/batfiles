use super::*;

use crate::vars::name::VarName;

/// A single-layer variable scope.
fn vars(bindings: &[(&str, &str)]) -> Rc<VarScope> {
    let manifest = bindings
        .iter()
        .map(|(name, value)| {
            (
                VarName::try_from((*name).to_owned()).expect("valid name"),
                crate::vars::scope::VarValue::Static((*value).to_owned()),
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

fn host(environment: &[(&str, &str)]) -> HostNamespaces {
    HostNamespaces::capture(&Environment::from_pairs(environment.iter().copied()))
}

fn condition(source: &str) -> Condition {
    Condition::new(source).unwrap_or_else(|error| panic!("`{source}`: {error}"))
}

/// Evaluate `source` against the given bindings, expecting it to succeed.
fn truth(source: &str, bindings: &Bindings<'_>) -> bool {
    eval(&condition(source), bindings).unwrap_or_else(|error| panic!("{error}"))
}

fn failure(source: &str, bindings: &Bindings<'_>) -> EvalError {
    match eval(&condition(source), bindings) {
        Ok(value) => panic!("`{source}` should not evaluate, got {value}"),
        Err(error) => error,
    }
}

/// The whole line a reader sees for a condition that cannot be decided.
fn reported(source: &str, bindings: &Bindings<'_>) -> String {
    let parsed = condition(source);
    Gate::When(&parsed).evaluation_failure_reason(Some(CONSEQUENCE), &failure(source, bindings))
}

/// Stands in for a caller's clause; the wording belongs to the caller.
const CONSEQUENCE: &str = "it is not installed";

#[test]
fn a_condition_keeps_the_text_it_was_written_as() {
    for source in ["work", "work && facts.os == 'macos'", "a&&b", "a  &&  b"] {
        assert_eq!(condition(source).source(), source);
    }
}

#[test]
fn a_malformed_condition_is_rejected_where_it_is_read() {
    let error = Condition::new("work &&").expect_err("an incomplete expression");
    let message = error.to_string();
    assert!(message.contains("is not a valid condition"), "{message}");
    assert!(message.contains("at character"), "{message}");
}

#[test]
fn a_rejection_cannot_break_the_line_it_is_rendered_in() {
    // Literal newlines in TOML multiline strings must stay on one diagnostic line.
    let error = Condition::new("work &&\n'oops").expect_err("an unterminated string");
    let message = error.to_string();
    assert!(!message.contains('\n'), "{message}");
    assert!(message.contains("\\n"), "{message}");
    assert!(message.contains("at line 2"), "{message}");
}

#[test]
fn a_condition_deserializes_as_a_field_of_a_document() {
    #[derive(Debug, Deserialize)]
    struct Record {
        when: Condition,
    }

    let record: Record = toml::from_str("when = \"work\"\n").expect("deserialize");
    assert_eq!(record.when.source(), "work");

    // A malformed condition deserializes, keeping its error for validation to report.
    let record = toml::from_str::<Record>("when = \"work &&\"\n")
        .expect("a malformed condition is still a string");
    let error = record.when.syntax().expect_err("it does not parse");
    assert!(
        error.to_string().contains("is not a valid condition"),
        "{error}"
    );

    // Validation refuses it at load; evaluation would close the gate with the same error.
    let vars = vars(&[("work", "true")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);
    let EvalError::Failed { message } = eval(&record.when, &bindings).expect_err("no value") else {
        panic!("expected an evaluation failure");
    };
    assert!(message.starts_with("is not a valid condition"), "{message}");
}

#[test]
fn a_declared_variable_resolves_to_its_value() {
    let vars = vars(&[("profile", "personal"), ("work", "true")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth("profile == 'personal'", &bindings));
    assert!(truth("profile != 'work'", &bindings));
    assert!(truth("work && profile == 'personal'", &bindings));
}

#[test]
fn a_bare_identifier_nothing_declares_is_an_error() {
    let vars = vars(&[("declared", "yes")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth("declared", &bindings));
    assert_eq!(
        failure("undeclared", &bindings),
        EvalError::Undeclared {
            name: "undeclared".to_owned(),
        }
    );
}

#[test]
fn a_declared_empty_value_is_false_rather_than_undeclared() {
    let vars = vars(&[("empty", "")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(!truth("empty", &bindings));
    assert!(truth("empty == ''", &bindings));
}

#[test]
fn vars_is_total_where_the_bare_identifier_is_strict() {
    let vars = vars(&[("valued", "yes")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth("vars.valued", &bindings));
    assert!(truth("vars.valued == 'yes'", &bindings));

    assert!(!truth("vars.undeclared", &bindings));
    assert!(truth("vars.undeclared == ''", &bindings));
    assert!(matches!(
        failure("undeclared", &bindings),
        EvalError::Undeclared { .. }
    ));

    assert!(truth("vars.valued || vars.undeclared", &bindings));
    assert!(!truth("vars.undeclared || vars.nothere", &bindings));
}

#[test]
fn the_member_and_indexed_spellings_of_vars_are_one_lookup() {
    let vars = vars(&[("work", "1")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth("vars.work == vars['work']", &bindings));
    assert!(truth("vars['work']", &bindings));
    // Indexing accepts text no variable name could be, and answers the way
    // any absent key does.
    assert!(truth("vars['has-dash'] == ''", &bindings));
}

#[test]
fn the_vars_namespace_reads_the_layers_rather_than_a_copy_of_them() {
    let manifest = [("editor", "vi")]
        .into_iter()
        .map(|(name, value)| {
            (
                VarName::try_from(name.to_owned()).expect("valid name"),
                crate::vars::scope::VarValue::Static(value.to_owned()),
            )
        })
        .collect();
    let command_line = [(
        VarName::try_from("editor".to_owned()).expect("valid name"),
        "emacs".to_owned(),
    )];
    let vars = Rc::new(VarScope::stack(
        manifest,
        BTreeMap::new(),
        BTreeMap::new(),
        &command_line,
    ));
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth("editor == 'emacs'", &bindings));
    assert!(truth("vars.editor == 'emacs'", &bindings));
}

#[test]
fn facts_answers_both_spellings_and_a_missing_key_is_empty() {
    let vars = vars(&[]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth(
        &format!("facts.os == '{}'", std::env::consts::OS),
        &bindings
    ));
    assert!(truth(
        &format!("facts['arch'] == '{}'", std::env::consts::ARCH),
        &bindings
    ));
    // Unknown namespace keys, including misspellings, return empty strings.
    assert!(!truth("facts.arhc == 'arm64'", &bindings));
    assert!(truth("facts.arhc == ''", &bindings));
}

#[test]
fn the_captured_facts_describe_the_host_running_the_test() {
    let vars = vars(&[]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth("facts.os != ''", &bindings));
    assert!(truth(
        &format!("facts.family == '{}'", std::env::consts::FAMILY),
        &bindings
    ));
    assert!(
        matches!(std::env::consts::FAMILY, "unix" | "windows"),
        "there is a third family now"
    );
}

#[test]
fn the_hostname_is_what_the_platform_reports() {
    let expected = gethostname::gethostname().to_string_lossy().into_owned();
    let vars = vars(&[]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(!expected.is_empty(), "the host has no name");
    assert!(truth(&format!("facts.hostname == '{expected}'"), &bindings));
}

#[test]
fn env_answers_both_spellings_and_a_missing_key_is_empty() {
    let vars = vars(&[]);
    let host = host(&[("HOME", "/home/me"), ("XDG_CURRENT_DESKTOP", "GNOME")]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth("env.HOME == '/home/me'", &bindings));
    assert!(truth("env[\"XDG_CURRENT_DESKTOP\"] == 'GNOME'", &bindings));
    assert!(truth("env.NOPE == ''", &bindings));
    assert!(truth("env.HOME != ''", &bindings));
}

#[test]
fn env_is_the_raw_environment_and_not_the_variable_layer() {
    // Environment variable overrides remain available under their original keys in `env`.
    let vars = vars(&[("FOO", "from the variable layer")]);
    let host = host(&[("BATFILES_VAR_FOO", "raw"), ("BARE", "raw")]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth("FOO == 'from the variable layer'", &bindings));
    assert!(truth("env['BATFILES_VAR_FOO'] == 'raw'", &bindings));
    assert!(truth("env.FOO == ''", &bindings));
    assert!(truth("env.BARE == 'raw'", &bindings));
    assert!(matches!(
        failure("BARE", &bindings),
        EvalError::Undeclared { .. }
    ));
}

// Truthiness.

#[test]
fn the_truthiness_table_is_closed_on_both_sides() {
    let vars = vars(&[]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    for truthy in ["true", "'true'", "'1'", "'yes'", "'on'", "1", "-1", "2.5"] {
        assert!(truth(truthy, &bindings), "{truthy} should be true");
    }
    for falsy in ["false", "'false'", "'0'", "'no'", "'off'", "''", "0", "0.0"] {
        assert!(!truth(falsy, &bindings), "{falsy} should be false");
    }
    for neither in ["'personal'", "'True'", "'y'", "'2'"] {
        assert!(
            matches!(failure(neither, &bindings), EvalError::NotBoolean),
            "{neither} is outside the table"
        );
    }
}

#[test]
fn the_policy_applies_inside_the_operators_too() {
    // Every truthiness decision here is an operand, not a finished value.
    let vars = vars(&[("work", "1"), ("school", "0")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(!truth("work && school", &bindings));
    assert!(truth("work || school", &bindings));
    assert!(!truth("!work", &bindings));
    assert!(truth("!school", &bindings));
    assert!(truth("vars.work || vars.nothere", &bindings));
    assert!(!truth("vars.nothere && vars.work", &bindings));
}

#[test]
fn comparison_keeps_the_languages_own_rules() {
    // The truthiness table governs boolean contexts only, so `==` is
    // unaffected by it.
    let vars = vars(&[("profile", "personal")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth("profile == 'personal'", &bindings));
    assert!(!truth("profile == 'work'", &bindings));
}

#[test]
fn a_value_outside_the_table_reads_the_same_from_either_path() {
    // Exercise both final-result conversion and conversion inside a logical operator.
    let vars = vars(&[("profile", "personal"), ("work", "true")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    for source in ["profile", "profile && work", "!profile"] {
        assert_eq!(
            failure(source, &bindings).to_string(),
            NOT_BOOLEAN,
            "{source}"
        );
        assert!(
            reported(source, &bindings).contains(&quoted_value(source)),
            "{source}"
        );
    }
}

#[test]
fn a_value_outside_the_table_is_never_named_in_the_message() {
    // Check secret suppression both in final results and inside logical operators.
    let vars = vars(&[("token", "s3cret-value"), ("work", "true")]);
    let host = host(&[("GITHUB_TOKEN", "ghp_notarealtoken")]);
    let bindings = Bindings::new(&vars, &host);

    for source in ["token", "token && work", "!token", "env.GITHUB_TOKEN"] {
        let line = reported(source, &bindings);
        assert!(!line.contains("s3cret-value"), "{source}: {line}");
        assert!(!line.contains("ghp_notarealtoken"), "{source}: {line}");
    }
}

#[test]
fn an_evaluation_error_cannot_forge_a_line_of_its_own() {
    let vars = vars(&[]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    // This newline is valid syntax, so it exercises evaluation-error escaping.
    let line = reported("undeclared\n&& work", &bindings);
    assert!(!line.contains('\n'), "{line}");
    assert!(line.contains("\\n"), "{line}");

    let line = reported("'\u{1b}[2Kforged'", &bindings);
    assert!(!line.contains('\u{1b}'), "{line}");
    assert!(line.contains("\\u{1b}"), "{line}");
}

#[test]
fn a_namespace_is_not_itself_a_boolean() {
    let vars = vars(&[]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(matches!(failure("facts", &bindings), EvalError::NotBoolean));
}

#[test]
fn an_undeclared_identifier_names_all_three_fixes() {
    let vars = vars(&[]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    let line = reported("work && facts.os == 'macos'", &bindings);
    assert!(
        line.contains(&quoted_value("work && facts.os == 'macos'")),
        "{line}"
    );
    assert!(line.contains("`work = \"false\"`"), "{line}");
    assert!(line.contains("`batfiles vars set work <value>`"), "{line}");
    assert!(line.contains("`vars.work`"), "{line}");
}

#[test]
fn an_excluding_gate_names_the_spelling_and_the_verdict_that_closed_it() {
    let parsed = condition("work");

    assert_eq!(
        Gate::When(&parsed).exclusion_reason(),
        "when \"work\" is false"
    );
    assert_eq!(
        Gate::Unless(&parsed).exclusion_reason(),
        "unless \"work\" is true"
    );
}

#[test]
fn a_gate_that_cannot_be_decided_names_the_spelling_and_the_cost() {
    // An evaluation failure must close `unless`, not act like a false result.
    let vars = vars(&[]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);
    let parsed = condition("nowhere");
    let error = failure("nowhere", &bindings);

    assert!(
        Gate::When(&parsed)
            .evaluation_failure_reason(Some(CONSEQUENCE), &error)
            .starts_with("when \"nowhere\" cannot be evaluated, so it is not installed: "),
    );
    assert!(
        Gate::Unless(&parsed)
            .evaluation_failure_reason(Some(CONSEQUENCE), &error)
            .starts_with("unless \"nowhere\" cannot be evaluated, so it is not installed: "),
    );
    // A caller whose line has already said what is not happening.
    assert!(
        Gate::Unless(&parsed)
            .evaluation_failure_reason(None, &error)
            .starts_with("unless \"nowhere\" cannot be evaluated: "),
    );
}

#[test]
fn one_evaluation_decides_a_gate_three_ways() {
    let vars = vars(&[("work", "true")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);
    let work = condition("work");
    let nowhere = condition("nowhere");

    let verdict = |gate: Gate<'_>| gate.exclusion(&bindings, Some(CONSEQUENCE));

    assert!(verdict(Gate::When(&work)).is_none());
    let Some(Exclusion::Deliberate(reason)) = verdict(Gate::Unless(&work)) else {
        panic!("a true `unless` closes its gate");
    };
    assert_eq!(reason, "unless \"work\" is true");
    let Some(Exclusion::EvaluationFailed(reason)) = verdict(Gate::When(&nowhere)) else {
        panic!("a condition nothing declares cannot be decided");
    };
    assert!(
        reason.starts_with("when \"nowhere\" cannot be evaluated, so "),
        "{reason}"
    );
}

#[test]
fn short_circuiting_can_leave_an_undeclared_identifier_unreached() {
    let vars = vars(&[("work", "yes")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(truth("work || undeclared", &bindings));
    assert!(matches!(
        failure("undeclared || work", &bindings),
        EvalError::Undeclared { .. }
    ));
}

#[test]
fn an_evaluation_fault_that_is_neither_is_flattened_and_names_the_condition() {
    let vars = vars(&[]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    assert!(matches!(
        failure("vars.keys()", &bindings),
        EvalError::Failed { .. }
    ));
    let line = reported("vars.keys()", &bindings);
    assert!(line.contains(&quoted_value("vars.keys()")), "{line}");
}

#[test]
fn a_failed_lookup_is_reported_without_the_key_or_index_it_computed() {
    let vars = vars(&[("token", "s3cret-value")]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    for source in [
        "{'a': true}[token]",
        "[true][token == 's3cret-value']",
        "[true][7]",
    ] {
        let error = failure(source, &bindings);
        let EvalError::Failed { message } = &error else {
            panic!("{source}: {error:?}");
        };
        for operand in ["s3cret-value", "true", "7", "len"] {
            assert!(!message.contains(operand), "{source}: {message}");
        }
    }
}

#[test]
fn integer_overflow_is_an_evaluation_error_rather_than_an_abort() {
    let vars = vars(&[]);
    let host = host(&[]);
    let bindings = Bindings::new(&vars, &host);

    for source in [
        "9223372036854775807 + 1 == 0",
        "(-9223372036854775807 - 1) - 1 == 0",
        "(-9223372036854775807 - 1) % -1 == 0",
        "9223372036854775807 * 2 == 0",
        "-(-9223372036854775807 - 1) == 0",
    ] {
        let error = failure(source, &bindings);
        assert!(
            matches!(error, EvalError::Failed { .. }),
            "{source}: {error:?}"
        );
        let line = reported(source, &bindings);
        assert!(line.contains("integer overflow"), "{line}");
        assert!(line.contains(&quoted_value(source)), "{line}");
    }

    // Division promotes to floating point rather than overflowing, but the
    // zero divisor still has to be an error and not a trap.
    assert!(matches!(
        failure("1 / 0 == 0", &bindings),
        EvalError::Failed { .. }
    ));
}
