//! `vars eval`: evaluate a template in the leaf's scope, as a `[vars]` value would be, and say
//! how a condition would take the result. See
//! [`docs/commands/vars.md`](../../../docs/commands/vars.md#vars-eval).

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::commands::Session;
use crate::error::Error;
use crate::manifest::check::ValueRule;
use crate::manifest::condition::{self, Bindings, HostNamespaces};
use crate::manifest::template::{EnvAccess, Resolver, TemplateError, TemplateString};
use crate::run::lock::RunLock;
use crate::vars::dynamic::CachePolicy;
use crate::vars::env_overrides;
use crate::vars::machine::MachineVars;
use crate::vars::name::VarName;
use crate::vars::scope::VarScope;

/// How a failure names what failed.
const RECORD: &str = "vars eval";
const FIELD: &str = "template";

/// How a chain of variables read too deeply names the template, which is not a variable name.
const CHAIN_LABEL: &str = "the template";

/// What `vars eval` was asked to do.
pub(crate) struct EvalRequest<'a> {
    pub template: &'a str,
    /// Leave the leaf's `[vars]` out, reading no repository.
    pub no_manifest_vars: bool,
    /// Capture no dynamic variable and write no cache.
    pub no_refresh: bool,
    /// The `--var` overrides, in written order.
    pub vars: &'a [(VarName, String)],
}

/// Print `request.template`, evaluated in the leaf's scope, on standard output, then say on
/// standard error how a condition would take it.
///
/// A template that does not parse fails before anything is read. Unless the leaf's `[vars]`
/// are left out, the leaf's manifest is read and its dynamic variables are resolved as
/// [`vars list`](super::list) resolves them, under the run lock, held until this returns, when
/// they may be captured. The template counts toward the nesting limit as a `[vars]` value does.
/// Each dynamic variable the template reads that `no_refresh` left uncaptured warns, whether or
/// not the evaluation succeeds. Fails as a template field does when it cannot be evaluated.
pub(crate) fn run(request: &EvalRequest<'_>, session: &Session<'_>) -> Result<(), Error> {
    let template = TemplateString::from(request.template.to_owned());
    if let Err(syntax) = template.syntax() {
        return Err(Error::Template {
            record: RECORD.to_owned(),
            field: FIELD,
            template: template.source().to_owned(),
            source: Box::new(TemplateError::Syntax(syntax.clone())),
        });
    }

    let env = session.env;
    let reporter = session.reporter;
    let host = HostNamespaces::capture(env);
    // Held until the command returns, when a refresh may write the cache.
    let mut _lock: Option<RunLock> = None;
    let scope = if request.no_manifest_vars {
        let roots = session.machine_roots()?;
        let machine = MachineVars::load(&roots.machine_vars_path())?;
        let environment = env_overrides::overrides(env, reporter);
        Rc::new(VarScope::stack(
            BTreeMap::new(),
            machine.values,
            environment,
            request.vars,
        ))
    } else {
        let roots = session.repository_roots()?;
        let policy = if request.no_refresh {
            CachePolicy::Never
        } else {
            _lock = Some(RunLock::acquire(&roots.machine.cache_dir)?);
            CachePolicy::Auto
        };
        super::leaf_scope(&roots, env, request.vars, policy, &host, reporter)?
    };

    let bindings = Bindings::new(&scope, &host);
    // Evaluated as a `[vars]` value is, so that it nests as deeply as one could.
    let value = scope.as_value(CHAIN_LABEL, || {
        Resolver::new(RECORD, &bindings, EnvAccess::Allowed).field(
            FIELD,
            &template,
            ValueRule::Text,
        )
    });
    for (name, state) in scope.uncaptured_reads() {
        reporter.warn(&format!(
            "the template reads `{name}`, which --no-refresh did not capture: {state}"
        ));
    }
    let value = value?;
    reporter.data(&value);
    reporter.info(as_condition(condition::truthiness(&value)));
    Ok(())
}

/// How `when` and `unless` would take a variable holding a value of this truthiness.
fn as_condition(truthiness: Option<bool>) -> &'static str {
    match truthiness {
        Some(true) => "as a condition, this is true: `when` includes and `unless` skips",
        Some(false) => "as a condition, this is false: `when` skips and `unless` includes",
        None => {
            "as a condition, this fails: it is not true, false, 1, 0, yes, no, on, off, or empty"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_truthiness_says_what_both_conditions_do() {
        assert!(as_condition(Some(true)).contains("`when` includes and `unless` skips"));
        assert!(as_condition(Some(false)).contains("`when` skips and `unless` includes"));
        assert!(as_condition(None).contains("this fails"));
    }
}
