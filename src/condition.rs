//! Parse and evaluate `when`/`unless` conditions with Batfiles' truthiness rules.
//!
//! Parse conditions when their document is read. Bare identifiers require a
//! declared variable; `vars`, `facts`, and `env` return empty strings for missing
//! keys. See [`docs/repoformat.md`](../docs/repoformat.md#conditions).
//!
//! Evaluation is pure: no filesystem, clock, or subprocess access. Capture host
//! inputs once per run with [`HostNamespaces::capture`].

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
/// The reserved namespace of user variables, read totally.
const VARS: &str = "vars";

/// A parsed condition with its original text retained for diagnostics.
#[derive(Clone, Deserialize)]
#[serde(try_from = "String")]
pub(crate) struct Condition {
    source: String,
    expr: Expr,
}

impl Condition {
    /// Parse `source` as a condition, or report why it is not one.
    ///
    /// Conditions in a TOML document arrive through [`TryFrom`] instead, so
    /// that an invalid one fails the document that holds it.
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

/// The source text. A derived implementation would print the whole tree, and
/// records carrying an `Option<Condition>` derive [`Debug`].
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
    pub fn declared(when: Option<&'a Condition>, unless: Option<&'a Condition>) -> Option<Self> {
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
            Ok(false) => Some(Exclusion::Expected(self.exclusion_reason())),
            Err(error) => Some(Exclusion::EvaluationFailed(
                self.unevaluable(consequence, &error),
            )),
        }
    }

    /// Whether this run's bindings admit the record the gate is written on.
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
            self.spelling(),
            quoted_value(self.condition().source())
        )
    }

    /// Format an evaluation failure with the condition and optional consequence.
    fn unevaluable(self, consequence: Option<&str>, error: &EvalError) -> String {
        let condition = quoted_value(self.condition().source());
        let consequence = consequence.map_or_else(String::new, |what| format!(", so {what}"));
        format!(
            "{} {condition} cannot be evaluated{consequence}: {error}",
            self.spelling()
        )
    }

    /// The field the record wrote.
    fn spelling(self) -> &'static str {
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
    /// A disable, a run-only skip, or a gate this machine closes: all three are
    /// the run doing as it was asked.
    Expected(String),
    /// A condition this machine cannot decide, which closes the gate in either
    /// spelling. Rendered by [`Gate::unevaluable`].
    EvaluationFailed(String),
}

impl Exclusion {
    /// Report caller-supplied wording at `-v` for expected exclusions, or as a
    /// warning at every verbosity for evaluation failures.
    pub fn report(&self, reporter: &Reporter, message: &str) {
        match self {
            Self::Expected(_) => reporter.detail(1, message),
            Self::EvaluationFailed(_) => reporter.warn(message),
        }
    }

    /// Report an action or remote exclusion with its heading and reason, at the
    /// severity [`Self::report`] gives it.
    pub fn report_heading(&self, reporter: &Reporter, heading: &str) {
        let reason = self.reason();
        let message = match self {
            Self::Expected(_) => format!("{heading} - skipped: {reason}"),
            Self::EvaluationFailed(_) => format!("{heading}: {reason}"),
        };
        self.report(reporter, &message);
    }

    /// The reason, for a caller composing the line that names the record.
    pub fn reason(&self) -> &str {
        match self {
            Self::Expected(reason) | Self::EvaluationFailed(reason) => reason,
        }
    }
}

/// Why a candidate condition is not one.
///
/// Rendered inside TOML's error, which already names the file, line, and
/// column, so the message is one line without the parser's caret diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConditionError {
    candidate: String,
    /// Absent when the parser failed without a position, which only its
    /// internal error does.
    position: Option<Position>,
    message: String,
}

/// Where within the condition the parser stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Position {
    /// 1-based, and 1 for every condition written as an ordinary TOML string.
    line: usize,
    /// 1-based, in characters.
    column: usize,
}

