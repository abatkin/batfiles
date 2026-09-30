//! Parse and evaluate `when`/`unless` conditions using [Batfiles truthiness
//! rules](../docs/repoformat.md#conditions). Bare identifiers require declared variables;
//! missing namespace keys return empty strings. Evaluation uses inputs captured by
//! [`HostNamespaces::capture`] and performs no I/O.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use serde::Deserialize;
use simple_expressions::evaluator::{Evaluator, VariableResolver};
use simple_expressions::parser::parse_expression;
use simple_expressions::types::coerce::{Coercions, Number, STANDARD};
use simple_expressions::types::error::{Error as ExpressionError, Result as ExpressionResult};
use simple_expressions::types::expression::Expr;
use simple_expressions::types::object::Object;
use simple_expressions::types::primitive::Primitive;
use simple_expressions::types::value::Value;

use crate::env::Environment;
use crate::output::{Reporter, quoted_value};
use crate::var_set::VarSet;

/// The reserved namespace of host facts.
const FACTS: &str = "facts";
/// The reserved namespace of host environment variables.
const ENV: &str = "env";
/// Reserved namespace for user variables; missing keys return empty strings.
const VARS: &str = "vars";

/// A parsed condition with its original text retained for diagnostics.
#[derive(Clone, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Condition {
    source: String,
    expr: Expr,
}

impl Condition {
    /// Parse a condition, returning an error for invalid syntax.
    pub fn new(source: &str) -> Result<Self, ConditionError> {
        Self::try_from(source.to_owned())
    }

    /// The condition exactly as written, whitespace and quote style included.
    pub fn source(&self) -> &str {
        &self.source
    }
}

impl TryFrom<String> for Condition {
    type Error = ConditionError;

    fn try_from(source: String) -> Result<Self, Self::Error> {
        let expr =
            parse_expression(&source).map_err(|error| ConditionError::new(&source, &error))?;
        Ok(Self { source, expr })
    }
}

/// Debug output shows the original condition text.
impl fmt::Debug for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Condition").field(&self.source).finish()
    }
}

/// A condition and whether true (`when`) or false (`unless`) admits the record.
/// Evaluation failures exclude the record in either case.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Gate<'a> {
    When(&'a Condition),
    Unless(&'a Condition),
}

impl<'a> Gate<'a> {
    /// Construct a gate from mutually exclusive fields, or `None` if both are absent.
    /// Callers must reject records declaring both fields during document validation.
    pub fn from_fields(when: Option<&'a Condition>, unless: Option<&'a Condition>) -> Option<Self> {
        when.map(Self::When).or_else(|| unless.map(Self::Unless))
    }

    /// Return `None` if the gate admits the record, otherwise its exclusion.
    /// Evaluation failures also exclude the record. `consequence` supplies an optional
    /// description of the skipped work for the failure diagnostic.
    pub fn exclusion(
        self,
        bindings: &Bindings<'_>,
        consequence: Option<&str>,
    ) -> Option<Exclusion> {
        match self.admits(bindings) {
            Ok(true) => None,
            Ok(false) => Some(Exclusion::Deliberate(self.exclusion_reason())),
            Err(error) => Some(Exclusion::EvaluationFailed(
                self.evaluation_failure_reason(consequence, &error),
            )),
        }
    }

    /// Evaluate whether the condition admits the record.
    fn admits(self, bindings: &Bindings<'_>) -> Result<bool, EvalError> {
        match self {
            Self::When(condition) => eval(condition, bindings),
            Self::Unless(condition) => Ok(!eval(condition, bindings)?),
        }
    }

    /// Describe why a closed gate excludes its record, quoting the condition.
    /// The caller must have established that the gate is closed.
    pub fn exclusion_reason(self) -> String {
        let verdict = match self {
            Self::When(_) => "false",
            Self::Unless(_) => "true",
        };
        format!(
            "{} {} is {verdict}",
            self.field(),
            quoted_value(self.condition().source())
        )
    }

    /// Format an evaluation failure with the condition and optional consequence.
    fn evaluation_failure_reason(self, consequence: Option<&str>, error: &EvalError) -> String {
        let condition = quoted_value(self.condition().source());
        let consequence = consequence.map_or_else(String::new, |what| format!(", so {what}"));
        format!(
            "{} {condition} cannot be evaluated{consequence}: {error}",
            self.field()
        )
    }

    /// The condition field name: `when` or `unless`.
    fn field(self) -> &'static str {
        match self {
            Self::When(_) => "when",
            Self::Unless(_) => "unless",
        }
    }

    /// The condition itself, whichever field carried it.
    fn condition(self) -> &'a Condition {
        match self {
            Self::When(condition) | Self::Unless(condition) => condition,
        }
    }
}

