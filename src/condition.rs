//! Evaluating a `when`/`unless` condition: what its identifiers mean, and what
//! counts as true.
//!
//! [`Condition`] parsing happens where a manifest is read
//! (`crate::repo::value`), so this module never sees malformed text. What it
//! owns is the *binding*: a bare identifier is a user variable, `vars` is the
//! same variables read totally, and `facts` and `env` are the two reserved
//! namespaces `docs/repoformat.md` defines.
//!
//! **[`eval`] is pure.** No filesystem, no clock, no subprocess. Its inputs are
//! a parsed condition and bindings the caller prepared, which is the same
//! property that makes [`crate::scope`] testable. The one place here that
//! touches the host is [`Host::capture`], which runs once per invocation.
//!
//! Three rules a reader will otherwise have to reconstruct:
//!
//! - **A bare identifier has three cases, not two.** A declared variable with a
//!   value resolves to it; a declared variable that produced no value resolves
//!   to the empty string; an *undeclared* one is [`EvalError::Undeclared`].
//!   `docs/repoformat.md` deliberately makes a missing *fact* silent so the
//!   namespace can grow, but a variable name has no extensibility argument, and
//!   an `unless = "no_gui"` misspelt `no_gui_` would otherwise read as false on
//!   every machine forever — inverting a gate that installs things.
//! - **`vars` is the total counterpart**, for a variable that is legitimately
//!   optional: one set by `vars set` on some machines only, or one a third-party
//!   remote's action reads and cannot make the leaf declare. A missing key and a
//!   valueless variable are both `""` there, which reads as false everywhere.
//! - **Truthiness is batfiles'**, supplied as a [`Coercions`] policy so it
//!   applies to a condition's result and to every `&&`, `||`, and `!` operand
//!   alike. Comparison and `+` deliberately keep the language's own rules, so
//!   `==` is unaffected.
//!
//! Two consequences worth stating, because each looks like a bug:
//!
//! - **`||` and `&&` short-circuit**, so an undeclared identifier on the right
//!   of a satisfied `||` is never resolved and never reported. Undeclared
//!   identifiers are caught by *evaluation*, not by inspection, and whether one
//!   is caught depends on operand order. That is how every language with
//!   short-circuiting behaves; "undeclared is an error" is not a promise that it
//!   will always be reached.
//! - **A namespace lookup is total, including for names that look like
//!   methods.** `facts.os.toUpper()` works, because the member is on the
//!   resulting *string*; `vars.keys()` resolves to `""` and then fails as
//!   not-callable. That is the price of `docs/repoformat.md`'s empty-string
//!   rule, and it is the spec's price rather than this module's.
#![allow(
    dead_code,
    reason = "no command evaluates a condition until the reachability pipeline"
)]

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use simple_expressions::evaluator::{Evaluator, VariableResolver};
use simple_expressions::types::coerce::{Coercions, Number, STANDARD};
use simple_expressions::types::error::{Error as ExpressionError, Result as ExpressionResult};
use simple_expressions::types::object::Object;
use simple_expressions::types::primitive::Primitive;
use simple_expressions::types::value::Value;

use crate::config::Environment;
use crate::repo::Condition;
use crate::scope::Scope;

/// The reserved namespace of host facts.
const FACTS: &str = "facts";
/// The reserved namespace of host environment variables.
const ENV: &str = "env";
/// The reserved namespace of user variables, read totally.
const VARS: &str = "vars";

/// The namespaces that are identical in every scope of one invocation: `facts`
/// and `env`.
///
/// Built once, like [`Overlay`](crate::scope::Overlay), and for two reasons
/// harder than tidiness.
///
/// `Object: Any` makes an `Object` impl `'static`, so a namespace cannot borrow
/// its source and must own a map. Capturing per [`eval`] would clone the whole
/// process environment for every `when` in every manifest; captured once, each
/// resolve clones an [`Rc`].
///
/// And **[`facts`] is not as cheap as it looks** — see its own note. Three of
/// the four are compile-time constants, but `hostname` is a syscall, so a
/// `Host` per scope would pay it once per inclusion rather than once per run.
///
/// [`Bindings::new`] borrows a `Host` rather than taking one, so the shape the
/// type invites is one `Host` and many `Bindings`. That is the whole of the
/// enforcement: like [`Environment::capture`] and `Overlay::build`, capturing
/// once per invocation is the caller's obligation, not something the signature
/// can make impossible.
///
/// Named `Host` rather than `Context` because the expression crate has a
/// `coerce::Context` of its own, and because this is precisely the part of
/// evaluation that is not pure.
pub(crate) struct Host {
    facts: Value,
    env: Value,
}