impl ConditionError {
    fn new(candidate: &str, error: &ExpressionError) -> Self {
        // `Error` is `#[non_exhaustive]`, so the wildcard is required; every
        // arm but a parse failure loses only the position, not the message.
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
        // Repository text, possibly from a multi-line string: escaped so a
        // newline cannot add lines to TOML's report.
        write!(
            f,
            "{} is not a valid condition: {}",
            quoted_value(&self.candidate),
            self.message
        )?;
        match self.position {
            // The line is worth naming only when the candidate has more than
            // one; otherwise it is always 1, beside TOML's own line number.
            Some(Position { line, column }) if line > 1 => {
                write!(f, " at line {line}, character {column}")
            }
            Some(Position { column, .. }) => write!(f, " at character {column}"),
            None => Ok(()),
        }
    }
}

impl std::error::Error for ConditionError {}

/// The two namespaces sourced from outside the repository, prepared for
/// evaluation: the host facts and the host environment
/// [`docs/environment.md`](../docs/environment.md#host-facts-in-conditions)
/// specifies.
///
/// Unlike the `vars` namespace [`Bindings`] builds, these are the same for every
/// condition in an invocation, so they are captured once.
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

/// The four facts, and the whole of what `facts` contains.
///
/// An unknown key reads as the empty string, so `facts.arhc == 'arm64'` is
/// silently false; the enumerated set is what a spelling can be checked
/// against.
///
/// The host name is the platform's, never truncated at a dot, so a
/// domain-joined Windows machine reports a shorter name than Unix would. [The
/// environment reference](../docs/environment.md#host-facts-in-conditions)
/// specifies this; the qualified Windows name is
/// [an enhancement](../docs/future/roadmap.md#enhancements).
fn facts() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("os".to_owned(), std::env::consts::OS.to_owned()),
        ("arch".to_owned(), std::env::consts::ARCH.to_owned()),
        ("family".to_owned(), std::env::consts::FAMILY.to_owned()),
        (
            "hostname".to_owned(),
            // Infallible by signature, and decoded the way the environment is.
            gethostname::gethostname().to_string_lossy().into_owned(),
        ),
    ])
}

/// One variable set, prepared for evaluation: the bare-identifier lookup and
/// the `vars` namespace over the same variables, plus the invocation's
/// [`HostNamespaces`].
pub(crate) struct Bindings<'a> {
    vars: Rc<VarSet>,
    /// The captured `facts` and `env` namespaces.
    host: &'a HostNamespaces,
    /// The `vars` namespace over the same variables the bare-identifier lookup
    /// reads, built once because it is handed out by value.
    vars_namespace: Value,
}

impl<'a> Bindings<'a> {
    /// Bind variables and captured host namespaces for evaluation.
    /// Bare identifiers and the `vars` namespace share the same variable set.
    pub fn new(vars: &Rc<VarSet>, host: &'a HostNamespaces) -> Self {
        let vars_namespace = Rc::clone(vars);
        Self {
            vars: Rc::clone(vars),
            host,
            vars_namespace: Namespace::value(VARS, move |key| {
                vars_namespace.get(key).unwrap_or_default().to_owned()
            }),
        }
    }
}

/// No precedence check is needed: `facts`, `env`, and `vars` are reserved by
/// [`VarName`](crate::var::VarName), so no variable shadows them. `true` and
/// `false` are grammar literals and never reach here.
impl VariableResolver for Bindings<'_> {
    fn resolve(&self, name: &str) -> Option<Value> {
        match name {
            FACTS => Some(self.host.facts.clone()),
            ENV => Some(self.host.env.clone()),
            VARS => Some(self.vars_namespace.clone()),
            // `None` becomes the evaluator's `ResolveFailed`, reported as
            // [`EvalError::Undeclared`].
            _ => self.vars.get(name).map(|value| string(value.to_owned())),
        }
    }
}

/// A string-valued namespace supporting member and index lookup.
/// Lookups return empty strings for missing keys. The owned lookup closure
/// must be static to satisfy the expression evaluator's [`Object`] contract.
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

    /// Both spellings, both total. Member syntax always works for a `vars` key,
    /// because every variable name is identifier-compatible by construction;
    /// `env` is the namespace where indexing is sometimes required.
    fn get_member(&self, name: &str) -> ExpressionResult<Value> {
        Ok(string((self.lookup)(name)))
    }

    fn get_key_value(&self, key: &str) -> ExpressionResult<Value> {
        Ok(string((self.lookup)(key)))
    }

    // `as_bool` keeps its `None` default deliberately: a namespace is not a
    // boolean, so `when = "facts"` fails the truthiness table rather than
    // quietly reading as true.

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