/// A reason to skip a record, classified for verbose output or a warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Exclusion {
    /// An explicit disable, run-only skip, or condition that excludes the record.
    Deliberate(String),
    /// A condition evaluation failure that excludes the record.
    EvaluationFailed(String),
}

impl Exclusion {
    /// Report caller-supplied wording at `-v` for expected exclusions, or as a
    /// warning at every verbosity for evaluation failures.
    pub fn report(&self, reporter: &Reporter, message: &str) {
        match self {
            Self::Deliberate(_) => reporter.detail(1, message),
            Self::EvaluationFailed(_) => reporter.warn(message),
        }
    }

    /// Report an action or remote exclusion with its heading and reason, at the
    /// severity [`Self::report`] gives it.
    pub fn report_heading(&self, reporter: &Reporter, heading: &str) {
        let reason = self.reason();
        let message = match self {
            Self::Deliberate(_) => format!("{heading} - skipped: {reason}"),
            Self::EvaluationFailed(_) => format!("{heading}: {reason}"),
        };
        self.report(reporter, &message);
    }

    /// The reason, for a caller composing the line that names the record.
    pub fn reason(&self) -> &str {
        match self {
            Self::Deliberate(reason) | Self::EvaluationFailed(reason) => reason,
        }
    }
}

/// A condition parse error, formatted on one line without a caret diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConditionError {
    candidate: String,
    /// Parser error position, if available.
    position: Option<Position>,
    message: String,
}

/// Where within the condition the parser stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Position {
    /// One-based line number within the condition.
    line: usize,
    /// 1-based, in characters.
    column: usize,
}

impl ConditionError {
    fn new(candidate: &str, error: &ExpressionError) -> Self {
        // Non-parse errors have no position but still retain their message.
        let (position, message) = match error {
            ExpressionError::ParseError {
                line,
                column,
                message,
                ..
            } => (
                Some(Position {
                    line: *line,
                    column: *column,
                }),
                message.clone(),
            ),
            other => (None, other.to_string()),
        };
        Self {
            candidate: candidate.to_owned(),
            position,
            message,
        }
    }
}

impl fmt::Display for ConditionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Escape condition text so embedded newlines cannot alter the enclosing TOML
        // diagnostic.
        write!(
            f,
            "{} is not a valid condition: {}",
            quoted_value(&self.candidate),
            self.message
        )?;
        match self.position {
            Some(Position { line, column }) if line > 1 => {
                write!(f, " at line {line}, character {column}")
            }
            Some(Position { column, .. }) => write!(f, " at character {column}"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for ConditionError {}

/// Host facts and environment values captured once per invocation for condition evaluation. See
/// [host facts](../docs/environment.md#host-facts-in-conditions).
pub(crate) struct HostNamespaces {
    facts: Value,
    env: Value,
}

impl HostNamespaces {
    /// Build both namespaces from `std::env::consts`, the host name, and the
    /// [`Environment`] captured at startup.
    pub fn capture(environment: &Environment) -> Self {
        let facts = Rc::new(facts());
        let entries = environment.entries();
        Self {
            facts: Namespace::value(FACTS, move |key| lookup(&facts, key)),
            env: Namespace::value(ENV, move |key| lookup(&entries, key)),
        }
    }
}

/// Capture the host OS, architecture, family, and hostname. Use the platform hostname without
/// truncating at a dot.
fn facts() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("os".to_owned(), std::env::consts::OS.to_owned()),
        ("arch".to_owned(), std::env::consts::ARCH.to_owned()),
        ("family".to_owned(), std::env::consts::FAMILY.to_owned()),
        (
            "hostname".to_owned(),
            gethostname::gethostname().to_string_lossy().into_owned(),
        ),
    ])
}

