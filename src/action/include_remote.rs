//! `include-remote`: the actions and `[vars]` another repository declares, read
//! from the materialization this machine has of it, however stale.
//!
//! Reading is all this module does, while the run's list is assembled rather
//! than while it is executed; the record installs nothing. An inclusion with no
//! materialization warns and contributes nothing, which is what makes a plan
//! [partial](../../docs/cmdline.md#plan-completeness).

use std::collections::BTreeMap;

use crate::action::RunContext;
use crate::error::Error;
use crate::item::{ItemId, ItemIdList};
use crate::manifest::Manifest;
use crate::manifest::action::{Action, Contributor, IncludeRemoteAction};
use crate::output::Reporter;
use crate::paths;
use crate::var::VarName;

/// What one `include-remote` read out of the manifest it opened.
pub(crate) struct InclusionContents {
    /// The included manifest's own `[vars]`, which becomes the layer beneath the
    /// leaf's in the [scope](crate::var_set::VarSet::with_inclusion) these
    /// records are decided against. Empty is a remote that declared none.
    pub vars: BTreeMap<VarName, String>,
    pub actions: Vec<IncludedAction>,
}

/// One record of an included manifest, and what the inclusion's filters made
/// of it.
///
/// Included is what the manifest it was read from is, not a verdict on the
/// record: one the filters leave out is here too, unselected, because the run
/// still has things to say about it. It keeps the address that reaches it, so a
/// skip naming it is answered rather than reported as matching nothing, and the
/// run reports why it was passed over.
pub(crate) struct IncludedAction {
    /// The one-based position the record was declared at, in the manifest that
    /// declared it.
    pub number: usize,
    pub action: Action,
    /// Whether the inclusion's [filters](Filter) take this record.
    pub selected: bool,
}

/// Read the manifest of the remote this record includes, and return its `[vars]`
/// with the records it declares and what this inclusion's filters made of each.
///
/// `None` is an inclusion whose list was never read, because there was no
/// materialization to read it from; [`InclusionContents`] holding no records is
/// a manifest that was read but contributed no actions. Only the second can
/// answer whether a qualified name matches something, which is the distinction a
/// clone list's [entries](crate::manifest::action::GitCloneListAction::entries)
/// also draw. A record the filters leave out is returned unselected rather than
/// left out of the list, so both remain distinct from it.
///
/// Fails where the materialization cannot be inspected, and where its manifest
/// cannot be read, parsed, or validated. A materialized tree holding no manifest
/// at all is one of these: a remote's manifest is optional, so an inclusion
/// asking for one that is not there is asking for something absent rather than
/// for a tree batfiles has yet to fetch.
///
/// A remote its own condition closed is the caller's to decide, and is settled
/// before asking.
///
/// `label` is how a report [names](label) this inclusion, taken from the caller
/// rather than derived here: it is built from the record's position in the leaf
/// manifest, which this function cannot see.
pub(super) fn read(
    action: &IncludeRemoteAction,
    label: &str,
    context: &RunContext<'_>,
) -> Result<Option<InclusionContents>, Error> {
    let remote = &action.remote;
    let reporter = context.reporter();

    let tree = context.materialization(remote);
    if !paths::occupied(&tree)? {
        // The one thing a missing tree does that a missing source does not: a
        // leaf action reaching an absent materialization is refused, because one
        // action's content is something the rest of the plan can do without. A
        // list of actions is not, so this warns and the run carries on, having
        // said which part of the plan it could not draw.
        reporter.warn(&format!(
            "remote `{remote}` is not materialized at {}, so what it includes \
             cannot be listed; run `batfiles sync` to bring it down",
            tree.display()
        ));
        return Ok(None);
    }

    let manifest = tree.join(Manifest::FILE_NAME);
    if !paths::occupied(&manifest)? {
        return Err(Error::IncludedManifestMissing {
            remote: remote.clone(),
            path: manifest,
        });
    }

    let included = Manifest::load_included(&manifest)?;
    report_ignored_remotes(&included, label, remote, reporter);
    // How a line about one of these records names the inclusion that is reading
    // them, which is the inclusion's `id` where it has one and its label where
    // it has none.
    let by = contributor(action.id.as_ref(), label);
    let filter = Filter::of(action);
    let mut records = Vec::with_capacity(included.actions.len());
    for (index, record) in included.actions.into_iter().enumerate() {
        // The position the record was declared at, carried rather than
        // recomputed after the filtering below: a record with no `id` is named
        // by where it was written, so the gap a dropped one leaves stays a gap.
        let number = index + 1;
        // Inclusion is one level deep: an included repository does not reach
        // further repositories, which is the same rule that refuses an included
        // action sourcing from a remote. Dropped rather than refused, because
        // the manifest breaking it belongs to someone else and the rest of what
        // it declares is still good — and warned about rather than passed over
        // in silence, because a declaration that is not honored is worth saying.
        // Dropped ahead of the filters, so a nested inclusion's `id` is not one
        // of the names a filter can be satisfied by. What it names is not
        // required to resolve, since the manifest's own `[remotes]` is ignored
        // by the same rule.
        if let Action::IncludeRemote(_) = record {
            reporter.warn(&format!(
                "not included: {}; an included repository does not reach \
                 further repositories",
                record.describe(number, by)
            ));
            continue;
        }
        let selected = filter.selects(&record);
        records.push(IncludedAction {
            number,
            action: record,
            selected,
        });
    }
    filter.warn_unmatched(&records, label, remote, reporter);
    Ok(Some(InclusionContents {
        // The filters have nothing to say about these: a remote declares
        // variables for all of its records, and this inclusion takes them
        // whichever records it took.
        vars: included.vars,
        actions: records,
    }))
}