impl Host {
    /// Read the host once: `std::env::consts`, `gethostname`, and the already
    /// captured environment.
    ///
    /// The [`Environment`] arrives as a parameter rather than being captured
    /// here because batfiles reads the process environment exactly once, at
    /// startup. The facts have no such snapshot to reuse, so they are read here.
    pub fn capture(environment: &Environment) -> Self {
        Self {
            facts: Namespace::value(FACTS, facts()),
            env: Namespace::value(ENV, environment.entries().clone()),
        }
    }
}

/// The four facts, and the whole of what `facts` contains.
///
/// Enumerating the set is the point of defining it here.
/// `docs/repoformat.md` makes a missing key the empty string, so `facts.arhc ==
/// 'arm64'` reads as `"" == 'arm64'` with no diagnostic; an undefined set is
/// therefore a footgun and an enumerated one is the mitigation. Adding a key
/// later stays a non-breaking change.
///
/// `hostname` is **verbatim** — whatever the host is configured with, fully
/// qualified or not. Truncating at the first dot would discard the domain, which
/// is what distinguishes work from home on some fleets, and would lose it just
/// as silently as the mismatch it was meant to fix. `docs/environment.md`
/// documents the cost; a `hostname_short` can follow as an addition.
///
/// **`os`, `arch`, and `family` are free and `hostname` is not**, which is the
/// asymmetry that makes this function look cheaper than it is: the first three
/// are `&'static str` constants baked in at compile time, while the fourth is a
/// `uname(2)` on Unix and *two* `GetComputerNameExW` calls on Windows — one to
/// size the buffer, one to fill it. Small either way, but it is why [`Host`] is
/// captured once per invocation rather than per scope.
fn facts() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("os".to_owned(), std::env::consts::OS.to_owned()),
        ("arch".to_owned(), std::env::consts::ARCH.to_owned()),
        ("family".to_owned(), std::env::consts::FAMILY.to_owned()),
        (
            "hostname".to_owned(),
            // Infallible by signature, and decoded the way
            // `Environment::capture` decodes the environment.
            gethostname::gethostname().to_string_lossy().into_owned(),
        ),
    ])
}

/// One scope, prepared for evaluation: the bare-identifier lookup and the `vars`
/// namespace over the same bindings, plus the invocation's [`Host`].
///
/// Built once per scope for the reason [`Host`] is built once per invocation —
/// the `vars` namespace owns its map, so a `Bindings` rebuilt per condition
/// would clone every variable for every `when`. The reachability pipeline holds
/// a scope and loops conditions over it, which is exactly this shape.
pub(crate) struct Bindings<'a> {
    scope: &'a Scope,
    host: &'a Host,
    vars: Value,
}

impl<'a> Bindings<'a> {
    pub fn new(scope: &'a Scope, host: &'a Host) -> Self {
        let vars = scope
            .values
            .iter()
            .map(|(name, variable)| {
                (
                    name.as_ref().to_owned(),
                    // The valueless case and the missing key arrive at `""` by
                    // this one line: a dynamic declaration that ran and produced
                    // nothing reads exactly like a variable set to nothing.
                    variable.value.clone().unwrap_or_default(),
                )
            })
            .collect();
        Self {
            scope,
            host,
            vars: Namespace::value(VARS, vars),
        }
    }
}

/// The four-arm dispatch, and it needs no precedence check: `facts`, `env`, and
/// `vars` are all in `crate::var::RESERVED`, so no user variable can be named
/// any of them and the fourth arm cannot shadow the first three. That guarantee
/// looks like an oversight without this comment.
///
/// `true` and `false` never reach here at all — the grammar takes them as
/// literals before a variable is looked up.
impl VariableResolver for Bindings<'_> {
    fn resolve(&self, name: &str) -> Option<Value> {
        match name {
            FACTS => Some(self.host.facts.clone()),
            ENV => Some(self.host.env.clone()),
            VARS => Some(self.vars.clone()),
            // `None` is what the evaluator turns into `ResolveFailed`, which is
            // the undeclared-identifier error this module then names.
            _ => self
                .scope
                .get(name)
                .map(|variable| string(variable.value.clone().unwrap_or_default())),
        }
    }
}

