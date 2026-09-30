//! Compose an opened inclusion's variables and filtered records during run assembly.
//! [`crate::inclusion`] handles identity, admission, and manifest loading.

use std::collections::BTreeMap;

use crate::condition::Exclusion;
use crate::inclusion::Inclusion;
use crate::item::{ItemId, ItemIdList, ItemKind};
use crate::manifest::Manifest;
use crate::manifest::action::{Action, IncludeRemoteAction};
use crate::manifest::vars::VarSpec;
use crate::output::Reporter;
use crate::var::VarName;

/// Variables and records contributed by an opened inclusion.
pub(super) struct InclusionContents {
    /// The included manifest's `[vars]`: the layer below the leaf's in the
    /// inclusion's [scope](crate::var_set::VarSet::with_inclusion). Empty when
    /// the remote declares none.
    pub vars: BTreeMap<VarName, VarSpec>,
    pub records: Vec<IncludedRecord>,
}

/// An included manifest record and its filter result. Retain excluded records for skip matching
/// and reporting.
pub(super) struct IncludedRecord {
    /// One-based position in the included manifest.
    pub number: usize,
    pub action: Action,
    /// Whether the inclusion filters select this record; no command bypasses these filters.
    pub included_by_filter: bool,
}

/// Compose `included` using `inclusion`'s identity and `declaration`'s filters. Return its
/// variables and records with filter results.
///
/// Drop nested inclusions and warn about them, ignored remote declarations, and unmatched
/// filter names.
pub(super) fn compose(
    inclusion: &Inclusion,
    declaration: &IncludeRemoteAction,
    included: Manifest,
    reporter: &Reporter,
) -> InclusionContents {
    let label = inclusion.label();
    let remote = inclusion.remote();
    report_ignored_remotes(&included, label, remote, reporter);
    let by = inclusion.contributor();
    let filter = Filter::of(declaration);
    let mut records = Vec::with_capacity(included.actions.len());
    for (index, record) in included.actions.into_iter().enumerate() {
        // The declared position, not recomputed after filtering: a record
        // without an `id` is named by where it was written.
        let number = index + 1;
        // Drop nested inclusions before filtering so their IDs cannot satisfy a filter.
        if let Action::IncludeRemote(_) = record {
            reporter.warn(&format!(
                "not included: {}; an included repository does not reach \
                 further repositories",
                record.describe(number, by)
            ));
            continue;
        }
        let included_by_filter = filter.selects(&record);
        records.push(IncludedRecord {
            number,
            action: record,
            included_by_filter,
        });
    }
    filter.warn_unmatched(&records, label, remote, reporter);
    InclusionContents {
        vars: included.vars,
        records,
    }
}

/// The exclusion for a record `inclusion`'s filters left out, naming the
/// inclusion.
pub(super) fn not_selected(inclusion: &Inclusion) -> Exclusion {
    Exclusion::Deliberate(format!("not selected by {}", inclusion.label()))
}

/// Warn once per inclusion about ignored remote declarations, listing their IDs.
fn report_ignored_remotes(included: &Manifest, label: &str, remote: &ItemId, reporter: &Reporter) {
    if included.remotes.is_empty() {
        return;
    }
    let names = included
        .remotes
        .keys()
        .map(|id| format!("`{id}`"))
        .collect::<Vec<_>>()
        .join(", ");
    reporter.warn(&format!(
        "{label}: ignoring the remotes `{remote}` declares ({names}); an included \
         repository does not reach further repositories, so nothing materializes \
         them and no included action can name one"
    ));
}

/// Action and group filters for an inclusion. Absent filters select everything; an empty
/// allow-list selects nothing.
///
/// Match declared IDs and groups. An unnamed record fails an allow-list and passes a deny-list
/// for that field. See [filter rules](../../docs/repoformat.md#selecting-part-of-a-remote).
struct Filter {
    install_actions: Option<ItemIdList>,
    install_groups: Option<ItemIdList>,
    exclude_actions: Option<ItemIdList>,
    exclude_groups: Option<ItemIdList>,
}

