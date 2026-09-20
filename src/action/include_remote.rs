//! `include-remote`: the actions and `[vars]` another repository declares, read
//! from the materialization this machine has of it, however stale.
//!
//! Reading is all this module does, while the run's list is assembled rather
//! than while it is executed; the record installs nothing. An inclusion with no
//! materialization warns and contributes nothing, which is what makes a plan
//! [partial](../../docs/cmdline.md#plan-completeness).

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::action::RunContext;
use crate::condition::Exclusion;
use crate::error::Error;
use crate::item::{ItemId, ItemIdList};
use crate::manifest::Manifest;
use crate::manifest::action::{Action, Contributor, IncludeRemoteAction};
use crate::output::Reporter;
use crate::paths;
use crate::var::VarName;
use crate::var_set::VarSet;

/// One `include-remote` the run reached, as the leaf manifest declared it.
///
/// Owns its identity and selection filters before the remote manifest is read.
pub(crate) struct Inclusion {
    /// The `id` it was written with, which qualifies the addresses of what it
    /// contributes. `None` is one written without an `id`: no address reaches
    /// what it brought in, and [`label`](Self::label) names it instead.
    id: Option<ItemId>,
    /// The remote it includes, whose materialization the records it contributes
    /// read their repository paths from.
    remote: ItemId,
    label: String,
    filter: Filter,
}

impl Inclusion {
    /// Identify the inclusion the record at one-based position `number` writes.
    ///
    /// Reports name it by `id`, or by its position and remote if unnamed.
    /// `number` must be its position in the validated leaf manifest.
    pub(crate) fn at(action: &IncludeRemoteAction, number: usize) -> Self {
        let label = match &action.id {
            Some(id) => format!("include-remote `{id}`"),
            None => format!(
                "include-remote action {number} of remote `{}`",
                action.remote
            ),
        };
        Self {
            id: action.id.clone(),
            remote: action.remote.clone(),
            label,
            filter: Filter::of(action),
        }
    }

    /// The `id` this inclusion answers to, where it was written with one.
    pub(crate) fn id(&self) -> Option<&ItemId> {
        self.id.as_ref()
    }

    /// The remote it includes.
    pub(crate) fn remote(&self) -> &ItemId {
        &self.remote
    }

    /// How a report names it.
    pub(crate) fn label(&self) -> &str {
        &self.label
    }

    /// How the records it contributes are named in a line about them: under its
    /// `id` where it has one, and by its label where it has none, since then no
    /// address reaches them.
    fn contributor(&self) -> Contributor<'_> {
        match &self.id {
            Some(id) => Contributor::Inclusion(id),
            None => Contributor::UnnamedInclusion(&self.label),
        }
    }

    /// A remote's exclusion, said about the inclusion that depended on it.
    ///
    /// The severity is the remote's: a condition batfiles could not decide is a
    /// warning wherever it is reported.
    pub(crate) fn closed_by_remote(&self, exclusion: &Exclusion) -> Exclusion {
        let reason = format!(
            "remote `{}` is excluded here: {}",
            self.remote,
            exclusion.reason()
        );
        match exclusion {
            Exclusion::Expected(_) => Exclusion::Expected(reason),
            Exclusion::EvaluationFailed(_) => Exclusion::EvaluationFailed(reason),
        }
    }

    /// What this inclusion hands the records it contributed, once its manifest
    /// has been read and `vars` derived from it.
    pub(crate) fn with_scope(self, vars: Rc<VarSet>) -> Contribution {
        Contribution {
            inclusion: self,
            vars,
        }
    }
}

/// One opened inclusion, and everything the records it contributed answer to.
///
/// Shared by all of them rather than copied onto each: they were read from one
/// manifest, their paths resolve in one materialization, and their conditions
/// are decided in one scope, so nothing a record is treated as can disagree with
/// the inclusion that brought it in.
pub(crate) struct Contribution {
    inclusion: Inclusion,
    /// The scope these records' own conditions -- and their clone lists' entry
    /// conditions -- are decided against: what the inclusion
    /// [derived](crate::var_set::VarSet::with_inclusion), which is the run's own
    /// set where neither it nor its remote declared anything.
    vars: Rc<VarSet>,
}

impl Contribution {
    /// The remote whose materialization these records' repository paths are read
    /// from.
    pub(crate) fn remote(&self) -> &ItemId {
        self.inclusion.remote()
    }

    /// The scope they are decided in.
    pub(crate) fn scope(&self) -> &Rc<VarSet> {
        &self.vars
    }