/// Variable scope and captured host namespaces used to evaluate conditions.
pub(crate) struct Bindings<'a> {
    scope: Rc<VarSet>,
    /// The captured `facts` and `env` namespaces.
    host: &'a HostNamespaces,
    /// The `vars` namespace, backed by the same scope as bare-identifier lookup.
    vars_namespace: Value,
}

impl<'a> Bindings<'a> {
    /// Bind variables and captured host namespaces for evaluation.
    /// Bare identifiers and the `vars` namespace share the same variable set.
    pub fn new(scope: &Rc<VarSet>, host: &'a HostNamespaces) -> Self {
        let vars_namespace = Rc::clone(scope);
        Self {
            scope: Rc::clone(scope),
            host,
            vars_namespace: Namespace::value(VARS, move |key| {
                vars_namespace.get(key).unwrap_or_default().to_owned()
            }),
        }
    }
}

/// Resolve reserved namespaces and declared variables; return `None` for undeclared names.
impl VariableResolver for Bindings<'_> {
    fn resolve(&self, name: &str) -> Option<Value> {
        match name {
            FACTS => Some(self.host.facts.clone()),
            ENV => Some(self.host.env.clone()),
            VARS => Some(self.vars_namespace.clone()),
            // `None` becomes the evaluator's `ResolveFailed`, reported as
            // [`EvalError::Undeclared`].
            _ => self.scope.get(name).map(|value| string(value.to_owned())),
        }
    }
}

/// A string-valued namespace supporting member and index lookup. Missing keys return empty
/// strings. The lookup closure must have a `'static` lifetime.
struct Namespace {
    /// The namespace's own name, which is what a type error reports.
    name: &'static str,
    lookup: Box<dyn Fn(&str) -> String>,
}

impl Namespace {
    fn value(name: &'static str, lookup: impl Fn(&str) -> String + 'static) -> Value {
        Value::Object(Rc::new(Self {
            name,
            lookup: Box::new(lookup),
        }))
    }
}

impl Object for Namespace {
    fn type_name(&self) -> &'static str {
        self.name
    }

    /// Look up a member, returning an empty string if it is absent.
    fn get_member(&self, name: &str) -> ExpressionResult<Value> {
        Ok(string((self.lookup)(name)))
    }

    fn get_key_value(&self, key: &str) -> ExpressionResult<Value> {
        Ok(string((self.lookup)(key)))
    }

    // Keep the default `as_bool`: namespace objects must fail boolean conversion.

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// One entry of a captured map, or the empty string.
fn lookup(entries: &BTreeMap<String, String>, key: &str) -> String {
    entries.get(key).cloned().unwrap_or_default()
}

fn string(text: String) -> Value {
    Value::Primitive(Primitive::Str(text))
}

/// Boolean conversion for condition results and logical operands. Accept booleans, numbers
/// (zero is false), and the strings `true`, `1`, `yes`, `on`, `false`, `0`, `no`, `off`, and
/// empty. Reject other values without disclosing them. Arithmetic and comparisons use the
/// evaluator's standard rules.
struct BatfilesCoercions;

/// Shared boolean-conversion policy.
static COERCIONS: BatfilesCoercions = BatfilesCoercions;