/// A string-valued namespace whose lookups are total.
///
/// Hand-written rather than the crate's `DictObject`, and that is a correctness
/// requirement rather than a preference: the built-in dict answers an unknown
/// key with `NoSuchKey` and an unknown member with `UnknownMember`, which
/// contradicts `docs/repoformat.md`'s empty-string rule outright.
///
/// One type serves all three namespaces, so `facts.os`, `vars.work`, and
/// `env["XDG_CURRENT_DESKTOP"]` reach the same lookup by the same rule.
struct Namespace {
    /// The namespace's own name, which is what a type error reports.
    name: &'static str,
    entries: BTreeMap<String, String>,
}

impl Namespace {
    fn value(name: &'static str, entries: BTreeMap<String, String>) -> Value {
        Value::Object(Rc::new(Self { name, entries }))
    }

    fn lookup(&self, key: &str) -> Value {
        string(self.entries.get(key).cloned().unwrap_or_default())
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
        Ok(self.lookup(name))
    }

    fn get_key_value(&self, key: &str) -> ExpressionResult<Value> {
        Ok(self.lookup(key))
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
/// **Closed on both sides.** A falsy list with everything else true was weighed
/// and rejected: `profile = "personal"` written as `when = "profile"` would then
/// be silently, permanently true — a bare identifier where a comparison was
/// meant, and an action that always runs with nothing on screen. The set is
/// generous about boolean *spellings*, because a dotfiles manifest holds `"1"`
/// and `"yes"` constantly, and closed against everything else.
struct BatfilesCoercions;

/// A unit struct behind a `static`, mirroring the crate's own `STANDARD`, so
/// there is no lifetime to thread into `Evaluator::new_with_coercions`.
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
                other => Err(ExpressionError::EvaluationFailed(not_boolean(other))),
            },
            // A namespace, a list, a dict, or a function. None of them is a
            // decision, and each one names itself in the message.
            Value::Object(_) => Err(ExpressionError::EvaluationFailed(not_boolean(
                &value.as_str_lossy(),
            ))),
        }
    }

    /// Delegated: batfiles has no opinion about arithmetic, and a policy that
    /// invented one would be answering a question nobody asked.
    fn to_number(&self, value: &Value) -> ExpressionResult<Number> {
        STANDARD.to_number(value)
    }
}

/// The one sentence a value outside the table produces.
///
/// **One function, two callers, and that is the point.** The policy above is
/// applied from inside the evaluator (for `&&`, `||`, `!`) and by [`eval`] to
/// the finished value; the first surfaces as a crate error and lands in
/// [`EvalError::Failed`], the second is caught directly and becomes
/// [`EvalError::NotBoolean`]. Routing both through here is what keeps which path
/// fired invisible to the user — two hand-written messages drifting apart is the
/// failure mode this prevents.
fn not_boolean(value: &str) -> String {
    format!(
        "`{value}` is not a boolean. Write a comparison such as `== '{value}'`, \
         or use one of true, false, 1, 0, yes, no, on, or off"
    )
}

/// Evaluate one condition against one scope's bindings.
pub(crate) fn eval(condition: &Condition, bindings: &Bindings<'_>) -> Result<bool, EvalError> {
    let evaluator = Evaluator::new_with_coercions(bindings, &COERCIONS);
    let value = evaluator
        .evaluate(condition.expr())
        .map_err(|error| EvalError::from_expression(condition, &error))?;

    // The finished value goes through the same policy the evaluator applied
    // inside `!`, `&&`, and `||`, so there is no asymmetry between a whole
    // condition and a subexpression of one.
    COERCIONS
        .to_bool(&value)
        .map_err(|_| EvalError::NotBoolean {
            condition: condition.source().to_owned(),
            value: value.as_str_lossy(),
        })
}

/// Whether a record's gate opens.
///
/// Absent conditions open it, and `unless` is `when` negated. Resolved here
/// once, so the several callers that gate a `[remotes]` entry, an
/// `include-remote`, and a leaf action against three different scopes cannot
/// disagree about the direction.
///
/// Both present is rejected by
/// [`BatfilesConfig::validate`](crate::repo::BatfilesConfig::validate), so that
/// branch is unreachable for a validated manifest. It is written rather than
/// `unreachable!()`d because this is a pure function whose caller is not obliged
/// to have validated; **preferring `when` is arbitrary**, and is documented as
/// arbitrary rather than argued for.
pub(crate) fn gate(
    when: Option<&Condition>,
    unless: Option<&Condition>,
    bindings: &Bindings<'_>,
) -> Result<bool, EvalError> {
    match (when, unless) {
        (Some(when), _) => eval(when, bindings),
        (None, Some(unless)) => eval(unless, bindings).map(|open| !open),
        (None, None) => Ok(true),
    }
}