    /// How a line about one of them names the inclusion.
    pub(crate) fn contributor(&self) -> Contributor<'_> {
        self.inclusion.contributor()
    }

    /// Why a record this inclusion's filters left out was passed over.
    ///
    /// Not an exclusion this machine applied: it is the leaf saying what it
    /// composed, which is why the line names the inclusion rather than a list.
    pub(crate) fn not_selected(&self) -> Exclusion {
        Exclusion::Expected(format!("not selected by {}", self.inclusion.label))
    }
}

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
/// A record the filters leave out is here too, unselected: it keeps the address
/// that reaches it, so a skip naming it is answered rather than reported as
/// matching nothing, and the run says why it was passed over.
pub(crate) struct IncludedAction {
    /// The one-based position the record was declared at, in the manifest that
    /// declared it.
    pub number: usize,
    pub action: Action,
    /// Whether the inclusion's [filters](Filter) take this record. It says what
    /// the leaf repository composed and nothing about what this machine leaves
    /// out of a run, which is why no command waives it.
    pub included_by_filter: bool,
}

impl Inclusion {
    /// Read the manifest of the remote this inclusion names, and return its
    /// `[vars]` with the records it declares and what the filters made of each.
    ///
    /// `None` is an inclusion whose manifest was never read, because there was
    /// no materialization to read it from; [`InclusionContents`] holding no
    /// records is a manifest that was read and contributed nothing. Only the
    /// second can answer whether a qualified name matches something. A record
    /// the filters leave out is returned unselected rather than left out, so it
    /// stays distinct from both.
    ///
    /// Fails where the materialization cannot be inspected, and where its
    /// manifest cannot be read, parsed, or validated. A materialized tree
    /// holding no manifest at all is one of these: a remote's manifest is
    /// optional, so an inclusion asking for one that is not there is asking for
    /// something absent rather than for a tree batfiles has yet to fetch.
    ///
    /// The caller must check the remote's condition before reading.
    pub(crate) fn read(
        &self,
        context: &RunContext<'_>,
    ) -> Result<Option<InclusionContents>, Error> {
        let label = self.label();
        let remote = self.remote();
        let reporter = context.reporter();

        let tree = context.materialization(remote);
        if !paths::occupied(&tree)? {
            // The one thing a missing tree does that a missing source does not:
            // a leaf action reaching an absent materialization is refused,
            // because one action's content is something the rest of the plan can
            // do without. A list of actions is not, so this warns and the run
            // carries on, having said which part of the plan it could not draw.
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
        let by = self.contributor();
        let filter = &self.filter;
        let mut records = Vec::with_capacity(included.actions.len());
        for (index, record) in included.actions.into_iter().enumerate() {
            // The position the record was declared at, carried rather than
            // recomputed after the filtering below: a record with no `id` is
            // named by where it was written, so the gap a dropped one leaves
            // stays a gap.
            let number = index + 1;
            // Inclusion is one level deep, so a nested one is left out rather
            // than refused, and said out loud. Left out ahead of the filters, so
            // its `id` is not one of the names a filter can be satisfied by.
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
        Ok(Some(InclusionContents {
            // The filters have nothing to say about these: a remote declares
            // variables for all of its records, and this inclusion takes them
            // whichever records it took.
            vars: included.vars,
            actions: records,
        }))
    }
}

/// Warn that the `[remotes]` an included manifest declares does nothing here:
/// one line per inclusion, naming every record in the map, and silent where the
/// manifest declares none.
///
/// The rule the warning states, and how far such a record is checked, are
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
/// Each field is an `Option` rather than a list that happens to be empty: with
/// none of them written the inclusion takes everything, while an empty
/// allow-list takes nothing. Which combinations a record may write is
/// [`check_inclusion_filters`](crate::manifest::check)'s rule; what each selects
/// is [`docs/repoformat.md`](../../docs/repoformat.md#selecting-part-of-a-remote)'s.
///
/// A filter names records as the manifest that declared them names them, so a
/// record written without an `id` is reached by neither action filter: an
/// allow-list leaves it out, a deny-list takes it, and `group` reads the same
/// way.
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
        // Allow first, then deny, which is the order the fields read in and the
        // only order under which `exclude-actions` narrows what a group filter
        // selected. At most one of the two allow-lists is written, so the
        // second is consulted only where the first was not.
        let allowed = names(self.install_actions.as_ref(), id)
            .or_else(|| names(self.install_groups.as_ref(), group))
            .unwrap_or(true);
        allowed
            && !names(self.exclude_groups.as_ref(), group).unwrap_or(false)
            && !names(self.exclude_actions.as_ref(), id).unwrap_or(false)
    }

    /// Warn once per filter name that nothing in the included manifest answers
    /// to.
    ///
    /// A warning rather than a failure: a remote at an older revision than the
    /// leaf expects is a repository to update, not a run to stop.
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
    fn an_inclusion_is_labelled_by_its_id_or_by_where_it_was_written() {
        let labelled =
            |action: &IncludeRemoteAction, number| Inclusion::at(action, number).label().to_owned();
        assert_eq!(labelled(&inclusion(""), 3), "include-remote `corp`");
        // Two inclusions of one remote, neither written with an `id`: the
        // position is the whole of what tells the labels apart.
        let unnamed: IncludeRemoteAction =
            toml::from_str("remote = \"corporate\"\n").expect("the record should parse");
        assert_eq!(
            labelled(&unnamed, 2),
            "include-remote action 2 of remote `corporate`"
        );
        assert_eq!(
            labelled(&unnamed, 5),
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
