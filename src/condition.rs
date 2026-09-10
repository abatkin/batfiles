//! What a `when` or `unless` condition means, and what counts as true.
//!
//! A [`Condition`] is parsed when the document declaring it is read, so nothing
//! here ever sees malformed text. What this module owns is the binding — a bare
//! identifier is a user variable, `vars` is the same variables read totally, and
//! `facts` and `env` are the two reserved namespaces — the truthiness table
//! every boolean context is read through, and the [`Gate`] a record's condition
//! makes of it. All three are specified in
//! [`docs/repoformat.md`](../docs/repoformat.md#conditions).
//!
//! Evaluation is pure: no filesystem, no clock, no subprocess. The one part of
//! it that reads the host is [`HostNamespaces::capture`], which a run performs
//! once.

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
use crate::output::quoted_value;
use crate::var_set::VarSet;

/// The reserved namespace of host facts.
const FACTS: &str = "facts";
/// The reserved namespace of host environment variables.
const ENV: &str = "env";
/// The reserved namespace of user variables, read totally.
const VARS: &str = "vars";

/// A condition, parsed, and the text it was written as.
///
/// The parse happens while the declaring document is read, so `when = "work &&"`
/// is a load error naming the file and line rather than a surprise partway
/// through a run. Both halves are kept: the text is the condition's identity,
/// since batfiles never rewrites a manifest and whitespace, parentheses, and
/// quote style are not recoverable from a parse tree.
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
/// every record that will carry an `Option<Condition>` derives [`Debug`].
impl fmt::Debug for Condition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Condition").field(&self.source).finish()
    }
}

/// The condition one record carries, and which way it decides.
///
/// A record writes `when`, or `unless`, or neither, and writing both is refused
/// where the record is read — which is what makes this two variants rather than
/// two fields. `when` admits the record when its condition is true and `unless`
/// admits it when the condition is false.
///
/// The two are not one rule and its negation, and the difference matters
/// wherever a gate has to be closed for a reason other than its own verdict: a
/// false `unless` *opens* a gate, so a record whose condition cannot be
/// evaluated is one batfiles has no verdict for in either spelling.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Gate<'a> {
    When(&'a Condition),
    Unless(&'a Condition),
}

impl<'a> Gate<'a> {
    /// The gate a record declares, or `None` where it declares neither.
    ///
    /// A record declaring both is refused as its document is read, so
    /// preferring `when` here decides nothing: it is only what a record that
    /// cannot exist would have meant.
    pub fn declared(when: Option<&'a Condition>, unless: Option<&'a Condition>) -> Option<Self> {
        when.map(Self::When).or_else(|| unless.map(Self::Unless))
    }

    /// Whether this run's bindings admit the record the gate is written on.
    pub fn admits(self, bindings: &Bindings<'_>) -> Result<bool, EvalError> {
        match self {
            Self::When(condition) => eval(condition, bindings),
            Self::Unless(condition) => Ok(!eval(condition, bindings)?),
        }
    }

    /// The line a gate this run cannot decide produces: the spelling, the
    /// condition as written, what the run is not doing about it, and the fault
    /// itself, which is the half that teaches the fix.
    ///
    /// The spelling is named for the reason [`Display`](fmt::Display) names it,
    /// and here it matters more: a reader who knows `unless` closed the gate
    /// knows batfiles did not read the failure as false and install the record.
    ///
    /// `consequence` is the caller's because only the record knows what it was
    /// going to do, and some lines have said it before they reach this: an
    /// entry of a clone list opens with `not cloning`, so it passes `None`.
    pub fn unevaluable(self, consequence: Option<&str>, error: &EvalError) -> String {
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

/// Why the gate is closed, which is the only state a report ever names: a
/// record the gate admits is reported by what it did.
///
/// The spelling the record used is named rather than the verdict alone, because
/// `unless` is the one a reader gets backwards, and the condition goes through
/// [`quoted_value`] like every other piece of repository text batfiles repeats.
impl fmt::Display for Gate<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let verdict = match self {
            Self::When(_) => "false",
            Self::Unless(_) => "true",
        };
        write!(
            f,
            "{} {} is {verdict}",
            self.spelling(),
            quoted_value(self.condition().source())
        )
    }
}

/// Why a record is being passed over, and how loudly to say so.
///
/// The two are reported the same way — where the record is named, rather than
/// where the reason was settled — and differ only in what they cost the reader.
/// Every reason but one is the run doing as it was asked, and belongs with the
/// rest of what `-v` reports; a condition batfiles cannot decide is nothing
/// anyone asked for, so it is printed at every verbosity and nothing is
/// silently ignored.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Skip {
    /// A disable, a run-only skip, or a gate this machine closes.
    AsAsked(String),
    /// A condition this machine cannot decide, which closes the gate in either
    /// spelling. Rendered by [`Gate::unevaluable`].
    Unevaluable(String),
}

