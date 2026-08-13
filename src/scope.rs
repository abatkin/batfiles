//! Layering variables into a scope: precedence, provenance, and nothing else.
//!
//! This is the one implementation of
//! `docs/environment.md`'s runtime variable precedence, for both layer lists.
//! It is a **pure merge**: nothing here opens a file, reads a clock, or spawns a
//! command. Which declarations exist is the reachability pipeline's answer, what
//! a dynamic command produced is [`dynamic::resolve`](crate::dynamic::resolve)'s,
//! and how a scope is rendered is `vars list`'s. A `Scope` that reads anything
//! has absorbed a job belonging to one of them.
//!
//! Two rules about *declarations* are settled here rather than left to the
//! layer lists, because a reader will otherwise guess at them:
//!
//! - **A remote that may not run commands contributes no dynamic variables at
//!   all.** [`Scope::inclusion`] skips every dynamic declaration in the remote's
//!   layer when [`IncludedRemote::allow_dynamic_vars`] is false, so a lower
//!   layer's value stands and the name has no row of its own. The manifest and
//!   `vars refresh <key>` are the only surfaces that explain it; nothing should
//!   go looking for a scope entry that is not there.
//! - **A declaration that ran and produced nothing still overrides the layers
//!   beneath it.** The higher declaration won, and it produced nothing; falling
//!   back to a lower layer's string would be a precedence rule nobody wrote.
//!
//! Those two look alike and are not. The first is a rule, and the second is
//! ordinary; a dynamic declaration that is *allowed* and has no outcome at all
//! is neither, and panics — see [`RESOLVED`].
#![allow(
    dead_code,
    reason = "no command dispatches to a scope until `vars list`"
)]

use std::collections::BTreeMap;

use crate::config::Environment;
use crate::dynamic::{Refresh, Resolution, Resolved};
use crate::item::ItemId;
use crate::output::Reporter;
use crate::repo::{IncludedRemote, Inclusion, Repository, VarDecl};
use crate::state::MachineVars;
use crate::var::{VarName, VarNameError};

/// One name's effective binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Variable {
    /// Absent when a dynamic declaration produced nothing: every
    /// [`Refresh::Missing`]. A *static* binding always has a value, the empty
    /// string included.
    pub value: Option<String>,
    /// The layer the value came from.
    pub source: Source,
    /// How a dynamic declaration arrived at it, when one did.
    pub refresh: Option<Refresh>,
}

/// The effective variables of one repository or one inclusion.
///
/// A name with no value is still a name: "declared but valueless" and "not
/// declared" are different answers, and the second is what `vars get`'s
/// absent-key failure exists to preserve. How a valueless identifier
/// *evaluates* is condition evaluation's rule, next to the `facts` and `env`
/// resolvers that already have a missing-key rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Scope {
    pub values: BTreeMap<VarName, Variable>,
}

/// Where a value came from. **Declaration order is precedence order**, low to
/// high, and `Ord` is derived from it.
///
/// Both of `docs/environment.md`'s layer lists are ascending subsequences of
/// this one total order — that is the whole content of "one precedence
/// implementation", and [`merge`] asserts it rather than trusting it.
///
/// [`RemoteVars`](Source::RemoteVars) is unqualified: an inclusion scope has
/// exactly one remote, and the listing's section header already names the
/// inclusion, so carrying the [`ItemId`] here would be a second spelling of
/// something already on screen. There is no `Display` either — rendering a
/// provenance label lands in the module that first needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Source {
    RemoteVars,
    LeafVars,
    InclusionVars,
    Machine,
    Environment,
    CommandLine,
}

/// The layers that are identical in every scope of one invocation: `vars.toml`,
/// `BATFILES_VAR_*`, and `--var`.
///
/// Built once rather than per scope. One run produces a leaf scope and a scope
/// per effective inclusion, so validating `BATFILES_VAR_*` inside the merge
/// would print the same complaint about the same stray environment variable
/// once per scope; building the overlay ahead of them makes "warn once"
/// structural rather than a deduplication pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Overlay {
    machine: BTreeMap<VarName, String>,
    environment: BTreeMap<VarName, String>,
    command_line: BTreeMap<VarName, String>,
}

