//! Bootstrap disabled state for `clone` and `sync --bootstrap`. Apply eligible `[default-disabled]` entries only when
//! `disabled.toml` is absent, then apply environment and CLI decisions in [precedence
//! order](../../docs/environment.md#bootstrap-enable-and-disable-lists).

use crate::cli::BootstrapOptions;
use crate::env::Environment;
use crate::error::Error;
use crate::filesystem::mode::RunMode;
use crate::filesystem::paths;
use crate::location::StateRoots;
use crate::manifest::condition::{Bindings, Gate};
use crate::manifest::default_disabled::DefaultDisabled;
use crate::manifest::disabled::{Change, DisabledItems};
use crate::manifest::item::{ItemAddress, ItemKind};
use crate::output::Reporter;

/// Warning suffix for a candidate whose condition cannot be evaluated.
const NOT_DISABLED: &str = "it is left enabled";

/// The origin reported for a candidate the repository declared.
const DECLARED: &str = "default-disabled";

/// Explicit enable/disable decisions for a bootstrap, in precedence order.
#[derive(Debug)]
pub(crate) struct BootstrapDecisions {
    decisions: Vec<Decision>,
}

/// An enable/disable decision with its item kind, address, and origin.
#[derive(Debug)]
struct Decision {
    kind: ItemKind,
    change: Change,
    origin: &'static str,
    name: ItemAddress,
}

impl BootstrapDecisions {
    /// Read bootstrap environment variables and CLI options in precedence order. Invalid option
    /// values fail; invalid environment values warn and are dropped.
    pub fn read(
        options: &BootstrapOptions,
        env: &Environment,
        reporter: &Reporter,
    ) -> Result<Self, Error> {
        use Change::{Disable, Enable};
        use ItemKind::{Action, Group};

        let mut decisions = Vec::new();
        // Environment precedes CLI; within each, enable overrides disable.
        for (kind, change, variable) in [
            (Action, Disable, "BATFILES_DISABLE_ACTIONS"),
            (Group, Disable, "BATFILES_DISABLE_GROUPS"),
            (Action, Enable, "BATFILES_ENABLE_ACTIONS"),
            (Group, Enable, "BATFILES_ENABLE_GROUPS"),
        ] {
            for value in env.list(variable) {
                match ItemAddress::try_from(value) {
                    Ok(name) => decisions.push(Decision {
                        kind,
                        change,
                        origin: variable,
                        name,
                    }),
                    Err(error) => reporter.warn(&format!("{variable}: {error}")),
                }
            }
        }
        for (kind, change, option, values) in [
            (
                Action,
                Disable,
                "--disable-action",
                &options.disable_actions,
            ),
            (Group, Disable, "--disable-group", &options.disable_groups),
            (Action, Enable, "--enable-action", &options.enable_actions),
            (Group, Enable, "--enable-group", &options.enable_groups),
        ] {
            for value in values {
                decisions.push(Decision {
                    kind,
                    change,
                    origin: option,
                    name: ItemAddress::try_from(value.clone())?,
                });
            }
        }
        Ok(Self { decisions })
    }

    /// Apply bootstrap decisions, report each one, and return the disabled state the run
    /// selects with. Save it only when it changes and `mode` performs.
    pub fn adopt(
        &self,
        candidates: &DefaultDisabled,
        bindings: &Bindings<'_>,
        roots: &StateRoots,
        mode: RunMode,
        reporter: &Reporter,
    ) -> Result<DisabledItems, Error> {
        let path = roots.disabled_path();
        // An existing empty document still means bootstrap defaults were already considered.
        let offered = !paths::occupied(&path)?;
        let mut disabled = DisabledItems::load(&path)?;

        let mut lines = Vec::new();
        let mut changed = false;

        if offered {
            for (kind, name, gate) in entries(candidates) {
                if let Some(exclusion) =
                    gate.and_then(|gate| gate.exclusion(bindings, Some(NOT_DISABLED)))
                {
                    exclusion.report_heading(reporter, &heading(kind, name));
                    continue;
                }
                let moved = Change::Disable.apply_to(disabled.list_mut(kind), name);
                changed |= moved;
                lines.push(line(kind, Change::Disable, name, moved, DECLARED, mode));
            }
        }

        for decision in &self.decisions {
            let moved = decision
                .change
                .apply_to(disabled.list_mut(decision.kind), &decision.name);
            changed |= moved;
            lines.push(line(
                decision.kind,
                decision.change,
                &decision.name,
                moved,
                decision.origin,
                mode,
            ));
        }

        if changed && mode == RunMode::Perform {
            disabled.save(&path)?;
        }
        for line in &lines {
            reporter.info(line);
        }
        Ok(disabled)
    }
}

/// Iterate declared candidates and their conditions: actions first, then groups.
fn entries(
    candidates: &DefaultDisabled,
) -> impl Iterator<Item = (ItemKind, &ItemAddress, Option<Gate<'_>>)> {
    let actions = candidates
        .actions
        .iter()
        .map(|entry| (ItemKind::Action, &entry.id, entry.gate()));
    let groups = candidates
        .groups
        .iter()
        .map(|entry| (ItemKind::Group, &entry.group, entry.gate()));
    actions.chain(groups)
}

/// Format a candidate's kind and address for diagnostics.
fn heading(kind: ItemKind, name: &ItemAddress) -> String {
    format!("candidate {kind} `{name}`")
}

/// Format a decision with its origin and outcome, as `mode` would carry it out.
fn line(
    kind: ItemKind,
    change: Change,
    name: &ItemAddress,
    moved: bool,
    origin: &'static str,
    mode: RunMode,
) -> String {
    let outcome = match (mode, change, moved) {
        (RunMode::DryRun, Change::Disable, true) => format!("would disable {kind} `{name}`"),
        (RunMode::DryRun, Change::Enable, true) => {
            format!("would enable {kind} `{name}`, which is disabled")
        }
        _ => change.outcome(kind, name, moved),
    };
    format!("{origin}: {outcome}")
}

#[cfg(test)]
mod tests;