impl Skip {
    /// The reason, for a caller composing the line that names the record.
    pub fn reason(&self) -> &str {
        match self {
            Self::AsAsked(reason) | Self::Unevaluable(reason) => reason,
        }
    }
}

/// Why a candidate condition is not one.
///
/// The message is rendered inside the TOML error that already names the file,
/// line, and column, so it stays one line and the parser's own caret diagram is
/// left unused: a second one would repeat the text and point at something else.
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
        // A condition is repository text, and a TOML multi-line string is a
        // legal place to write one, so the candidate goes through the same
        // escaping every other untrusted value does: a raw newline here would
        // add source lines inside TOML's own report.
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
/// [`docs/future/environment.md`](../docs/future/environment.md#host-facts-in-conditions)
/// specifies.
///
/// These are the same for every condition in one invocation, which is what
/// separates them from the `vars` namespace [`Bindings`] builds: that one
/// varies with the variable set, and will vary per inclusion at 7.5.
///
/// Captured once because [`facts`] is not as cheap as it looks — three of the
/// four are compile-time constants, but the host name is a syscall — and
/// because a namespace is an owned object, so rebuilding one per condition
/// would rebuild what it reads too.
pub(crate) struct HostNamespaces {
    facts: Value,
    env: Value,
}

impl HostNamespaces {
    /// Build both namespaces, reading the host once: `std::env::consts`, the
    /// host name, and the already captured environment.
    ///
    /// The [`Environment`] arrives as a parameter rather than being captured
    /// here because batfiles reads the process environment exactly once, at
    /// startup. The facts have no such snapshot to reuse, so they are read here.
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
/// Enumerating the set is the point of defining it here: a key batfiles does not
/// define resolves to the empty string, so `facts.arhc == 'arm64'` is silently
/// false, and an enumerated set is what a spelling can be checked against.
/// Adding a key later stays a non-breaking change.
///
/// The host name is whatever the platform reports, and batfiles never truncates
/// it at the first dot. What the platform reports differs: on Unix it is
/// `uname`'s nodename, fully qualified where the host is configured that way,
/// while on Windows it is `GetComputerNameExW(ComputerNamePhysicalDnsHostname)`,
/// which is the host component without the DNS suffix. A domain-joined Windows
/// machine therefore reports `silver` where the same machine's Unix counterpart
/// would report `silver.example.net`. The qualified Windows name needs a second
/// API and is
/// [an enhancement](../rewrite/steps.md#enhancements) rather than a flag on this
/// call; `docs/future/environment.md` documents the difference for users.
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
    /// Kept as `host` rather than `namespaces`, because `total` below is a
    /// namespace too and only these two come from the host.
    host: &'a HostNamespaces,
    /// The `vars` namespace, built once because it is handed out by value.
    total: Value,
}

impl<'a> Bindings<'a> {
    /// Bind `vars` for evaluation alongside the namespaces `host` captured.
    ///
    /// The variable set is shared rather than borrowed because a namespace owns
    /// what it reads. Sharing it, rather than copying the variables into the
    /// namespace, is what keeps one answer to what a name is worth: both
    /// spellings walk the same layers in the same precedence order.
    pub fn new(vars: &Rc<VarSet>, host: &'a HostNamespaces) -> Self {
        let total = Rc::clone(vars);
        Self {
            vars: Rc::clone(vars),
            host,
            total: Namespace::value(VARS, move |key| {
                total.get(key).unwrap_or_default().to_owned()
            }),
        }
    }
}

/// The four-arm dispatch, which needs no precedence check: `facts`, `env`, and
/// `vars` are all reserved by [`VarName`](crate::var::VarName), so no user
/// variable can be named any of them and the fourth arm cannot shadow the first
/// three.
///
/// `true` and `false` never reach here at all — the grammar takes them as
/// literals before a variable is looked up.
impl VariableResolver for Bindings<'_> {
    fn resolve(&self, name: &str) -> Option<Value> {
        match name {
            FACTS => Some(self.host.facts.clone()),
            ENV => Some(self.host.env.clone()),
            VARS => Some(self.total.clone()),
            // `None` is what the evaluator turns into `ResolveFailed`, which is
            // the undeclared-identifier error [`EvalError`] then names.
            _ => self.vars.get(name).map(|value| string(value.to_owned())),
        }
    }
}

