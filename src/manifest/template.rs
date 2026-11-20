//! Parse and evaluate [template strings](../../docs/repoformat.md#templates): manifest values
//! with `${...}` expressions in them. A template is parsed when its manifest loads and evaluated
//! against the same [`Bindings`] a condition on its record uses, only when its value is needed.

use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::rc::Rc;

use serde::Deserialize;
use simple_expressions::evaluator::VariableResolver;
use simple_expressions::template::Template;
use simple_expressions::types::error::{Error as ExpressionError, Result as ExpressionResult};
use simple_expressions::types::object::Object;
use simple_expressions::types::value::Value;

use crate::error::Error;
use crate::manifest::check::{ValueError, ValueRule};
use crate::manifest::condition::{Bindings, COERCIONS, EvalError, Position, SyntaxError};
use crate::manifest::item::ItemId;

/// The reserved namespace of host environment variables.
const ENV: &str = "env";

/// A template string, parsed as it is read. A value that does not parse is kept with its
/// error, so that manifest validation can report it with the record and field that hold it.
#[derive(Clone, Deserialize)]
#[serde(from = "String")]
pub(crate) struct TemplateString {
    /// The value exactly as written.
    source: String,
    parsed: Result<Template, SyntaxError>,
}

impl TemplateString {
    /// The value exactly as written.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Why the value does not parse, if it does not.
    pub fn syntax(&self) -> Result<(), &SyntaxError> {
        self.parsed.as_ref().map(|_| ())
    }

    /// The value, with escapes applied, if it parses and holds no `${...}`.
    pub fn literal(&self) -> Option<&str> {
        self.parsed.as_ref().ok().and_then(Template::as_literal)
    }

    /// The value, evaluated in `scope` and checked against `rule`. A literal value was checked
    /// when its manifest loaded and is returned as is.
    fn resolve(
        &self,
        rule: ValueRule,
        scope: &TemplateScope<'_, '_>,
    ) -> Result<Cow<'_, str>, TemplateError> {
        let template = self
            .parsed
            .as_ref()
            .map_err(|error| TemplateError::Syntax(error.clone()))?;
        if let Some(literal) = template.as_literal() {
            return Ok(Cow::Borrowed(literal));
        }
        let text = template
            .evaluate_rendered(scope, &COERCIONS, render)
            .map_err(|error| TemplateError::from_expression(&error))?;
        rule.check(&text).map_err(TemplateError::Invalid)?;
        Ok(Cow::Owned(text))
    }
}

impl From<String> for TemplateString {
    fn from(source: String) -> Self {
        let parsed =
            Template::parse(&source).map_err(|error| SyntaxError::new("template", &source, &error));
        Self { source, parsed }
    }
}

/// Debug output shows the original template text.
impl fmt::Debug for TemplateString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TemplateString")
            .field(&self.source())
            .finish()
    }
}

/// Render one expression's value: a string, number, or boolean. Never include a refused
/// value, which may contain secrets.
fn render(value: &Value) -> Result<String, ExpressionError> {
    match value {
        Value::Primitive(primitive) => Ok(primitive.as_str_lossy()),
        Value::Object(_) => Err(ExpressionError::EvaluationFailed(NOT_SCALAR.to_owned())),
    }
}

/// Diagnostic for an expression whose value cannot be written into text.
const NOT_SCALAR: &str = "its value is not a string, number, or boolean";

/// Whether a record's templates may read `env`.
#[derive(Debug, Clone, Copy)]
pub(crate) enum EnvAccess<'a> {
    /// A leaf record or remote.
    Allowed,
    /// A record contributed by an inclusion of this remote, which does not set
    /// `allow-env-templates`.
    DeniedBy(&'a ItemId),
}

/// What a template evaluates against: the bindings a condition on its record sees, except that
/// `env` holds no data when the record may not read the host environment.
struct TemplateScope<'a, 'b> {
    bindings: &'a Bindings<'b>,
    /// A [`DeniedEnv`] standing in for `env`, or `None` when `env` may be read.
    denied_env: Option<Value>,
}

impl<'a, 'b> TemplateScope<'a, 'b> {
    fn new(bindings: &'a Bindings<'b>, env: EnvAccess<'_>) -> Self {
        let denied_env = match env {
            EnvAccess::Allowed => None,
            EnvAccess::DeniedBy(remote) => Some(Value::Object(Rc::new(DeniedEnv {
                remote: remote.to_string(),
            }))),
        };
        Self {
            bindings,
            denied_env,
        }
    }
}