impl Filter {
    fn of(action: &IncludeRemoteAction) -> Self {
        Self {
            install_actions: action.install_actions.clone(),
            install_groups: action.install_groups.clone(),
            exclude_actions: action.exclude_actions.clone(),
            exclude_groups: action.exclude_groups.clone(),
        }
    }

    /// Whether this inclusion takes the record.
    fn selects(&self, action: &Action) -> bool {
        let (id, group) = (action.id(), action.group());
        let names = |list: Option<&ItemIdList>, item: Option<&ItemId>| {
            list.map(|list| item.is_some_and(|item| list.contains(item)))
        };
        let allowed = names(self.install_actions.as_ref(), id)
            .or_else(|| names(self.install_groups.as_ref(), group))
            .unwrap_or(true);
        allowed
            && !names(self.exclude_groups.as_ref(), group).unwrap_or(false)
            && !names(self.exclude_actions.as_ref(), id).unwrap_or(false)
    }

    /// Warn once per filter name that matches no declared action or group.
    fn warn_unmatched(
        &self,
        included: &[IncludedRecord],
        label: &str,
        remote: &ItemId,
        reporter: &Reporter,
    ) {
        for (field, name, kind) in self.unmatched(included) {
            reporter.warn(&format!(
                "{label}: {field} `{name}` matched no {kind} in remote `{remote}`"
            ));
        }
    }

    /// Every unmatched filter name, as (field, name, kind). Action filters are
    /// checked against declared IDs, group filters against declared groups.
    fn unmatched(&self, included: &[IncludedRecord]) -> Vec<(&'static str, &ItemId, ItemKind)> {
        let declared = |of: fn(&Action) -> Option<&ItemId>| -> Vec<&ItemId> {
            included.iter().filter_map(|it| of(&it.action)).collect()
        };
        let (actions, groups) = (declared(Action::id), declared(Action::group));
        let mut unmatched = Vec::new();
        for (field, list, present, kind) in [
            (
                "install-actions",
                &self.install_actions,
                &actions,
                ItemKind::Action,
            ),
            (
                "exclude-actions",
                &self.exclude_actions,
                &actions,
                ItemKind::Action,
            ),
            (
                "install-groups",
                &self.install_groups,
                &groups,
                ItemKind::Group,
            ),
            (
                "exclude-groups",
                &self.exclude_groups,
                &groups,
                ItemKind::Group,
            ),
        ] {
            for name in list.as_ref().map(ItemIdList::as_slice).unwrap_or_default() {
                if !present.contains(&name) {
                    unmatched.push((field, name, kind));
                }
            }
        }
        unmatched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse an inclusion with the supplied filters.
    fn inclusion(filters: &str) -> IncludeRemoteAction {
        toml::from_str(&format!("id = \"corp\"\nremote = \"corporate\"\n{filters}"))
            .expect("the record should parse")
    }

    /// Build an included action with optional ID and group.
    fn action(id: Option<&str>, group: Option<&str>) -> Action {
        let named = |field: &str, value: Option<&str>| match value {
            Some(value) => format!("{field} = \"{value}\"\n"),
            None => String::new(),
        };
        toml::from_str(&format!(
            "type = \"create-dir\"\ndest = \"~/x\"\n{}{}",
            named("id", id),
            named("group", group)
        ))
        .expect("the record should parse")
    }

    /// Return the fixture record labels selected by `filters`.
    fn selected(filters: &str) -> Vec<&'static str> {
        let declared = [
            ("zshrc", Some("shell")),
            ("p10k", Some("prompt")),
            ("seeds", None),
        ];
        let record = inclusion(filters);
        let filter = Filter::of(&record);
        let mut taken = Vec::new();
        for (id, group) in declared {
            if filter.selects(&action(Some(id), group)) {
                taken.push(id);
            }
        }
        if filter.selects(&action(None, Some("shell"))) {
            taken.push("<unnamed in shell>");
        }
        if filter.selects(&action(None, None)) {
            taken.push("<unnamed>");
        }
        taken
    }