/// Batfiles' truthiness, over a closed set of spellings.
///
/// | Value | Reads as |
/// | --- | --- |
/// | a real boolean | itself |
/// | a number | `false` at zero, `true` otherwise |
/// | `"true"`, `"1"`, `"yes"`, `"on"` | `true` |
/// | `"false"`, `"0"`, `"no"`, `"off"`, `""` | `false` |
/// | anything else | an error that does not disclose the value |
///
/// Supplied as a [`Coercions`] policy so that it applies to a condition's result
/// and to every `&&`, `||`, and `!` operand alike. Comparison and `+` are
/// unaffected: those keep the language's own rules, so `==` behaves as it
/// defines it.
struct BatfilesCoercions;

/// A unit struct behind a `static`, mirroring the crate's own [`STANDARD`], so
/// there is no lifetime to thread into [`Evaluator::new_with_coercions`].
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
            // A namespace, a list, a dict, or a function. None of them is a
            // decision either.
            Value::Object(_) => Err(ExpressionError::EvaluationFailed(NOT_BOOLEAN.to_owned())),
        }
    }

    /// Delegated: batfiles has no opinion about arithmetic.
    fn to_number(&self, value: &Value) -> ExpressionResult<Number> {
        STANDARD.to_number(value)
    }
}

/// The one sentence a value outside the table produces.
///
/// Shared by the evaluator's `&&`, `||`, and `!` and by [`eval`]'s check of the
/// finished value, so both paths read the same.
///
/// **The offending value is never named.** `when = "env.GITHUB_TOKEN"` would
/// otherwise print a credential, and a manifest batfiles evaluates is not
/// always the user's own. The condition's text still names what failed.
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

/// Why a condition could not be evaluated.
///
/// Each variant is the fault clause only; [`Gate::unevaluable`] adds the
/// condition and the field that carried it.
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
            // The inner string, without `Display`'s "evaluation failed: "
            // prefix. Operator truthiness failures arrive here as
            // [`NOT_BOOLEAN`].
            ExpressionError::EvaluationFailed(message) => Self::Failed {
                message: message.clone(),
            },
            other => Self::Failed {
                message: other.to_string(),
            },
        }
    }
}