impl Overlay {
    /// Build the three override layers, warning about the environment names
    /// that are not valid variable names and dropping them.
    ///
    /// **Infallible.** `--var` keys arrive already validated from the command
    /// line, where an invalid one is a usage error raised before any file is
    /// opened, and an invalid `BATFILES_VAR_*` suffix is a warning rather than
    /// an error (`docs/environment.md`), so no input here can fail. The
    /// `reporter` is a sink for those warnings and nothing else.
    ///
    /// `vars list` passes `&[]` for `command_line`, which is
    /// `docs/state.md`'s "`--var` is not included" with no special case: the
    /// command has no `--var` option to pass.
    pub fn build(
        machine: &MachineVars,
        environment: &Environment,
        command_line: &[(VarName, String)],
        reporter: &Reporter,
    ) -> Self {
        let mut one_shot = BTreeMap::new();
        for (suffix, value) in environment.one_shot_vars() {
            match VarName::new(suffix) {
                Ok(name) => {
                    one_shot.insert(name, value.to_owned());
                }
                Err(error) => reporter.warn(&ignored(suffix, error)),
            }
        }

        Self {
            machine: machine.values.clone(),
            environment: one_shot,
            // A repeated `--var` key takes its last value
            // (`docs/cmdline.md`), which is what collecting a sequence into a
            // map already does.
            command_line: command_line.iter().cloned().collect(),
        }
    }

    /// The three layers, in precedence order, as both constructors append them.
    fn layers(&self) -> [Layer<'_>; 3] {
        [
            Layer {
                source: Source::Machine,
                entries: Entries::Strings(&self.machine),
            },
            Layer {
                source: Source::Environment,
                entries: Entries::Strings(&self.environment),
            },
            Layer {
                source: Source::CommandLine,
                entries: Entries::Strings(&self.command_line),
            },
        ]
    }
}

/// The line a `BATFILES_VAR_*` name that is not a variable name prints.
///
/// Pure, because [`Reporter::warn`] writes straight to standard error with no
/// seam a unit test can reach — the same shape the dynamic resolver's `warning`
/// takes, and for the same reason. The **whole environment variable** is named,
/// prefix included, since that is what the user has to go delete; the bare
/// suffix the merge rejected is not something they can find.
fn ignored(suffix: &str, error: VarNameError) -> String {
    format!("ignoring `{}{suffix}`: {error}", Environment::VAR_PREFIX)
}

/// Every dynamic outcome the caller has, keyed the way a layer looks one up.
///
/// An inclusion scope merges declarations from two repositories whose outcomes
/// come from two different [`dynamic::resolve`](crate::dynamic::resolve) calls,
/// so a single [`Resolution`] would not do and a pair of them would be two
/// parameters every constructor has to keep straight. One index, built once and
/// handed to every constructor, also keeps the lookup out of a linear scan per
/// name.
///
/// The key is a tuple of references rather than an
/// [`Identity`](crate::dynamic::Identity), so a lookup allocates nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Outcomes<'a> {
    by_identity: BTreeMap<(Option<&'a ItemId>, &'a VarName), &'a Resolved>,
}

impl<'a> Outcomes<'a> {
    /// Index every outcome of every resolution the caller performed.
    pub fn index(resolutions: impl IntoIterator<Item = &'a Resolution>) -> Self {
        let by_identity = resolutions
            .into_iter()
            .flat_map(|resolution| resolution.vars.iter())
            .map(|resolved| {
                (
                    (resolved.identity.remote.as_ref(), &resolved.identity.name),
                    resolved,
                )
            })
            .collect();
        Self { by_identity }
    }

    /// The outcome of one declaration, or `None` if the caller never resolved
    /// it.
    fn get(&self, remote: Option<&ItemId>, name: &VarName) -> Option<&'a Resolved> {
        self.by_identity.get(&(remote, name)).copied()
    }
}

/// The invariant every allowed dynamic declaration holds by construction: the
/// pipeline resolves a layer's declarations before it builds the layer.
///
/// A hard `expect` rather than a `debug_assert!` with a release fallback, and
/// the difference matters. Contributing `{ value: None, .. }` instead is only
/// *visible* in `vars list`, where a valueless row reads as wrong; conditions
/// evaluate against this same scope, and there a valueless variable silently
/// flips a `when` with nothing on screen at all. A behavior that exists only in
/// release is also one `task ci` never executes, since it runs `cargo test` in
/// debug. `app::dispatch`'s `HANDED` is the precedent: a cross-module pipeline
/// invariant, held by construction, written as a sentence.
const RESOLVED: &str = "a declaration that reaches a scope has a resolved outcome";