    #[test]
    fn an_inclusion_writing_no_filter_takes_everything() {
        assert_eq!(
            selected(""),
            ["zshrc", "p10k", "seeds", "<unnamed in shell>", "<unnamed>"]
        );
    }

    #[test]
    fn an_allow_list_takes_only_what_it_names() {
        assert_eq!(selected("install-actions = [\"zshrc\"]"), ["zshrc"]);
        assert_eq!(
            selected("install-groups = [\"shell\"]"),
            ["zshrc", "<unnamed in shell>"]
        );
    }

    #[test]
    fn a_deny_list_leaves_out_only_what_it_names() {
        assert_eq!(
            selected("exclude-actions = [\"p10k\"]"),
            ["zshrc", "seeds", "<unnamed in shell>", "<unnamed>"]
        );
        assert_eq!(
            selected("exclude-groups = [\"shell\"]"),
            ["p10k", "seeds", "<unnamed>"]
        );
    }

    #[test]
    fn a_record_a_filter_cannot_name_is_taken_only_by_a_deny_list() {
        assert!(!selected("install-actions = [\"zshrc\"]").contains(&"<unnamed>"));
        assert!(!selected("install-groups = [\"shell\"]").contains(&"seeds"));
        assert!(selected("exclude-actions = [\"zshrc\"]").contains(&"<unnamed>"));
        assert!(selected("exclude-groups = [\"shell\"]").contains(&"seeds"));
    }

    #[test]
    fn excluded_actions_narrow_what_a_group_filter_selected() {
        assert_eq!(
            selected("install-groups = [\"shell\"]\nexclude-actions = [\"zshrc\"]"),
            ["<unnamed in shell>"]
        );
        assert_eq!(
            selected("exclude-groups = [\"prompt\"]\nexclude-actions = [\"zshrc\"]"),
            ["seeds", "<unnamed in shell>", "<unnamed>"]
        );
    }

    #[test]
    fn an_empty_list_is_not_an_absent_one() {
        assert!(selected("install-actions = []").is_empty());
        assert!(selected("install-groups = []").is_empty());
        assert_eq!(selected("exclude-actions = []"), selected(""));
        assert_eq!(selected("exclude-groups = []"), selected(""));
    }

    /// Return unmatched filter names formatted as diagnostic labels.
    fn unmatched(filters: &str) -> Vec<String> {
        let included = ["zshrc", "p10k"]
            .into_iter()
            .enumerate()
            .map(|(index, id)| IncludedRecord {
                number: index + 1,
                action: action(Some(id), Some("shell")),
                included_by_filter: true,
            })
            .collect::<Vec<_>>();
        let record = inclusion(filters);
        Filter::of(&record)
            .unmatched(&included)
            .into_iter()
            .map(|(field, name, kind)| format!("{field} `{name}` matched no {kind}"))
            .collect()
    }

    #[test]
    fn a_filter_name_the_manifest_does_not_declare_is_reported_once() {
        assert!(unmatched("install-actions = [\"zshrc\", \"p10k\"]").is_empty());
        assert_eq!(
            unmatched("install-actions = [\"zshrc\", \"seeds\"]"),
            ["install-actions `seeds` matched no action"]
        );
        assert_eq!(
            unmatched("exclude-groups = [\"prompt\", \"gui\"]"),
            [
                "exclude-groups `prompt` matched no group",
                "exclude-groups `gui` matched no group"
            ]
        );
    }

    #[test]
    fn each_filter_is_matched_against_the_namespace_it_names() {
        assert!(unmatched("install-groups = [\"shell\"]").is_empty());
        assert_eq!(
            unmatched("install-actions = [\"shell\"]"),
            ["install-actions `shell` matched no action"]
        );
        assert_eq!(
            unmatched("exclude-groups = [\"zshrc\"]"),
            ["exclude-groups `zshrc` matched no group"]
        );
    }
}