/// Warn that the `[remotes]` an included manifest declares does nothing here,
/// once for the map rather than once for each record in it.
///
/// Inclusion is one level deep, so a remote another repository declares is
/// neither materialized nor nameable: an included action sourcing from one is
/// refused as the manifest is read, and an included `include-remote` is left out
/// of the run. The map is read as part of the document and then ignored, which
/// is why its records are not checked for anything beyond being readable — a
/// remote type this batfiles has yet to build is that repository's business,
/// answered where it is the leaf.
///
/// Said out loud rather than passed over in silence, for the same reason a
/// dropped nested inclusion is: a declaration that is not honored is worth a
/// line. Silent where the manifest declares no remotes, which is the common
/// case.
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

/// How a report names one inclusion: stable for a given manifest, and shared by
/// no two inclusions of it.
///
/// Its `id` where it has one, since that is the address a reader would type. One
/// written without an `id` is named by `number`, its one-based position in the
/// leaf manifest, and by the remote it includes: the position is what tells two
/// inclusions of one remote apart, and is also the manifest's own answer for a
/// record nothing else can name, since that is how a report names any record
/// written without an `id`. The remote comes with it because what an inclusion
/// includes is the next most useful thing to say about it.
///
/// Unique because a manifest declaring one `id` twice is
/// [refused](crate::manifest::check::Invalid::DuplicateActionId) as it is read,
/// so an inclusion is told from every other by its `id` or by its position.
pub(super) fn label(action: &IncludeRemoteAction, number: usize) -> String {
    match &action.id {
        Some(id) => format!("include-remote `{id}`"),
        None => format!(
            "include-remote action {number} of remote `{}`",
            action.remote
        ),
    }
}

/// How the records one inclusion contributes are named in a line about them:
/// under its `id` where it has one, and by its `label` where it has none, since
/// then no address reaches them.
pub(super) fn contributor<'a>(id: Option<&'a ItemId>, label: &'a str) -> Contributor<'a> {
    match id {
        Some(id) => Contributor::Inclusion(id),
        None => Contributor::UnnamedInclusion(label),
    }
}