impl Scope {
    /// leaf `[vars]` < vars.toml < BATFILES_VAR_* < --var
    ///
    /// A `&Repository` rather than a `&Leaf` on purpose: the pipeline builds
    /// the leaf scope *before* it reads any remote, when it is holding the
    /// structural walk and not a whole `Leaf`.
    pub fn leaf(leaf: &Repository, outcomes: &Outcomes<'_>, overlay: &Overlay) -> Self {
        let mut layers = vec![Layer {
            source: Source::LeafVars,
            entries: Entries::Declared {
                vars: &leaf.config.vars,
                remote: None,
                allow_dynamic: true,
            },
        }];
        layers.extend(overlay.layers());
        merge(&layers, outcomes)
    }

    /// remote `[vars]` < leaf `[vars]` < inclusion vars < vars.toml
    /// < BATFILES_VAR_* < --var
    ///
    /// Takes the [`IncludedRemote`] the caller already looked up rather than
    /// looking it up from a `Leaf`, so it returns a `Scope` rather than a
    /// `Result` over a case the loader forbids.
    ///
    /// A remote whose state is `NoManifest` or `NotMaterialized` carries an
    /// empty config, so its layer contributes nothing with no branch here.
    pub fn inclusion(
        remote: &IncludedRemote,
        leaf: &Repository,
        inclusion: &Inclusion,
        outcomes: &Outcomes<'_>,
        overlay: &Overlay,
    ) -> Self {
        let mut layers = vec![
            Layer {
                source: Source::RemoteVars,
                entries: Entries::Declared {
                    vars: &remote.repo.config.vars,
                    remote: Some(&remote.id),
                    allow_dynamic: remote.allow_dynamic_vars,
                },
            },
            Layer {
                source: Source::LeafVars,
                entries: Entries::Declared {
                    vars: &leaf.config.vars,
                    remote: None,
                    allow_dynamic: true,
                },
            },
            Layer {
                source: Source::InclusionVars,
                entries: Entries::Strings(&inclusion.vars),
            },
        ];
        layers.extend(overlay.layers());
        merge(&layers, outcomes)
    }

    /// `--machine-only`: `vars.toml` alone.
    ///
    /// Takes the [`MachineVars`] directly and never sees an [`Overlay`], so
    /// "reads only this file" (`docs/state.md`) is a property of the signature.
    pub fn machine_only(machine: &MachineVars) -> Self {
        merge(
            &[Layer {
                source: Source::Machine,
                entries: Entries::Strings(&machine.values),
            }],
            // Nothing in this list is a declaration, so the index is never
            // consulted.
            &Outcomes::default(),
        )
    }

    /// Lookup by the identifier text an expression carries.
    pub fn get(&self, name: &str) -> Option<&Variable> {
        self.values.get(name)
    }
}

/// One layer of a merge: its provenance, and where its entries come from.
///
/// Private, along with [`merge`], because a caller that can compose its own
/// list can spell the layer order — and the point of the named constructors is
/// that no command can.
#[derive(Debug)]
struct Layer<'a> {
    source: Source,
    entries: Entries<'a>,
}

/// The two shapes a layer's entries take.
#[derive(Debug)]
enum Entries<'a> {
    /// A repository's `[vars]`, whose dynamic entries are looked up in the
    /// outcomes index.
    Declared {
        vars: &'a BTreeMap<VarName, VarDecl>,
        /// The declaring remote, or `None` for the leaf — half of a dynamic
        /// declaration's identity.
        remote: Option<&'a ItemId>,
        /// Whether this repository's dynamic declarations may contribute at
        /// all: `IncludedRemote::allow_dynamic_vars` for a remote layer, always
        /// true for the leaf.
        allow_dynamic: bool,
    },
    /// A plain `name = string` map.
    Strings(&'a BTreeMap<VarName, String>),
}

