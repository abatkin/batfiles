//! Read validated `BATFILES_VAR_*` overrides from the captured environment.
//! [`crate::var_set`] applies their precedence relative to other layers.
//!
//! Only the prefix creates a user variable: `BATFILES_VAR_FOO` supplies `FOO`
//! and remains readable as `env.BATFILES_VAR_FOO`. An unprefixed `FOO` is only
//! available through `env.FOO`.
//!
//! Invalid ambient names warn and are dropped; invalid explicit `--var` names
//! fail the invocation in [`crate::cli::options`].
//!
//! Warnings omit values, which may be tokens; explicitly requested listings may
//! show them. Rejected names are arbitrary text, so escape them with
//! [`quoted_value`] to prevent embedded newlines from forging diagnostic lines.

use std::collections::BTreeMap;

use crate::env::Environment;
use crate::output::{Reporter, quoted_value};
use crate::var::VarName;

/// The one-shot variables `BATFILES_VAR_*` sets, keyed by name.
///
/// A suffix that is not a usable variable name — including a reserved one, so
/// `BATFILES_VAR_env` — is reported and left out; the rest of the environment is
/// still read, and the run continues.
///
/// This is the third layer of the [variable set](crate::var_set) a run
/// resolves, above `vars.toml` and below `--var`.
pub(crate) fn one_shot(env: &Environment, reporter: &Reporter) -> BTreeMap<VarName, String> {
    let mut values = BTreeMap::new();
    for (name, value) in env.one_shot_vars() {
        match VarName::try_from(name.to_owned()) {
            Ok(name) => {
                values.insert(name, value.to_owned());
            }
            Err(error) => reporter.warn(&format!(
                "ignoring {}: {error}",
                quoted_value(&format!("{}{name}", Environment::VAR_PREFIX))
            )),
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Verbosity;

    /// Warnings are printed whatever the verbosity, so what these tests read is
    /// the map. The wording reaches a user through a CLI test.
    fn quiet() -> Reporter {
        let mut reporter = Reporter::new(false);
        reporter.set_verbosity(Verbosity::Quiet);
        reporter
    }

    fn one_shot_of<const N: usize>(pairs: [(&str, &str); N]) -> Vec<(String, String)> {
        one_shot(&Environment::from_pairs(pairs), &quiet())
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect()
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn a_prefixed_variable_defines_the_name_after_the_prefix() {
        assert_eq!(
            one_shot_of([("BATFILES_VAR_EDITOR", "nvim"), ("EDITOR", "emacs")]),
            vec![pair("EDITOR", "nvim")]
        );
    }

    #[test]
    fn an_empty_value_defines_the_variable_as_the_empty_string() {
        assert_eq!(
            one_shot_of([("BATFILES_VAR_PROFILE", "")]),
            vec![pair("PROFILE", "")]
        );
    }

    #[test]
    fn an_unusable_name_is_dropped_and_the_rest_are_read() {
        // The order is the environment's, so the survivors do not depend on
        // where in it the bad name sat.
        assert_eq!(
            one_shot_of([
                ("BATFILES_VAR_1up", "x"),
                ("BATFILES_VAR_editor", "nvim"),
                ("BATFILES_VAR_has-dash", "x"),
            ]),
            vec![pair("editor", "nvim")]
        );
    }

    #[test]
    fn a_reserved_name_is_unusable_like_any_other() {
        // `BATFILES_VAR_env` reads as though it should reach the `env`
        // namespace, and it is exactly the name no user variable may have.
        assert!(one_shot_of([("BATFILES_VAR_env", "x")]).is_empty());
    }

    #[test]
    fn a_bare_prefix_defines_nothing() {
        assert!(one_shot_of([("BATFILES_VAR_", "orphan")]).is_empty());
    }

    #[test]
    fn a_name_keeps_the_case_it_was_written_in() {
        // On Windows `capture` has already uppercased both of these into one
        // key; on Unix they are two variables, as they are two names here.
        assert_eq!(
            one_shot_of([
                ("BATFILES_VAR_editor", "nvim"),
                ("BATFILES_VAR_EDITOR", "vi")
            ]),
            vec![pair("EDITOR", "vi"), pair("editor", "nvim")]
        );
    }
}