impl Coercions for BatfilesCoercions {
    fn to_bool(&self, value: &Value) -> ExpressionResult<bool> {
        match value {
            Value::Primitive(Primitive::Bool(boolean)) => Ok(*boolean),
            Value::Primitive(Primitive::Int(int)) => Ok(*int != 0),
            Value::Primitive(Primitive::Float(float)) => Ok(*float != 0.0),
            Value::Primitive(Primitive::Str(text)) => match text.as_str() {
                "true" | "1" | "yes" | "on" => Ok(true),
                "false" | "0" | "no" | "off" | "" => Ok(false),
                _ => Err(ExpressionError::EvaluationFailed(NOT_BOOLEAN.to_owned())),
            },
            Value::Object(_) => Err(ExpressionError::EvaluationFailed(NOT_BOOLEAN.to_owned())),
        }
    }

    /// Use the evaluator's standard numeric conversions.
    fn to_number(&self, value: &Value) -> ExpressionResult<Number> {
        STANDARD.to_number(value)
    }
}

/// Diagnostic for unsupported boolean values. Never include the value, which may contain
/// secrets.
const NOT_BOOLEAN: &str = "a value in it is not a boolean. Write a comparison, \
     such as `profile == 'personal'`, or use one of true, false, 1, 0, yes, no, on, or off";

/// Evaluate one condition against one set of bindings. [`Gate::admits`] applies
/// `when` or `unless` to the result.
fn eval(condition: &Condition, bindings: &Bindings<'_>) -> Result<bool, EvalError> {
    let evaluator = Evaluator::new_with_coercions(bindings, &COERCIONS);
    let value = evaluator
        .evaluate(&condition.expr)
        .map_err(|error| EvalError::from_expression(&error))?;

    // The same policy the evaluator applies inside `!`, `&&`, and `||`.
    COERCIONS.to_bool(&value).map_err(|_| EvalError::NotBoolean)
}

/// Condition evaluation failures. Messages omit the condition text;
/// [`Gate::evaluation_failure_reason`] adds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EvalError {
    /// A bare identifier that no layer declares.
    Undeclared { name: String },
    /// The condition's own value is outside the truthiness table.
    NotBoolean,
    /// Any other evaluator error, such as a divide by zero, an index out of
    /// bounds, or a truthiness failure inside an operator, with its message.
    Failed { message: String },
}

impl EvalError {
    fn from_expression(error: &ExpressionError) -> Self {
        match error {
            ExpressionError::ResolveFailed(name) => Self::Undeclared { name: name.clone() },
            // Use the message without the evaluator's "evaluation failed:" prefix.
            ExpressionError::EvaluationFailed(message) => Self::Failed {
                message: message.clone(),
            },
            other => Self::Failed {
                message: other.to_string(),
            },
        }
    }
}

/// Format the failure reason without the condition or field name.
impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Identifier syntax excludes characters that would need diagnostic escaping.
            Self::Undeclared { name } => write!(
                f,
                "`{name}` is not declared. Add `{name} = \"false\"` to [vars] in batfiles.toml, \
                 run `batfiles vars set {name} <value>`, or write `vars.{name}` if the variable \
                 is meant to be optional"
            ),
            Self::NotBoolean => f.write_str(NOT_BOOLEAN),
            Self::Failed { message } => f.write_str(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::var::VarName;

    /// A single-layer variable set.
    fn vars(bindings: &[(&str, &str)]) -> Rc<VarSet> {
        let manifest = bindings
            .iter()
            .map(|(name, value)| {
                (
                    VarName::try_from((*name).to_owned()).expect("valid name"),
                    crate::var_set::VarValue::Static((*value).to_owned()),
                )
            })
            .collect();
        Rc::new(VarSet::stack(
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

        let error = toml::from_str::<Record>("when = \"work &&\"\n")
            .expect_err("a malformed condition should fail the document");
        assert!(
            error.to_string().contains("is not a valid condition"),
            "{error}"
        );
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
                    crate::var_set::VarValue::Static(value.to_owned()),
                )
            })
            .collect();
        let command_line = [(
            VarName::try_from("editor".to_owned()).expect("valid name"),
            "emacs".to_owned(),
        )];
        let vars = Rc::new(VarSet::stack(
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
}