/// Apply the layers in order, low precedence first.
fn merge(layers: &[Layer<'_>], outcomes: &Outcomes<'_>) -> Scope {
    // A list built from literals by the two constructors above, and the
    // precedence tests assert the result directly — so an ordinary test fails
    // first and this is only a faster, more specific way to learn the same
    // thing. That is why it is a `debug_assert!` and `RESOLVED` is not.
    debug_assert!(
        layers
            .windows(2)
            .all(|pair| pair[0].source < pair[1].source),
        "layers must be listed in ascending precedence order"
    );

    let mut values = BTreeMap::new();
    for layer in layers {
        match &layer.entries {
            Entries::Declared {
                vars,
                remote,
                allow_dynamic,
            } => {
                for (name, decl) in *vars {
                    let variable = match decl {
                        VarDecl::Static(value) => Variable {
                            value: Some(value.clone()),
                            source: layer.source,
                            refresh: None,
                        },
                        // Skipped by name, without consulting the outcomes at
                        // all: a remote that may not run commands has no value
                        // here rather than an absent one, so a lower layer's
                        // value stands. This is the other half of the rule the
                        // pipeline implements when it filters the declaration
                        // set — not the same filter written twice.
                        VarDecl::Dynamic(_) if !allow_dynamic => continue,
                        VarDecl::Dynamic(_) => {
                            let outcome = outcomes.get(*remote, name).expect(RESOLVED);
                            Variable {
                                value: outcome.value.clone(),
                                source: layer.source,
                                refresh: Some(outcome.refresh.clone()),
                            }
                        }
                    };
                    bind(&mut values, name, variable);
                }
            }
            Entries::Strings(strings) => {
                for (name, value) in *strings {
                    bind(
                        &mut values,
                        name,
                        Variable {
                            value: Some(value.clone()),
                            source: layer.source,
                            refresh: None,
                        },
                    );
                }
            }
        }
    }
    Scope { values }
}

/// Last writer wins wholesale — value, provenance, and refresh detail together
/// — with [`Refresh::Shadowed`] as the single carve-out.
///
/// A leaf `[vars]` string over a remote's dynamic declaration produces a static
/// row with static provenance and no refresh detail: the remote's command did
/// run, but its result is not effective, and a listing reports what is
/// effective.
///
/// `Shadowed` is sticky because it would otherwise be unobservable by
/// construction — it exists *because* `vars.toml` holds the name, so the
/// machine layer always overwrites it — and it is the only way a listing can
/// say "there is a dynamic declaration here that is deliberately not being
/// run". Stickiness continues past the machine layer: a `BATFILES_VAR_*` or
/// `--var` win keeps it, because "not run" stays true regardless of which
/// override took effect.
fn bind(values: &mut BTreeMap<VarName, Variable>, name: &VarName, mut variable: Variable) {
    if values
        .get(name)
        .is_some_and(|previous| previous.refresh == Some(Refresh::Shadowed))
    {
        variable.refresh = Some(Refresh::Shadowed);
    }
    values.insert(name.clone(), variable);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use crate::dynamic::{Absence, Identity};
    use crate::output::Verbosity;
    use crate::repo::{BatfilesConfig, Capture, CommandSpec, DynamicVar, RemoteState};

    fn name(text: &str) -> VarName {
        VarName::new(text).expect("valid name")
    }

    fn item(text: &str) -> ItemId {
        ItemId::new(text).expect("valid id")
    }

    /// A dynamic declaration. Nothing in this module runs one, so the command
    /// is only there to make the entry a table rather than a string.
    fn dynamic() -> VarDecl {
        VarDecl::Dynamic(DynamicVar {
            command: CommandSpec::Shell("true".to_owned()),
            capture: Capture::Stdout,
            cache: None,
            command_timeout: None,
        })
    }

    fn statics(value: &str) -> VarDecl {
        VarDecl::Static(value.to_owned())
    }

    /// A repository whose `[vars]` is the given declarations.
    fn repo(vars: &[(&str, VarDecl)]) -> Repository {
        Repository {
            root: PathBuf::from("/repo"),
            config: BatfilesConfig {
                vars: vars
                    .iter()
                    .map(|(key, decl)| (name(key), decl.clone()))
                    .collect(),
                ..BatfilesConfig::default()
            },
        }
    }

    fn included(id: &str, allow_dynamic_vars: bool, vars: &[(&str, VarDecl)]) -> IncludedRemote {
        IncludedRemote {
            id: item(id),
            allow_dynamic_vars,
            repo: repo(vars),
            state: RemoteState::Present,
        }
    }

    /// An inclusion of `remote` carrying `vars` as its per-inclusion overrides.
    fn inclusion(remote: &str, vars: &[(&str, &str)]) -> Inclusion {
        Inclusion {
            position: 0,
            id: None,
            remote: item(remote),
            vars: strings(vars),
            label: format!("remote={remote}"),
        }
    }

    fn strings(pairs: &[(&str, &str)]) -> BTreeMap<VarName, String> {
        pairs
            .iter()
            .map(|(key, value)| (name(key), (*value).to_owned()))
            .collect()
    }

    fn machine(pairs: &[(&str, &str)]) -> MachineVars {
        MachineVars {
            values: strings(pairs),
        }
    }

    /// One resolution holding the given outcomes.
    fn resolution(outcomes: Vec<Resolved>) -> Resolution {
        Resolution {
            vars: outcomes,
            changed: false,
        }
    }

    fn outcome(remote: Option<&str>, key: &str, value: Option<&str>, refresh: Refresh) -> Resolved {
        Resolved {
            identity: Identity {
                remote: remote.map(item),
                name: name(key),
            },
            value: value.map(str::to_owned),
            refresh,
        }
    }

    /// An overlay over explicit machine, environment, and command-line layers.
    fn overlay(
        machine: &MachineVars,
        environment: &[(&str, &str)],
        command_line: &[(&str, &str)],
    ) -> Overlay {
        Overlay::build(
            machine,
            &Environment::from_pairs(environment.iter().copied()),
            &command_line
                .iter()
                .map(|(key, value)| (name(key), (*value).to_owned()))
                .collect::<Vec<_>>(),
            &Reporter::new(false, Verbosity::Quiet),
        )
    }

    /// The empty overlay: no machine values, no environment, no `--var`.
    fn no_overlay() -> Overlay {
        Overlay::default()
    }

    /// The binding of `key`, which the test expects to exist.
    fn var<'a>(scope: &'a Scope, key: &str) -> &'a Variable {
        scope.get(key).unwrap_or_else(|| panic!("`{key}` is bound"))
    }

    /// The effective value and provenance of `key`.
    fn effective<'a>(scope: &'a Scope, key: &str) -> (Option<&'a str>, Source) {
        let variable = var(scope, key);
        (variable.value.as_deref(), variable.source)
    }

    #[test]
    fn the_leaf_list_layers_low_to_high() {
        // Asserted as each layer is added, so a transposition anywhere in the
        // list fails rather than only one at an extreme.
        let leaf = repo(&[("profile", statics("repo"))]);
        let outcomes = Outcomes::default();

        assert_eq!(
            effective(&Scope::leaf(&leaf, &outcomes, &no_overlay()), "profile"),
            (Some("repo"), Source::LeafVars)
        );

        let with_machine = overlay(&machine(&[("profile", "machine")]), &[], &[]);
        assert_eq!(
            effective(&Scope::leaf(&leaf, &outcomes, &with_machine), "profile"),
            (Some("machine"), Source::Machine)
        );

        let with_environment = overlay(
            &machine(&[("profile", "machine")]),
            &[("BATFILES_VAR_profile", "environment")],
            &[],
        );
        assert_eq!(
            effective(&Scope::leaf(&leaf, &outcomes, &with_environment), "profile"),
            (Some("environment"), Source::Environment)
        );

        let with_command_line = overlay(
            &machine(&[("profile", "machine")]),
            &[("BATFILES_VAR_profile", "environment")],
            &[("profile", "command-line")],
        );
        assert_eq!(
            effective(
                &Scope::leaf(&leaf, &outcomes, &with_command_line),
                "profile"
            ),
            (Some("command-line"), Source::CommandLine)
        );
    }

    #[test]
    fn the_inclusion_list_layers_low_to_high() {
        let remote = included("core", true, &[("profile", statics("remote"))]);
        let bare = repo(&[]);
        let leaf = repo(&[("profile", statics("leaf"))]);
        let no_overrides = inclusion("core", &[]);
        let overrides = inclusion("core", &[("profile", "inclusion")]);
        let outcomes = Outcomes::default();

        let scope = Scope::inclusion(&remote, &bare, &no_overrides, &outcomes, &no_overlay());
        assert_eq!(
            effective(&scope, "profile"),
            (Some("remote"), Source::RemoteVars)
        );

        let scope = Scope::inclusion(&remote, &leaf, &no_overrides, &outcomes, &no_overlay());
        assert_eq!(
            effective(&scope, "profile"),
            (Some("leaf"), Source::LeafVars)
        );

        let scope = Scope::inclusion(&remote, &leaf, &overrides, &outcomes, &no_overlay());
        assert_eq!(
            effective(&scope, "profile"),
            (Some("inclusion"), Source::InclusionVars)
        );

        let with_machine = overlay(&machine(&[("profile", "machine")]), &[], &[]);
        let scope = Scope::inclusion(&remote, &leaf, &overrides, &outcomes, &with_machine);
        assert_eq!(
            effective(&scope, "profile"),
            (Some("machine"), Source::Machine)
        );

        let with_environment = overlay(
            &machine(&[("profile", "machine")]),
            &[("BATFILES_VAR_profile", "environment")],
            &[],
        );
        let scope = Scope::inclusion(&remote, &leaf, &overrides, &outcomes, &with_environment);
        assert_eq!(
            effective(&scope, "profile"),
            (Some("environment"), Source::Environment)
        );

        let with_command_line = overlay(
            &machine(&[("profile", "machine")]),
            &[("BATFILES_VAR_profile", "environment")],
            &[("profile", "command-line")],
        );
        let scope = Scope::inclusion(&remote, &leaf, &overrides, &outcomes, &with_command_line);
        assert_eq!(
            effective(&scope, "profile"),
            (Some("command-line"), Source::CommandLine)
        );
    }

    #[test]
    fn sources_are_declared_in_precedence_order() {
        // Written in the precedence order `docs/environment.md` specifies,
        // independently of how the enum happens to be declared: the assertion
        // is that the derived `Ord` — which `merge` debug-asserts on — agrees
        // with the spec.
        let ascending = [
            Source::RemoteVars,
            Source::LeafVars,
            Source::InclusionVars,
            Source::Machine,
            Source::Environment,
            Source::CommandLine,
        ];
        assert!(
            ascending.windows(2).all(|pair| pair[0] < pair[1]),
            "{ascending:?} is not in ascending precedence order"
        );
    }

    #[test]
    fn a_repeated_command_line_key_takes_its_last_value() {
        let leaf = repo(&[]);
        let overlay = overlay(
            &machine(&[]),
            &[],
            &[("profile", "work"), ("profile", "home")],
        );

        assert_eq!(
            effective(
                &Scope::leaf(&leaf, &Outcomes::default(), &overlay),
                "profile"
            ),
            (Some("home"), Source::CommandLine)
        );
    }

    #[test]
    fn an_empty_command_line_leaves_the_environment_winning() {
        // What `vars list` passes: the command has no `--var` option, so the
        // top layer is simply empty rather than specially suppressed.
        let leaf = repo(&[]);
        let overlay = overlay(
            &machine(&[("profile", "machine")]),
            &[("BATFILES_VAR_profile", "environment")],
            &[],
        );

        assert_eq!(
            effective(
                &Scope::leaf(&leaf, &Outcomes::default(), &overlay),
                "profile"
            ),
            (Some("environment"), Source::Environment)
        );
    }

    #[test]
    fn machine_only_is_the_file_and_nothing_else() {
        let scope = Scope::machine_only(&machine(&[("editor", "nvim"), ("profile", "work")]));

        assert_eq!(scope.values.len(), 2);
        assert_eq!(effective(&scope, "editor"), (Some("nvim"), Source::Machine));
        assert_eq!(
            effective(&scope, "profile"),
            (Some("work"), Source::Machine)
        );
        // The environment is not a parameter, so a `BATFILES_VAR_*` cannot
        // reach this scope even when one is set.
        assert!(Scope::machine_only(&machine(&[])).values.is_empty());
    }

    #[test]
    fn a_static_declaration_lands_with_its_layers_provenance_and_no_detail() {
        let leaf = repo(&[("profile", statics("work")), ("empty", statics(""))]);
        let scope = Scope::leaf(&leaf, &Outcomes::default(), &no_overlay());

        assert_eq!(
            var(&scope, "profile"),
            &Variable {
                value: Some("work".to_owned()),
                source: Source::LeafVars,
                refresh: None,
            }
        );
        assert_eq!(
            var(&scope, "empty").value.as_deref(),
            Some(""),
            "the empty string is a value like any other"
        );
    }

    #[test]
    fn a_dynamic_declaration_enters_through_its_outcome() {
        let leaf = repo(&[("email", dynamic())]);
        let resolutions = [resolution(vec![outcome(
            None,
            "email",
            Some("me@example.com"),
            Refresh::Refreshed,
        )])];
        let outcomes = Outcomes::index(&resolutions);

        assert_eq!(
            var(&Scope::leaf(&leaf, &outcomes, &no_overlay()), "email"),
            &Variable {
                value: Some("me@example.com".to_owned()),
                source: Source::LeafVars,
                refresh: Some(Refresh::Refreshed),
            }
        );
    }

    #[test]
    fn a_disallowed_remote_contributes_no_dynamic_declaration_and_shadows_nothing() {
        // The outcomes index deliberately *contains* an entry for the name, so
        // the skip is proven to be by the flag rather than by absence — the
        // half a single "no outcome ⇒ contributes nothing" rule gets wrong.
        let remote = included("core", false, &[("email", dynamic())]);
        let leaf = repo(&[("email", statics("leaf@example.com"))]);
        let resolutions = [resolution(vec![outcome(
            Some("core"),
            "email",
            Some("remote@example.com"),
            Refresh::Refreshed,
        )])];
        let outcomes = Outcomes::index(&resolutions);

        let scope = Scope::inclusion(
            &remote,
            &leaf,
            &inclusion("core", &[]),
            &outcomes,
            &no_overlay(),
        );
        assert_eq!(
            effective(&scope, "email"),
            (Some("leaf@example.com"), Source::LeafVars),
            "the lower layer's value stands"
        );

        // And with nothing beneath it, the name has no row at all.
        let scope = Scope::inclusion(
            &remote,
            &repo(&[]),
            &inclusion("core", &[]),
            &outcomes,
            &no_overlay(),
        );
        assert!(scope.get("email").is_none());
    }

    #[test]
    #[should_panic(expected = "a declaration that reaches a scope has a resolved outcome")]
    fn a_missing_outcome_for_an_allowed_declaration_is_a_bug() {
        // Paired with the test above, this is the whole rule: the same absent
        // outcome is ordinary when the flag says so, and a bug report when it
        // does not.
        let leaf = repo(&[("email", dynamic())]);
        let _ = Scope::leaf(&leaf, &Outcomes::default(), &no_overlay());
    }

    #[test]
    fn a_resolved_declaration_with_no_value_still_overrides_the_layer_beneath() {
        // Not to be confused with the panic above: this is a resolved outcome
        // that produced nothing, which is ordinary, and it is the reason
        // `value` is an `Option` at all.
        let remote = included("core", true, &[("email", statics("remote@example.com"))]);
        let leaf = repo(&[("email", dynamic())]);
        let resolutions = [resolution(vec![outcome(
            None,
            "email",
            None,
            Refresh::Missing(Absence::CommandFailed),
        )])];
        let outcomes = Outcomes::index(&resolutions);

        let scope = Scope::inclusion(
            &remote,
            &leaf,
            &inclusion("core", &[]),
            &outcomes,
            &no_overlay(),
        );
        assert_eq!(
            var(&scope, "email"),
            &Variable {
                value: None,
                source: Source::LeafVars,
                refresh: Some(Refresh::Missing(Absence::CommandFailed)),
            }
        );
    }

    #[test]
    fn last_writer_wins_wholesale() {
        let remote = included("core", true, &[("email", dynamic())]);
        let leaf = repo(&[("email", statics("leaf@example.com"))]);
        let resolutions = [resolution(vec![outcome(
            Some("core"),
            "email",
            Some("remote@example.com"),
            Refresh::Refreshed,
        )])];
        let outcomes = Outcomes::index(&resolutions);

        let scope = Scope::inclusion(
            &remote,
            &leaf,
            &inclusion("core", &[]),
            &outcomes,
            &no_overlay(),
        );
        assert_eq!(
            var(&scope, "email"),
            &Variable {
                value: Some("leaf@example.com".to_owned()),
                source: Source::LeafVars,
                refresh: None,
            },
            "the remote's command ran, but the listing reports what is effective"
        );
    }

    #[test]
    fn a_shadowed_refresh_detail_survives_every_override() {
        // The one carve-out to last-writer-wins, and the test a later "make the
        // merge uniform" edit would break. Without it the variant would be
        // unobservable by construction: it exists *because* `vars.toml` holds
        // the name.
        let leaf = repo(&[("email", dynamic())]);
        let resolutions = [resolution(vec![outcome(
            None,
            "email",
            Some("cached@example.com"),
            Refresh::Shadowed,
        )])];
        let outcomes = Outcomes::index(&resolutions);

        let with_machine = overlay(&machine(&[("email", "machine@example.com")]), &[], &[]);
        assert_eq!(
            var(&Scope::leaf(&leaf, &outcomes, &with_machine), "email"),
            &Variable {
                value: Some("machine@example.com".to_owned()),
                source: Source::Machine,
                refresh: Some(Refresh::Shadowed),
            }
        );

        let and_environment = overlay(
            &machine(&[("email", "machine@example.com")]),
            &[("BATFILES_VAR_email", "env@example.com")],
            &[],
        );
        assert_eq!(
            var(&Scope::leaf(&leaf, &outcomes, &and_environment), "email"),
            &Variable {
                value: Some("env@example.com".to_owned()),
                source: Source::Environment,
                refresh: Some(Refresh::Shadowed),
            },
            "`not run` stays true regardless of which override took effect"
        );
    }

    #[test]
    fn an_unmaterialized_remote_contributes_nothing() {
        let remote = IncludedRemote {
            state: RemoteState::NotMaterialized,
            ..included("core", true, &[])
        };
        let scope = Scope::inclusion(
            &remote,
            &repo(&[]),
            &inclusion("core", &[]),
            &Outcomes::default(),
            &no_overlay(),
        );

        assert!(scope.values.is_empty());
    }

    #[test]
    fn the_outcomes_index_keeps_the_leaf_and_remote_namespaces_apart() {
        let remote = included("core", true, &[("email", dynamic())]);
        let leaf = repo(&[("email", dynamic())]);
        let resolutions = [
            resolution(vec![outcome(
                None,
                "email",
                Some("leaf@example.com"),
                Refresh::Refreshed,
            )]),
            resolution(vec![outcome(
                Some("core"),
                "email",
                Some("remote@example.com"),
                Refresh::Assumed,
            )]),
        ];
        let outcomes = Outcomes::index(&resolutions);

        // The remote's declaration is beaten by the leaf's, so its own outcome
        // is read through a scope where the leaf declares nothing.
        let scope = Scope::inclusion(
            &remote,
            &repo(&[]),
            &inclusion("core", &[]),
            &outcomes,
            &no_overlay(),
        );
        assert_eq!(
            effective(&scope, "email"),
            (Some("remote@example.com"), Source::RemoteVars)
        );

        let scope = Scope::inclusion(
            &remote,
            &leaf,
            &inclusion("core", &[]),
            &outcomes,
            &no_overlay(),
        );
        assert_eq!(
            effective(&scope, "email"),
            (Some("leaf@example.com"), Source::LeafVars)
        );
    }

    #[test]
    fn a_lookup_takes_the_identifier_text_an_expression_carries() {
        let leaf = repo(&[("profile", statics("work"))]);
        let scope = Scope::leaf(&leaf, &Outcomes::default(), &no_overlay());

        assert_eq!(
            scope.get("profile").and_then(|v| v.value.as_deref()),
            Some("work")
        );
        assert!(scope.get("editor").is_none());
    }

    #[test]
    fn an_invalid_environment_suffix_is_dropped_and_the_rest_survives() {
        // The emission cannot be asserted — `Reporter::warn` has no seam — so
        // the text is asserted through the pure line and the drop through the
        // built overlay.
        assert_eq!(
            ignored("1up", VarNameError::Invalid),
            "ignoring `BATFILES_VAR_1up`: a variable name must start with a letter or \
             underscore, followed by letters, digits, or underscores"
        );

        let leaf = repo(&[]);
        let overlay = overlay(
            &machine(&[]),
            &[
                ("BATFILES_VAR_1up", "dropped"),
                ("BATFILES_VAR_profile", "work"),
            ],
            &[],
        );
        let scope = Scope::leaf(&leaf, &Outcomes::default(), &overlay);

        assert_eq!(scope.values.len(), 1);
        assert_eq!(
            effective(&scope, "profile"),
            (Some("work"), Source::Environment)
        );
    }

    #[test]
    fn a_reserved_environment_suffix_takes_the_same_path() {
        assert!(ignored("env", VarNameError::Reserved).contains("`BATFILES_VAR_env`"));

        let overlay = overlay(&machine(&[]), &[("BATFILES_VAR_env", "dropped")], &[]);
        let scope = Scope::leaf(&repo(&[]), &Outcomes::default(), &overlay);

        assert!(scope.values.is_empty());
    }

    #[test]
    fn an_empty_environment_value_is_kept_and_a_bare_prefix_is_not() {
        let overlay = overlay(
            &machine(&[]),
            &[("BATFILES_VAR_PROFILE", ""), ("BATFILES_VAR_", "orphan")],
            &[],
        );
        let scope = Scope::leaf(&repo(&[]), &Outcomes::default(), &overlay);

        assert_eq!(scope.values.len(), 1);
        assert_eq!(
            effective(&scope, "PROFILE"),
            (Some(""), Source::Environment)
        );
    }
}