/// Why a condition could not be evaluated.
///
/// Every variant carries the condition's source text, because this is read long
/// after the manifest was parsed and "the condition failed" is not something a
/// user can act on — the same reason `dynamic::Identity` carries the key a user
/// types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EvalError {
    /// A bare identifier named nothing in any layer of the scope.
    Undeclared { condition: String, name: String },
    /// The condition's own value is outside the truthiness table.
    NotBoolean { condition: String, value: String },
    /// Everything else the evaluator can produce — a divide by zero, an index
    /// out of bounds, a member on a value that has none, a truthiness failure
    /// inside an operator — flattened.
    ///
    /// Flattened because the crate's `Error` is `#[non_exhaustive]` and batfiles
    /// has nothing useful to add to `index out of bounds: 5 (len: 3)` beyond
    /// saying which condition raised it. The wildcard arm that
    /// `#[non_exhaustive]` requires is this one.
    Failed { condition: String, message: String },
}

impl EvalError {
    fn from_expression(condition: &Condition, error: &ExpressionError) -> Self {
        let condition = condition.source().to_owned();
        match error {
            ExpressionError::ResolveFailed(name) => Self::Undeclared {
                condition,
                name: name.clone(),
            },
            // The inner string rather than the `Display`, which would prefix
            // "evaluation failed: ". This is the arm a truthiness failure inside
            // `&&`, `||`, or `!` arrives through, and the message is already
            // `not_boolean`'s.
            ExpressionError::EvaluationFailed(message) => Self::Failed {
                condition,
                message: message.clone(),
            },
            other => Self::Failed {
                condition,
                message: other.to_string(),
            },
        }
    }

    /// The condition this error is about, for a caller reporting it.
    pub fn condition(&self) -> &str {
        match self {
            Self::Undeclared { condition, .. }
            | Self::NotBoolean { condition, .. }
            | Self::Failed { condition, .. } => condition,
        }
    }
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // One frame for all three, so the fault clause is the only thing that
        // varies — which is what lets the two truthiness paths render alike.
        write!(
            f,
            "the condition `{}` cannot be evaluated: ",
            self.condition()
        )?;
        match self {
            // Both messages do the teaching: these are the two errors a
            // well-formed manifest can still hit.
            Self::Undeclared { name, .. } => write!(
                f,
                "`{name}` is not declared. Add `{name} = \"false\"` to [vars] in batfiles.toml, \
                 run `batfiles vars set {name} <value>`, or write `vars.{name}` if the variable \
                 is meant to be optional"
            ),
            Self::NotBoolean { value, .. } => f.write_str(&not_boolean(value)),
            Self::Failed { message, .. } => f.write_str(message),
        }
    }
}

