use std::collections::BTreeMap;
use std::rc::Rc;

use super::*;
use crate::env::Environment;
use crate::manifest::condition::HostNamespaces;
use crate::vars::name::VarName;
use crate::vars::scope::{VarScope, VarValue};

/// A single-layer variable scope.
fn vars(bindings: &[(&str, &str)]) -> Rc<VarScope> {
    let manifest = bindings
        .iter()
        .map(|(name, value)| {
            (
                VarName::try_from((*name).to_owned()).expect("valid name"),
                VarValue::Static((*value).to_owned()),
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

fn template(source: &str) -> TemplateString {
    let value = TemplateString::from(source.to_owned());
    if let Err(error) = value.syntax() {
        panic!("`{source}`: {error}");
    }
    value
}

/// Resolve `value` against `rule` with `profile = "work"` declared and `HOME_SECRET` in the
/// environment.
fn resolved_as(
    value: &TemplateString,
    rule: ValueRule,
    env: EnvAccess<'_>,
) -> Result<String, TemplateError> {
    let scope = vars(&[("profile", "work"), ("count", "3")]);
    let host = host(&[("HOME_SECRET", "hunter2")]);
    let bindings = Bindings::new(&scope, &host);
    value
        .resolve(rule, &TemplateScope::new(&bindings, env))
        .map(Cow::into_owned)
}

/// Resolve `source` as a destination.
fn resolved(source: &str, env: EnvAccess<'_>) -> Result<String, TemplateError> {
    resolved_as(&template(source), ValueRule::Destination, env)
}

fn failure(source: &str) -> String {
    resolved(source, EnvAccess::Allowed)
        .expect_err("the template should fail")
        .to_string()
}

#[test]
fn a_template_keeps_the_text_it_was_written_as() {
    let written = "~/.config/${ profile }/rc";
    assert_eq!(template(written).source(), written);
}

#[test]
fn a_value_without_an_expression_is_literal_after_escapes() {
    assert_eq!(template("~/.zshrc").literal(), Some("~/.zshrc"));
    assert_eq!(template("").literal(), Some(""));
    assert_eq!(template("a$b$$c").literal(), Some("a$b$$c"));
    assert_eq!(template("~/$${HOME}").literal(), Some("~/${HOME}"));
    assert_eq!(template("~/${profile}").literal(), None);
}

#[test]
fn a_malformed_expression_keeps_its_error_and_its_text() {
    for (source, position) in [
        ("~/${profile", "at character 12"),
        ("~/${profile ==}", "at character"),
        ("~/a\n${'x' +}", "at line 2, character"),
    ] {
        let value = TemplateString::from(source.to_owned());
        let message = value
            .syntax()
            .expect_err("the template should be refused")
            .to_string();
        assert!(message.contains("is not a valid template"), "{message}");
        assert!(message.contains(position), "{message}");
        assert_eq!(value.source(), source);
        assert_eq!(value.literal(), None);

        // Validation refuses it at load; evaluation would report the same error.
        let message = resolved_as(&value, ValueRule::Destination, EnvAccess::Allowed)
            .expect_err("a malformed template cannot be resolved")
            .to_string();
        assert!(message.starts_with("is not a valid template"), "{message}");
        assert!(message.contains(position), "{message}");
    }
}

#[test]
fn a_malformed_template_deserializes_for_validation_to_report() {
    let document = toml::from_str::<BTreeMap<String, TemplateString>>("dest = \"~/${x +}\"\n")
        .expect("a malformed template is still a string");
    assert!(document["dest"].syntax().is_err());
}

#[test]
fn expressions_render_strings_numbers_and_booleans() {
    assert_eq!(
        resolved(
            "~/${profile}/${2 * 3}/${profile == 'work'}",
            EnvAccess::Allowed
        )
        .expect("it resolves"),
        "~/work/6/true"
    );
    assert_eq!(
        resolved("~/$${profile}/${profile}", EnvAccess::Allowed).expect("it resolves"),
        "~/${profile}/work"
    );
}

#[test]
fn a_bare_identifier_nothing_declares_is_an_error_where_it_is_written() {
    let message = failure("~/${profile}/${missing}");
    assert!(
        message.contains("cannot be evaluated at character 14"),
        "{message}"
    );
    assert!(message.contains("`missing` is not declared"), "{message}");
}

#[test]
fn an_optional_variable_is_empty_and_the_field_rule_still_applies() {
    let message = failure("${vars.missing}");
    assert!(
        message.starts_with("resolves to a value that is empty"),
        "{message}"
    );
}

#[test]
fn a_value_that_is_not_a_scalar_is_refused_without_being_shown() {
    for source in ["~/${[profile]}", "~/${{'a': profile}}", "~/${facts}"] {
        let message = failure(source);
        assert!(
            message.contains("not a string, number, or boolean"),
            "{source}: {message}"
        );
        assert!(!message.contains("work"), "{source}: {message}");
    }
}

#[test]
fn a_failed_lookup_keeps_its_kind_and_position_but_not_its_operands() {
    for (source, kind) in [
        (
            "~/${{'a': 'b'}[env.HOME_SECRET]}",
            "a dictionary has no entry for the key looked up in it",
        ),
        ("~/${['a'][5]}", "a list index is out of range"),
        (
            "~/${['a'][env.HOME_SECRET == 'hunter2']}",
            "indexed by something other than a string or integer",
        ),
    ] {
        let message = failure(source);
        assert!(message.contains("at character 3"), "{source}: {message}");
        assert!(message.contains(kind), "{source}: {message}");
        for operand in ["hunter2", "true", "5", "len"] {
            assert!(!message.contains(operand), "{source}: {message}");
        }
    }
}

#[test]
fn a_resolved_value_follows_the_rule_a_literal_one_does_without_being_shown() {
    let message = failure("~${profile}/rc");
    assert!(
        message.starts_with("resolves to a value that names another user's home"),
        "{message}"
    );
    assert!(!message.contains("work"), "{message}");

    let message = resolved_as(
        &template("${env.HOME_SECRET}"),
        ValueRule::Digest,
        EnvAccess::Allowed,
    )
    .expect_err("a secret is not a digest")
    .to_string();
    assert_eq!(
        message,
        "resolves to a value that is not 64 hexadecimal digits"
    );
}

#[test]
fn a_contributed_template_reads_env_only_with_permission() {
    let remote = ItemId::try_from("corporate".to_owned()).expect("valid ID");
    for source in [
        "~/${env.HOME_SECRET}",
        "~/${env['HOME_SECRET']}",
        "~/${env[0]}",
        "~/${profile + env.HOME_SECRET}",
        "~/${[env][0].HOME_SECRET}",
    ] {
        let message = resolved(source, EnvAccess::DeniedBy(&remote))
            .expect_err("env should be refused")
            .to_string();
        assert!(message.contains("at character 3"), "{source}: {message}");
        assert!(
            message.contains("`[remotes.corporate]` sets `allow-env-templates = true`"),
            "{source}: {message}"
        );
        assert!(!message.contains("hunter2"), "{source}: {message}");
    }
    for source in ["~/${env}", "~/${env.keys()}", "~/${env()}"] {
        let message = resolved(source, EnvAccess::DeniedBy(&remote))
            .expect_err("a denied env has nothing to give")
            .to_string();
        assert!(!message.contains("hunter2"), "{source}: {message}");
    }
    for (source, value) in [("~/x${vars.env}", "~/x"), ("~/${'env'}", "~/env")] {
        assert_eq!(
            resolved(source, EnvAccess::DeniedBy(&remote)).expect("it resolves"),
            value
        );
    }
    assert_eq!(
        resolved("~/${env.HOME_SECRET}", EnvAccess::Allowed).expect("it resolves"),
        "~/hunter2"
    );
    assert_eq!(
        resolved("~/${profile}", EnvAccess::DeniedBy(&remote)).expect("it resolves"),
        "~/work"
    );
}

#[test]
fn a_literal_is_returned_without_evaluation_or_checks() {
    let remote = ItemId::try_from("corporate".to_owned()).expect("valid ID");
    assert_eq!(
        resolved("~/$${env.HOME}", EnvAccess::DeniedBy(&remote)).expect("it resolves"),
        "~/${env.HOME}"
    );
}
