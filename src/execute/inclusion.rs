//! What an opened `include-remote` contributes to a run: the records of the
//! manifest it read, with its filters' verdict on each.
//!
//! Composition happens while the run's list is assembled; the record itself
//! installs nothing. Which inclusion it is, whether it is opened, and reading
//! its manifest are [`inclusion`](crate::inclusion)'s.

use std::collections::BTreeMap;

use crate::condition::Exclusion;
use crate::inclusion::Inclusion;
use crate::item::{ItemId, ItemIdList};
use crate::manifest::Manifest;
use crate::manifest::action::{Action, IncludeRemoteAction};
use crate::manifest::vars::VarSpec;
use crate::output::Reporter;
use crate::var::VarName;

/// What one `include-remote` read out of the manifest it opened.
pub(super) struct InclusionContents {
    /// The included manifest's `[vars]`: the layer below the leaf's in the
    /// inclusion's [scope](crate::var_set::VarSet::with_inclusion). Empty when
    /// the remote declares none.
    pub vars: BTreeMap<VarName, VarSpec>,
    pub actions: Vec<IncludedAction>,
}

/// One record of an included manifest, with the inclusion filters' verdict.
///
/// Records the filters leave out are kept, so a skip naming one still matches
/// and the run can report why it was passed over.
pub(super) struct IncludedAction {
    /// One-based position in the included manifest.
    pub number: usize,
    pub action: Action,
    /// Whether the inclusion's [filters](Filter) take this record. Filters state
    /// what the leaf repository composed, not what this machine leaves out, so
    /// no command waives them.
    pub included_by_filter: bool,
}

/// What `inclusion`, written as `declaration`, contributes from `included`,
/// the manifest it opened: its `[vars]`, and its records with the filters'
/// verdict on each.
///
/// Warns about the manifest's own `[remotes]`, any inclusion it declares, which
/// is dropped, and each filter name it does not declare. Contents with no
/// records mean the manifest contributed nothing; only then can a qualified
/// name be checked against it.
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
        // Inclusion is one level deep: a nested inclusion is dropped with a
        // warning, before the filters, so its `id` cannot satisfy one.
        if let Action::IncludeRemote(_) = record {
            reporter.warn(&format!(
                "not included: {}; an included repository does not reach \
                 further repositories",
                record.describe(number, by)
            ));
            continue;
        }
        let included_by_filter = filter.selects(&record);
        records.push(IncludedAction {
            number,
            action: record,
            included_by_filter,
        });
    }
    filter.warn_unmatched(&records, label, remote, reporter);
    InclusionContents {
        // The remote's `[vars]` apply whichever records the filters took.
        vars: included.vars,
        actions: records,
    }
}

/// The exclusion for a record `inclusion`'s filters left out, naming the
/// inclusion.
pub(super) fn not_selected(inclusion: &Inclusion) -> Exclusion {
    Exclusion::Expected(format!("not selected by {}", inclusion.label()))
}

/// Warn, once per inclusion, that the included manifest's `[remotes]` are
/// ignored, naming each; silent when it declares none. The rule is
/// [`docs/repoformat.md`](../../docs/repoformat.md#an-included-manifests-own-remotes)'s.
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

