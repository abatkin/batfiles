//! Parse and evaluate `when`/`unless` conditions using [Batfiles truthiness
//! rules](../../docs/repoformat.md#conditions). Bare identifiers require declared variables;
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
use crate::vars::scope::VarScope;

/// The reserved namespace of host facts.
const FACTS: &str = "facts";
/// The reserved namespace of host environment variables.
const ENV: &str = "env";
/// Reserved namespace for user variables; missing keys return empty strings.
const VARS: &str = "vars";

/// A condition, parsed as it is read. A condition that does not parse is kept with its error,
/// so that manifest validation can report it with the record that holds it.
#[derive(Clone, Deserialize)]
#[serde(from = "String")]
pub(crate) struct Condition {
    source: String,
    parsed: Result<Expr, SyntaxError>,
}

impl Condition {
    /// Parse a condition, returning an error for invalid syntax.
    pub fn new(source: &str) -> Result<Self, SyntaxError> {
        let condition = Self::from(source.to_owned());
        condition.syntax().map_err(SyntaxError::clone)?;
        Ok(condition)
    }

    /// The condition exactly as written, whitespace and quote style included.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Why the condition does not parse, if it does not.
    pub fn syntax(&self) -> Result<(), &SyntaxError> {
        self.parsed.as_ref().map(|_| ())
    }
}

impl From<String> for Condition {
    fn from(source: String) -> Self {
        let parsed = parse_expression(&source)
            .map_err(|error| SyntaxError::new("condition", &source, &error));
        Self { source, parsed }
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
    /// A permission the record needs and does not have.
    NotPermitted(String),
}

impl Exclusion {
    /// Report caller-supplied wording at `-v` for expected exclusions, or as a
    /// warning at every verbosity for evaluation failures and missing permissions.
    pub fn report(&self, reporter: &Reporter, message: &str) {
        match self {
            Self::Deliberate(_) => reporter.detail(1, message),
            Self::EvaluationFailed(_) | Self::NotPermitted(_) => reporter.warn(message),
        }
    }

    /// Report an action or remote exclusion with its heading and reason, at the
    /// severity [`Self::report`] gives it.
    pub fn report_heading(&self, reporter: &Reporter, heading: &str) {
        let reason = self.reason();
        let message = match self {
            Self::Deliberate(_) | Self::NotPermitted(_) => format!("{heading} - skipped: {reason}"),
            Self::EvaluationFailed(_) => format!("{heading}: {reason}"),
        };
        self.report(reporter, &message);
    }

    /// The reason, for a caller composing the line that names the record.
    pub fn reason(&self) -> &str {
        match self {
            Self::Deliberate(reason)
            | Self::EvaluationFailed(reason)
            | Self::NotPermitted(reason) => reason,
        }
    }
}

/// A condition or template parse error, formatted on one line without a caret diagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyntaxError {
    /// What the text was meant to be: `condition` or `template`.
    what: &'static str,
    candidate: String,
    /// Parser error position, if available.
    position: Option<Position>,
    message: String,
}

/// Where within a condition or template an error is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Position {
    /// One-based line number within the text.
    pub line: usize,
    /// 1-based, in characters.
    pub column: usize,
}

/// `line L, character C`, or `character C` on the first line.
impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self { line, column } if *line > 1 => write!(f, "line {line}, character {column}"),
            Self { column, .. } => write!(f, "character {column}"),
        }
    }
}

impl SyntaxError {
    /// The parse `error` in `candidate`, which was meant to be a `what`.
    pub(super) fn new(what: &'static str, candidate: &str, error: &ExpressionError) -> Self {
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
            what,
            candidate: candidate.to_owned(),
            position,
            message,
        }
    }
}

impl SyntaxError {
    /// The failure without the text it was found in: `is not a valid <what>: <message>`, with
    /// its position.
    pub(crate) fn problem(&self) -> String {
        let Self {
            what,
            message,
            position,
            ..
        } = self;
        match position {
            Some(position) => format!("is not a valid {what}: {message} at {position}"),
            None => format!("is not a valid {what}: {message}"),
        }
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Escape the text so embedded newlines cannot alter the enclosing TOML diagnostic.
        write!(f, "{} {}", quoted_value(&self.candidate), self.problem())
    }
}

impl std::error::Error for SyntaxError {}

/// Host facts and environment values captured once per invocation for condition evaluation. See
/// [host facts](../../docs/environment.md#host-facts).
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
    scope: Rc<VarScope>,
    /// The captured `facts` and `env` namespaces.
    host: &'a HostNamespaces,
    /// The `vars` namespace, backed by the same scope as bare-identifier lookup.
    vars_namespace: Value,
}

impl<'a> Bindings<'a> {
    /// Bind variables and captured host namespaces for evaluation.
    /// Bare identifiers and the `vars` namespace share the same variable scope.
    pub fn new(scope: &Rc<VarScope>, host: &'a HostNamespaces) -> Self {
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
pub(super) struct BatfilesCoercions;

/// Shared boolean-conversion policy.
pub(super) static COERCIONS: BatfilesCoercions = BatfilesCoercions;

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
    // Manifest validation refuses a condition that does not parse before any evaluation.
    let expr = condition
        .parsed
        .as_ref()
        .map_err(|error| EvalError::Failed {
            message: error.problem(),
        })?;
    let evaluator = Evaluator::new_with_coercions(bindings, &COERCIONS);
    let value = evaluator
        .evaluate(expr)
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
    /// bounds, or a truthiness failure inside an operator. The message names
    /// no value computed at run time.
    Failed { message: String },
}

impl EvalError {
    /// Classify an evaluator error. A key, index, or operand computed at run time may come
    /// from a variable or the host environment, so the message never carries one.
    pub(super) fn from_expression(error: &ExpressionError) -> Self {
        let message = match error {
            ExpressionError::ResolveFailed(name) => return Self::Undeclared { name: name.clone() },
            // Use the message without the evaluator's "evaluation failed:" prefix.
            // Batfiles and the evaluator build these from fixed text.
            ExpressionError::EvaluationFailed(message) => message.clone(),
            // These name only types, operators, and members written in the expression.
            ExpressionError::NotCallable
            | ExpressionError::TypeMismatch(_)
            | ExpressionError::NotCoercible { .. }
            | ExpressionError::DivideByZero
            | ExpressionError::IntegerOverflow { .. }
            | ExpressionError::UnknownMember { .. } => error.to_string(),
            ExpressionError::NoSuchKey(_) => NO_SUCH_KEY.to_owned(),
            ExpressionError::IndexOutOfBounds { .. } => INDEX_OUT_OF_RANGE.to_owned(),
            ExpressionError::NotIndexable(_) => NOT_INDEXABLE.to_owned(),
            // An error kind this list does not know may carry a value.
            _ => UNKNOWN_FAILURE.to_owned(),
        };
        Self::Failed { message }
    }
}

const NO_SUCH_KEY: &str = "a dictionary has no entry for the key looked up in it";
const INDEX_OUT_OF_RANGE: &str = "a list index is out of range";
const NOT_INDEXABLE: &str =
    "a value is indexed by something other than a string or integer, or cannot be indexed";
const UNKNOWN_FAILURE: &str = "the expression could not be evaluated";

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
mod tests;