impl VariableResolver for TemplateScope<'_, '_> {
    fn resolve(&self, name: &str) -> Option<Value> {
        match &self.denied_env {
            Some(denied) if name == ENV => Some(denied.clone()),
            _ => self.bindings.resolve(name),
        }
    }
}

/// The `env` a contributed record sees when its remote does not set `allow-env-templates`. It
/// holds no environment, so nothing can be read through it, and every lookup in it fails.
struct DeniedEnv {
    remote: String,
}

impl DeniedEnv {
    fn refusal(&self) -> ExpressionError {
        ExpressionError::EvaluationFailed(format!(
            "reads `env`, which a template in an included manifest may do only if the leaf's \
             `[remotes.{}]` sets `allow-env-templates = true`",
            self.remote
        ))
    }
}

impl Object for DeniedEnv {
    fn type_name(&self) -> &'static str {
        ENV
    }

    fn get_member(&self, _name: &str) -> ExpressionResult<Value> {
        Err(self.refusal())
    }

    fn get_index(&self, _index: i64) -> ExpressionResult<Value> {
        Err(self.refusal())
    }

    fn get_key_value(&self, _key: &str) -> ExpressionResult<Value> {
        Err(self.refusal())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Resolves one record's template fields, naming the record as `record` in failures.
pub(crate) struct Resolver<'a, 'b> {
    record: &'a str,
    scope: TemplateScope<'a, 'b>,
}

impl<'a, 'b> Resolver<'a, 'b> {
    /// Resolve the fields of the record headed `record` against `bindings`, which hold the scope
    /// a condition on it would see, reading `env` as `env` permits.
    pub fn new(record: &'a str, bindings: &'a Bindings<'b>, env: EnvAccess<'_>) -> Self {
        Self {
            record,
            scope: TemplateScope::new(bindings, env),
        }
    }

    /// The heading of the record whose fields this resolves.
    pub fn record(&self) -> &str {
        self.record
    }

    /// `value`, written in `field`, evaluated and checked against `rule`. Failures are
    /// [`Error::Template`].
    pub fn field<'t>(
        &self,
        field: &'static str,
        value: &'t TemplateString,
        rule: ValueRule,
    ) -> Result<Cow<'t, str>, Error> {
        value
            .resolve(rule, &self.scope)
            .map_err(|source| Error::Template {
                record: self.record.to_owned(),
                field,
                template: value.source().to_owned(),
                source: Box::new(source),
            })
    }

    /// [`Self::field`] for a field that may be absent.
    pub fn optional<'t>(
        &self,
        field: &'static str,
        value: Option<&'t TemplateString>,
        rule: ValueRule,
    ) -> Result<Option<Cow<'t, str>>, Error> {
        value
            .map(|value| self.field(field, value, rule))
            .transpose()
    }
}

/// Why a template field has no usable value. Messages follow the field and its template.
#[derive(Debug)]
pub(crate) enum TemplateError {
    /// The template does not parse. Manifest validation reports this before any evaluation.
    Syntax(SyntaxError),
    /// An expression failed, at this position in the template.
    Evaluation {
        position: Option<Position>,
        error: EvalError,
    },
    /// The resolved value breaks its field's rule. The value is not kept: it may hold what a
    /// variable holds.
    Invalid(ValueError),
}

impl TemplateError {
    fn from_expression(error: &ExpressionError) -> Self {
        match error {
            ExpressionError::InterpolationFailed {
                line,
                column,
                error,
                ..
            } => Self::Evaluation {
                position: Some(Position {
                    line: *line,
                    column: *column,
                }),
                error: EvalError::from_expression(error),
            },
            other => Self::Evaluation {
                position: None,
                error: EvalError::from_expression(other),
            },
        }
    }
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(error) => f.write_str(&error.problem()),
            Self::Evaluation {
                position: Some(position),
                error,
            } => write!(f, "cannot be evaluated at {position}: {error}"),
            Self::Evaluation {
                position: None,
                error,
            } => write!(f, "cannot be evaluated: {error}"),
            Self::Invalid(error) => write!(f, "resolves to a value that {error}"),
        }
    }
}

impl std::error::Error for TemplateError {}

#[cfg(test)]
mod tests;