impl std::error::Error for EvalError {}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::dynamic::{Absence, Refresh};
    use crate::scope::{Source, Variable};
    use crate::var::VarName;

    /// A scope built directly rather than through `Scope::leaf`: this module
    /// cares about what a name is bound to, not about which layer bound it.
    fn scope(bindings: &[(&str, Option<&str>)]) -> Scope {
        Scope {
            values: bindings
                .iter()
                .map(|(name, value)| {
                    (
                        VarName::new(name).expect("valid name"),
                        Variable {
                            value: value.map(str::to_owned),
                            source: Source::LeafVars,
                            // A declaration that ran and produced nothing is
                            // what a real resolution yields for the middle case.
                            refresh: value
                                .is_none()
                                .then_some(Refresh::Missing(Absence::CommandFailed)),
                        },
                    )
                })
                .collect(),
        }
    }

    fn host(environment: &[(&str, &str)]) -> Host {
        Host::capture(&Environment::from_pairs(environment.iter().copied()))
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

    #[test]
    fn a_declared_variable_resolves_to_its_value() {
        let scope = scope(&[("profile", Some("personal")), ("work", Some("true"))]);
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        // The three comparison spellings the spec's examples use.
        assert!(truth("profile == 'personal'", &bindings));
        assert!(truth("profile != 'work'", &bindings));
        assert!(truth("work && profile == 'personal'", &bindings));
    }

    #[test]
    fn a_bare_identifier_has_three_cases() {
        // Decision 5, whole. The middle row is the one step 6 deliberately
        // handed here, and it is not collapsed into either neighbor.
        let scope = scope(&[("valued", Some("yes")), ("valueless", None)]);
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        assert!(truth("valued", &bindings), "declared, with a value");
        assert!(
            !truth("valueless", &bindings),
            "declared but valueless reads as the empty string"
        );
        assert_eq!(
            failure("undeclared", &bindings),
            EvalError::Undeclared {
                condition: "undeclared".to_owned(),
                name: "undeclared".to_owned(),
            }
        );
    }

    #[test]
    fn vars_is_total_where_the_bare_identifier_is_strict() {
        // Decision 6, and the contrast is the whole of it: the two spellings can
        // never disagree about a variable that *is* declared.
        let scope = scope(&[("valued", Some("yes")), ("valueless", None)]);
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        assert!(truth("vars.valued", &bindings));
        assert!(truth("vars.valued == 'yes'", &bindings));
        assert!(!truth("vars.valueless", &bindings));

        // The case the namespace exists for.
        assert!(!truth("vars.undeclared", &bindings));
        assert!(truth("vars.undeclared == ''", &bindings));
        assert!(matches!(
            failure("undeclared", &bindings),
            EvalError::Undeclared { .. }
        ));

        // And it composes, which is what makes it usable rather than a curiosity.
        assert!(truth("vars.valued || vars.undeclared", &bindings));
        assert!(!truth("vars.undeclared || vars.valueless", &bindings));
    }

    #[test]
    fn the_member_and_indexed_spellings_of_vars_are_one_lookup() {
        let scope = scope(&[("work", Some("1"))]);
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        assert!(truth("vars.work == vars['work']", &bindings));
        assert!(truth("vars['work']", &bindings));
    }

    #[test]
    fn facts_answers_both_spellings_and_a_missing_key_is_empty() {
        let scope = Scope::default();
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        assert!(truth(
            &format!("facts.os == '{}'", std::env::consts::OS),
            &bindings
        ));
        assert!(truth(
            &format!("facts['arch'] == '{}'", std::env::consts::ARCH),
            &bindings
        ));
        // The footgun `docs/repoformat.md` accepts, pinned as behavior: a typo
        // in a fact name is false, not a failure.
        assert!(!truth("facts.arhc == 'arm64'", &bindings));
        assert!(truth("facts.arhc == ''", &bindings));
    }

    #[test]
    fn env_answers_both_spellings_and_a_missing_key_is_empty() {
        let scope = Scope::default();
        let host = host(&[("HOME", "/home/me"), ("XDG_CURRENT_DESKTOP", "GNOME")]);
        let bindings = Bindings::new(&scope, &host);

        assert!(truth("env.HOME == '/home/me'", &bindings));
        // The spelling the spec requires for a key that is not an identifier —
        // except this one is, so the point is that indexing works at all.
        assert!(truth("env[\"XDG_CURRENT_DESKTOP\"] == 'GNOME'", &bindings));
        assert!(truth("env.NOPE == ''", &bindings));
        assert!(truth("env.HOME != ''", &bindings));
    }

    #[test]
    fn the_truthiness_table_is_closed_on_both_sides() {
        // Decision 7, exhaustively. This is the specification, so it is written
        // as one.
        let scope = Scope::default();
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        for truthy in ["true", "'true'", "'1'", "'yes'", "'on'", "1", "-1", "2.5"] {
            assert!(truth(truthy, &bindings), "{truthy} should be true");
        }
        for falsy in ["false", "'false'", "'0'", "'no'", "'off'", "''", "0", "0.0"] {
            assert!(!truth(falsy, &bindings), "{falsy} should be false");
        }
        for neither in ["'personal'", "'True'", "'y'", "'2'"] {
            assert!(
                matches!(failure(neither, &bindings), EvalError::NotBoolean { .. }),
                "{neither} is outside the table"
            );
        }
    }

    #[test]
    fn the_policy_applies_inside_the_operators_too() {
        // The whole reason this step wants the crate's `Coercions`: a pre-0.4
        // design would need a `bool()` builtin and would fail every line here.
        let scope = scope(&[("work", Some("1")), ("school", Some("0"))]);
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        assert!(!truth("work && school", &bindings));
        assert!(truth("work || school", &bindings));
        assert!(!truth("!work", &bindings));
        assert!(truth("!school", &bindings));
        assert!(truth("vars.work || vars.nothere", &bindings));
        assert!(!truth("vars.nothere && vars.work", &bindings));
    }

    #[test]
    fn a_value_outside_the_table_reads_the_same_from_either_path() {
        // Decision 12's one-`not_boolean` rule, pinned. `profile` is caught by
        // `eval` on the finished value; `profile && work` is raised inside the
        // evaluator. A later hand-edit to either message breaks this.
        let scope = scope(&[("profile", Some("personal")), ("work", Some("true"))]);
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        let fault = format!(": {}", not_boolean("personal"));
        for source in ["profile", "profile && work", "!profile"] {
            let message = failure(source, &bindings).to_string();
            assert!(message.ends_with(&fault), "{source}: {message}");
            assert!(message.contains(&format!("`{source}`")), "{message}");
        }
    }

    #[test]
    fn an_undeclared_identifier_names_all_three_fixes() {
        let scope = Scope::default();
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        let message = failure("work && facts.os == 'macos'", &bindings).to_string();
        assert!(
            message.contains("`work && facts.os == 'macos'`"),
            "{message}"
        );
        assert!(message.contains("`work = \"false\"`"), "{message}");
        assert!(
            message.contains("`batfiles vars set work <value>`"),
            "{message}"
        );
        assert!(message.contains("`vars.work`"), "{message}");
    }

    #[test]
    fn a_namespace_is_not_itself_a_boolean() {
        let scope = Scope::default();
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        assert!(matches!(
            failure("facts", &bindings),
            EvalError::NotBoolean { .. }
        ));
    }

    #[test]
    fn an_evaluation_fault_that_is_neither_is_flattened_and_names_the_condition() {
        let scope = Scope::default();
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        let error = failure("vars.keys()", &bindings);
        assert!(matches!(error, EvalError::Failed { .. }));
        assert!(error.to_string().contains("`vars.keys()`"), "{error}");
    }

    #[test]
    fn integer_overflow_is_an_evaluation_error_rather_than_an_abort() {
        // Before `simple-expressions` 0.4.1 this arithmetic was unchecked: `+`,
        // `-`, and `*` panicked in debug and *silently wrapped* in release,
        // while `%` at `i64::MIN` against `-1` panicked in every profile,
        // because Rust checks remainder overflow regardless of
        // `overflow-checks`. A manifest is not always the user's own — batfiles
        // evaluates a third-party remote's — so repository content could abort
        // the tool. It now arrives as an ordinary [`EvalError`].
        let scope = Scope::default();
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

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
            // The flattening earns its keep here: batfiles has nothing to add to
            // the crate's message beyond saying which condition raised it.
            let message = error.to_string();
            assert!(message.contains("integer overflow"), "{message}");
            assert!(message.contains(&format!("`{source}`")), "{message}");
        }

        // Division and exponentiation promote to floating point rather than
        // overflowing, so they have no integer edge to check — but the zero
        // divisor still has to be an error and not a trap.
        assert!(matches!(
            failure("1 / 0 == 0", &bindings),
            EvalError::Failed { .. }
        ));
    }

    #[test]
    fn a_gate_opens_when_nothing_conditions_it() {
        let scope = scope(&[("work", Some("true")), ("home", Some("false"))]);
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        let when = condition("work");
        let never = condition("home");

        assert!(gate(None, None, &bindings).expect("no conditions"));
        assert!(gate(Some(&when), None, &bindings).expect("when passes through"));
        assert!(!gate(Some(&never), None, &bindings).expect("when passes through"));
        assert!(!gate(None, Some(&when), &bindings).expect("unless negates"));
        assert!(gate(None, Some(&never), &bindings).expect("unless negates"));
        // Arbitrary, documented as arbitrary, and unreachable for a validated
        // manifest — but a pure function still has to do something.
        assert!(gate(Some(&when), Some(&when), &bindings).expect("both present takes `when`"));
    }

    #[test]
    fn the_captured_facts_describe_the_host_running_the_test() {
        // Asserted against `std::env::consts` rather than a hard-coded platform,
        // so this passes on every runner.
        let scope = Scope::default();
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

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
    fn the_hostname_is_the_host_name_verbatim() {
        // Not truncated at a dot: a fully qualified name stays fully qualified,
        // which is decision 8 and the thing `docs/environment.md` warns about.
        let expected = gethostname::gethostname().to_string_lossy().into_owned();
        let scope = Scope::default();
        let host = host(&[]);
        let bindings = Bindings::new(&scope, &host);

        assert!(!expected.is_empty(), "the host has no name");
        assert!(truth(&format!("facts.hostname == '{expected}'"), &bindings));
    }
}