/// A string-valued namespace whose lookups are total, in member and index
/// syntax alike.
///
/// One type serves all three namespaces, so `facts.os`, `vars.work`, and
/// `env["XDG_CURRENT_DESKTOP"]` reach the same rule. The crate's own dict
/// cannot: it answers an unknown key with `NoSuchKey` and an unknown member
/// with `UnknownMember`, which contradicts the empty-string rule outright.
///
/// The lookup is a closure rather than a map because [`Object`] requires [`Any`]
/// and therefore `'static`, so a namespace cannot borrow what it reads. Each of
/// the three captures a share of its source instead of a copy, and nothing needs
/// the keys enumerated: the language has no way to ask for them.
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
/// | anything else | an error naming the value |
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
/// One constant and two callers: the policy is applied from inside the evaluator
/// for `&&`, `||`, and `!`, and by [`eval`] to the finished value. Sharing the
/// text is what keeps which path fired invisible to the user.
///
/// **The offending value is not named, and the example is a fixed one.** A
/// condition is the one place a value reaches a diagnostic without having been
/// asked for: `when = "env.GITHUB_TOKEN"` puts a credential outside the table,
/// and a manifest batfiles evaluates is not always the user's own. This is the
/// rule [`env_vars`](crate::env_vars) already states for the environment --
/// report the name a user acts on, not the value -- applied where a value would
/// otherwise be echoed twice. The condition's own text still names what failed.
const NOT_BOOLEAN: &str = "a value in it is not a boolean. Write a comparison, \
     such as `profile == 'personal'`, or use one of true, false, 1, 0, yes, no, on, or off";

/// Evaluate one condition against one set of bindings.
///
/// Reached through [`Gate::admits`], which is where a record's `when` or
/// `unless` decides what a bare `true` means for it.
fn eval(condition: &Condition, bindings: &Bindings<'_>) -> Result<bool, EvalError> {
    let evaluator = Evaluator::new_with_coercions(bindings, &COERCIONS);
    let value = evaluator
        .evaluate(&condition.expr)
        .map_err(|error| EvalError::from_expression(&error))?;

    // The finished value goes through the same policy the evaluator applied
    // inside `!`, `&&`, and `||`, so a whole condition and a subexpression of
    // one cannot disagree.
    COERCIONS.to_bool(&value).map_err(|_| EvalError::NotBoolean)
}

/// Why a condition could not be evaluated.
///
/// No variant names the condition it is about: this is read long after the
/// manifest was parsed, so the text a reader needs is the whole line
/// [`Gate::unevaluable`] builds, which has the condition and the spelling that
/// carried it. What is here is the fault clause of that line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EvalError {
    /// A bare identifier that no layer declares.
    Undeclared { name: String },
    /// The condition's own value is outside the truthiness table.
    NotBoolean,
    /// Everything else the evaluator can produce — a divide by zero, an index
    /// out of bounds, a member on a value that has none, a truthiness failure
    /// inside an operator — flattened, because the crate's error is
    /// `#[non_exhaustive]` and batfiles has nothing to add to `index out of
    /// bounds: 5 (len: 3)`.
    Failed { message: String },
}

impl EvalError {
    fn from_expression(error: &ExpressionError) -> Self {
        match error {
            ExpressionError::ResolveFailed(name) => Self::Undeclared { name: name.clone() },
            // The inner string rather than the `Display`, which would prefix
            // "evaluation failed: ". This is the arm a truthiness failure inside
            // `&&`, `||`, or `!` arrives through, and its message is already
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
            // Both messages do the teaching: these are the two errors a
            // well-formed manifest can still hit. An identifier needs no
            // escaping of its own -- the grammar admits only letters, digits,
            // and underscores -- so it is written as read.
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

    /// A variable set built from one layer, since this module cares about what
    /// a name is bound to and not about which layer bound it.
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
    /// The fault clause names no condition on its own, so anything about how a
    /// condition is repeated back is asserted here.
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
        // The contrast is the whole of it: the two spellings can never disagree
        // about a variable that *is* declared.
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

        // And it composes, which is what makes it usable rather than a curiosity.
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
        // Two distinct channels over one environment variable:
        // `BATFILES_VAR_FOO` defines the user variable `FOO` -- which is
        // `env_vars`' job, not this module's -- and stays readable under its own
        // name here, while a bare `FOO` is only ever an `env` entry.
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
        // Why this step wants the language's own `Coercions` rather than a
        // check on the finished value: every line here is an operand.
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
        // A condition is the one place a value reaches a diagnostic without
        // having been asked for, and a manifest batfiles evaluates is not
        // always the user's own. Both paths are checked, since the operand one
        // renders through the expression crate.
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