/// The fault clause alone, which [`Gate::unevaluable`] writes after the
/// condition it belongs to. Nothing renders one without that frame.
impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Both messages explain the fix: a valid manifest can still hit
            // them. Identifiers need no escaping; the grammar admits only
            // letters, digits, and underscores.
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
                    (*value).to_owned(),
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
        Gate::When(&parsed).unevaluable(Some(CONSEQUENCE), &failure(source, bindings))
    }

    /// Stands in for a caller's clause; the wording belongs to the caller.
    const CONSEQUENCE: &str = "it is not installed";

    // Parsing, which happens where a document is read.

    #[test]
    fn a_condition_keeps_the_text_it_was_written_as() {
        // Not a canonical form: batfiles never rewrites a manifest, so the
        // condition's identity is what the user typed.
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
        // A TOML multi-line string is a legal place to write a condition, so a
        // candidate can hold a real newline; it is reported inside TOML's own
        // caret report, which a second line would corrupt.
        let error = Condition::new("work &&\n'oops").expect_err("an unterminated string");
        let message = error.to_string();
        assert!(!message.contains('\n'), "{message}");
        assert!(message.contains("\\n"), "{message}");
        // Two lines, so the line number earns its place.
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

    // Binding: what an identifier means.

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
        // Asymmetric with `facts` and `env` on purpose: a namespace is
        // extensible, so an unknown key is forward compatibility, while a
        // variable name is not, so a name nothing declares is a typo.
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
        // The distinction `VarSet::get` keeps, read from the other end.
        let vars = vars(&[("empty", "")]);
        let host = host(&[]);
        let bindings = Bindings::new(&vars, &host);

        assert!(!truth("empty", &bindings));
        assert!(truth("empty == ''", &bindings));
    }

    #[test]
    fn vars_is_total_where_the_bare_identifier_is_strict() {
        // The two spellings agree about a declared variable.
        let vars = vars(&[("valued", "yes")]);
        let host = host(&[]);
        let bindings = Bindings::new(&vars, &host);

        assert!(truth("vars.valued", &bindings));
        assert!(truth("vars.valued == 'yes'", &bindings));

        // The case the namespace exists for.
        assert!(!truth("vars.undeclared", &bindings));
        assert!(truth("vars.undeclared == ''", &bindings));
        assert!(matches!(
            failure("undeclared", &bindings),
            EvalError::Undeclared { .. }
        ));

        // And it composes.
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
        // Both spellings answer from the same walk down the precedence order,
        // so an override is visible through either.
        let manifest = [("editor", "vi")]
            .into_iter()
            .map(|(name, value)| {
                (
                    VarName::try_from(name.to_owned()).expect("valid name"),
                    value.to_owned(),
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
        // The footgun the enumerated set exists to mitigate, pinned as
        // behavior: a typo in a fact name is false, not a failure.
        assert!(!truth("facts.arhc == 'arm64'", &bindings));
        assert!(truth("facts.arhc == ''", &bindings));
    }

    #[test]
    fn the_captured_facts_describe_the_host_running_the_test() {
        // Asserted against `std::env::consts` rather than a hard-coded
        // platform, so this passes on every runner.
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
        // Passed through, never truncated at a dot, so a fully qualified name
        // stays fully qualified. What the platform reports is the platform's
        // business, and differs on Windows -- see `facts`.
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
        // `BATFILES_VAR_FOO` defines the variable `FOO` (through `env_vars`)
        // and stays readable under its own name in `env`; `env.FOO` is empty.
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
        // This is the specification, so it is written as one.
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
        // `profile` is caught by `eval` on the finished value; `profile && work`
        // is raised inside the evaluator. A hand-edit to either message breaks
        // this.
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
        // Both paths: the operand one renders through the expression crate.
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
        // A condition that parses can still hold a newline or a control
        // character, and the message is written where a second line would read
        // as a diagnostic batfiles wrote.
        let vars = vars(&[]);
        let host = host(&[]);
        let bindings = Bindings::new(&vars, &host);

        // A newline is whitespace to the grammar, so this parses and fails at
        // evaluation, which is the path the parse-time escaping never covers.
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

    // What evaluation does with the rest of what can go wrong.

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
        // The verdict is the caller's: the gate itself holds only what the
        // record declared, and each spelling closes on the opposite value.
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
        // The line is built rather than the fault alone: `unless` is the
        // spelling a reader gets backwards, and a failure read as false would
        // open the gate instead of closing it.
        let vars = vars(&[]);
        let host = host(&[]);
        let bindings = Bindings::new(&vars, &host);
        let parsed = condition("nowhere");
        let error = failure("nowhere", &bindings);

        assert!(
            Gate::When(&parsed)
                .unevaluable(Some(CONSEQUENCE), &error)
                .starts_with("when \"nowhere\" cannot be evaluated, so it is not installed: "),
        );
        assert!(
            Gate::Unless(&parsed)
                .unevaluable(Some(CONSEQUENCE), &error)
                .starts_with("unless \"nowhere\" cannot be evaluated, so it is not installed: "),
        );
        // A caller whose line has already said what is not happening.
        assert!(
            Gate::Unless(&parsed)
                .unevaluable(None, &error)
                .starts_with("unless \"nowhere\" cannot be evaluated: "),
        );
    }

    #[test]
    fn one_evaluation_decides_a_gate_three_ways() {
        // The map every record carrying a condition goes through, so that a
        // failure closes the gate for an action, a clone-list entry, and a
        // remote alike.
        let vars = vars(&[("work", "true")]);
        let host = host(&[]);
        let bindings = Bindings::new(&vars, &host);
        let work = condition("work");
        let nowhere = condition("nowhere");

        let verdict = |gate: Gate<'_>| gate.exclusion(&bindings, Some(CONSEQUENCE));

        assert!(verdict(Gate::When(&work)).is_none());
        let Some(Exclusion::Expected(reason)) = verdict(Gate::Unless(&work)) else {
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
        // How every language with `||` behaves: "undeclared is an error" is not
        // a promise that the error will always be reached.
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
        // A manifest is not always the user's own -- batfiles will evaluate a
        // third-party remote's -- so repository content must not be able to
        // abort the tool.
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