/// Which of a remote's actions one inclusion takes.
///
/// Each field is an `Option`: with none written the inclusion takes
/// everything, while an empty allow-list takes nothing. Valid combinations are
/// [`check_inclusion_filters`](crate::manifest::check)'s rule; what each selects
/// is [`docs/repoformat.md`](../../docs/repoformat.md#selecting-part-of-a-remote)'s.
///
/// Filters match each record's declared `id` and `group`. A record without an
/// `id` is never named by an action filter: an allow-list leaves it out and a
/// deny-list takes it. The same holds for `group`.
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
        // Allow, then deny: the only order in which `exclude-actions` narrows a
        // group filter. At most one allow-list is written.
        let allowed = names(self.install_actions.as_ref(), id)
            .or_else(|| names(self.install_groups.as_ref(), group))
            .unwrap_or(true);
        allowed
            && !names(self.exclude_groups.as_ref(), group).unwrap_or(false)
            && !names(self.exclude_actions.as_ref(), id).unwrap_or(false)
    }

    /// Warn once per filter name that nothing in the included manifest answers
    /// to. Not a failure: the remote may be older than the leaf expects.
    fn warn_unmatched(
        &self,
        included: &[IncludedAction],
        label: &str,
        remote: &ItemId,
        reporter: &Reporter,
    ) {
        for (field, name, noun) in self.unmatched(included) {
            reporter.warn(&format!(
                "{label}: {field} `{name}` matched no {noun} in remote `{remote}`"
            ));
        }
    }

    /// Every unmatched filter name, as (field, name, noun). Action filters are
    /// checked against declared IDs, group filters against declared groups.
    fn unmatched(&self, included: &[IncludedAction]) -> Vec<(&'static str, &ItemId, &'static str)> {
        let declared = |of: fn(&Action) -> Option<&ItemId>| -> Vec<&ItemId> {
            included.iter().filter_map(|it| of(&it.action)).collect()
        };
        let (actions, groups) = (declared(Action::id), declared(Action::group));
        let mut unmatched = Vec::new();
        for (field, list, present, noun) in [
            ("install-actions", &self.install_actions, &actions, "action"),
            ("exclude-actions", &self.exclude_actions, &actions, "action"),
            ("install-groups", &self.install_groups, &groups, "group"),
            ("exclude-groups", &self.exclude_groups, &groups, "group"),
        ] {
            for name in list.as_ref().map(ItemIdList::as_slice).unwrap_or_default() {
                if !present.contains(&name) {
                    unmatched.push((field, name, noun));
                }
            }
        }
        unmatched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An inclusion carrying the filters named, read the way a manifest hands
    /// one over. The combinations that reach this point are the ones
    /// `check_inclusion_filters` accepts.
    fn inclusion(filters: &str) -> IncludeRemoteAction {
        toml::from_str(&format!("id = \"corp\"\nremote = \"corporate\"\n{filters}"))
            .expect("the record should parse")
    }

    /// One of the included manifest's records, by the two things a filter looks
    /// at. `None` is a record written without that field.
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

    /// Which of the `corporate` fixture's shape of manifest an inclusion takes:
    /// three named records in two groups, one record in no group, and one
    /// record with neither.
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
        // `seeds` is the same case one field over: a record written without a
        // `group` is named by neither group filter.
        assert!(!selected("install-actions = [\"zshrc\"]").contains(&"<unnamed>"));
        assert!(!selected("install-groups = [\"shell\"]").contains(&"seeds"));
        assert!(selected("exclude-actions = [\"zshrc\"]").contains(&"<unnamed>"));
        assert!(selected("exclude-groups = [\"shell\"]").contains(&"seeds"));
    }

    #[test]
    fn excluded_actions_narrow_what_a_group_filter_selected() {
        // The one combination the two halves are written for: take a group,
        // less one of its members.
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
        // The distinction the `Option` around each field exists for.
        assert!(selected("install-actions = []").is_empty());
        assert!(selected("install-groups = []").is_empty());
        assert_eq!(selected("exclude-actions = []"), selected(""));
        assert_eq!(selected("exclude-groups = []"), selected(""));
    }

    /// The names the filters in `filters` matched nothing, as the warning
    /// names them.
    fn unmatched(filters: &str) -> Vec<String> {
        let included = ["zshrc", "p10k"]
            .into_iter()
            .enumerate()
            .map(|(index, id)| IncludedAction {
                number: index + 1,
                action: action(Some(id), Some("shell")),
                included_by_filter: true,
            })
            .collect::<Vec<_>>();
        let record = inclusion(filters);
        Filter::of(&record)
            .unmatched(&included)
            .into_iter()
            .map(|(field, name, noun)| format!("{field} `{name}` matched no {noun}"))
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
        // An action ID is not a group name: a filter naming `shell` is
        // satisfied by the group and not by any record's `id`, and the other
        // way round for `zshrc`.
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