/// Which of a remote's actions one inclusion takes.
///
/// Four fields, of which the record writes a combination
/// [`check_inclusion_filters`](crate::manifest::check) accepts: an allow-list
/// that says what to take, a deny-list that says what to leave, or both, or
/// neither. With none of them written the inclusion takes everything, which is
/// why each field is an `Option` rather than a list that happens to be empty:
/// an empty allow-list takes nothing at all.
///
/// A filter names records as the manifest that declared them names them, so an
/// action written without an `id` is one no `install-actions` can reach and no
/// `exclude-actions` can name, and the same holds of `group` and the two group
/// filters. Under an allow-list such a record is left out, since nothing
/// selected it; under a deny-list it is taken, since nothing excluded it.
pub(super) struct Filter<'a> {
    install_actions: Option<&'a ItemIdList>,
    install_groups: Option<&'a ItemIdList>,
    exclude_actions: Option<&'a ItemIdList>,
    exclude_groups: Option<&'a ItemIdList>,
}

impl<'a> Filter<'a> {
    pub fn of(action: &'a IncludeRemoteAction) -> Self {
        Self {
            install_actions: action.install_actions.as_ref(),
            install_groups: action.install_groups.as_ref(),
            exclude_actions: action.exclude_actions.as_ref(),
            exclude_groups: action.exclude_groups.as_ref(),
        }
    }

    /// Whether this inclusion takes the record.
    pub fn selects(&self, action: &Action) -> bool {
        let (id, group) = (action.id(), action.group());
        let names = |list: Option<&ItemIdList>, item: Option<&ItemId>| {
            list.map(|list| item.is_some_and(|item| list.contains(item)))
        };
        // Allow first, then deny, which is the order the fields read in and the
        // only order under which `exclude-actions` narrows what a group filter
        // selected. At most one of the two allow-lists is written, so the
        // second is consulted only where the first was not.
        let allowed = names(self.install_actions, id)
            .or_else(|| names(self.install_groups, group))
            .unwrap_or(true);
        allowed
            && !names(self.exclude_groups, group).unwrap_or(false)
            && !names(self.exclude_actions, id).unwrap_or(false)
    }

    /// Warn once per filter name that nothing in the included manifest answers
    /// to.
    ///
    /// The manifest has been read, so batfiles can tell, and a name matching
    /// nothing is the same kind of mistake as a `--skip-action` that matches
    /// nothing: it selects or excludes nothing whatever the machine does.
    /// A warning rather than a failure, since a remote at an older revision
    /// than the leaf expects is a repository to update, not a run to stop.
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

    /// Every filter name nothing in the included manifest answers to, as the
    /// field that wrote it, the name, and what that field names.
    ///
    /// Each field is checked against its own namespace: the action filters
    /// against the IDs the manifest declared, the group filters against the
    /// groups its actions name.
    fn unmatched(&self, included: &[IncludedAction]) -> Vec<(&'static str, &ItemId, &'static str)> {
        let declared = |of: fn(&Action) -> Option<&ItemId>| -> Vec<&ItemId> {
            included.iter().filter_map(|it| of(&it.action)).collect()
        };
        let (actions, groups) = (declared(Action::id), declared(Action::group));
        let mut unmatched = Vec::new();
        for (field, list, present, noun) in [
            ("install-actions", self.install_actions, &actions, "action"),
            ("exclude-actions", self.exclude_actions, &actions, "action"),
            ("install-groups", self.install_groups, &groups, "group"),
            ("exclude-groups", self.exclude_groups, &groups, "group"),
        ] {
            for name in list.map(ItemIdList::as_slice).unwrap_or_default() {
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
    fn an_inclusion_is_labelled_by_its_id_or_by_where_it_was_written() {
        assert_eq!(label(&inclusion(""), 3), "include-remote `corp`");
        // Two inclusions of one remote, neither written with an `id`: the
        // position is the whole of what tells the labels apart, and it is the
        // manifest's own answer for a record nothing else can name.
        let unnamed: IncludeRemoteAction =
            toml::from_str("remote = \"corporate\"\n").expect("the record should parse");
        assert_eq!(
            label(&unnamed, 2),
            "include-remote action 2 of remote `corporate`"
        );
        assert_eq!(
            label(&unnamed, 5),
            "include-remote action 5 of remote `corporate`"
        );
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
        // A record written without an `id` is named by no `install-actions` and
        // by no `exclude-actions`, so an allow-list leaves it out for want of
        // anything selecting it and a deny-list keeps it for want of anything
        // excluding it. `group` and the two group filters read the same way,
        // which is `seeds` above.
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
                selected: true,
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
