//! `[default-disabled]`: what a fresh machine starts with switched off.
//!
//! These are candidates offered during `clone` bootstrap rather than a standing
//! setting (`docs/repoformat.md#default-disabled-bootstrap-entries`): adoption
//! resolves them against the environment and command line, then writes the
//! outcome to `disabled.toml`. A remote's `[default-disabled]` parses and is
//! then ignored, because bootstrap policy belongs to the leaf repository.

use serde::{Deserialize, Serialize};

use crate::repo::value::Condition;

/// The two candidate lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct DefaultDisabled {
    #[serde(default)]
    pub actions: Vec<DefaultDisabledAction>,
    #[serde(default)]
    pub groups: Vec<DefaultDisabledGroup>,
}

/// One `[[default-disabled.actions]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct DefaultDisabledAction {
    /// An addressable remote, action, included action, or manifest-entry ID.
    pub id: String,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
}

/// One `[[default-disabled.groups]]` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct DefaultDisabledGroup {
    /// A leaf or qualified included group address.
    pub group: String,
    pub when: Option<Condition>,
    pub unless: Option<Condition>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(document: &str) -> Result<DefaultDisabled, toml::de::Error> {
        toml::from_str(document)
    }

    #[test]
    fn both_lists_parse() {
        let disabled = parse(
            r#"
[[actions]]
id = "p10k"

[[actions]]
id = "core.work-tools"
when = "work"

[[groups]]
group = "gui"
unless = "facts.os == 'darwin'"
"#,
        )
        .expect("parse");

        assert_eq!(disabled.actions.len(), 2);
        assert_eq!(disabled.actions[0].id, "p10k");
        assert_eq!(disabled.actions[0].when, None);
        assert_eq!(disabled.actions[1].when.as_deref(), Some("work"));
        assert_eq!(disabled.groups[0].group, "gui");
        assert_eq!(
            disabled.groups[0].unless.as_deref(),
            Some("facts.os == 'darwin'")
        );
    }

    #[test]
    fn an_absent_section_disables_nothing() {
        assert_eq!(parse("").expect("empty"), DefaultDisabled::default());
    }

    #[test]
    fn an_action_entry_needs_its_address() {
        let error = parse("[[actions]]\nwhen = 'work'\n").expect_err("no id");
        assert!(error.to_string().contains("missing field `id`"), "{error}");
    }

    #[test]
    fn a_group_entry_needs_its_address() {
        let error = parse("[[groups]]\nwhen = 'work'\n").expect_err("no group");
        assert!(
            error.to_string().contains("missing field `group`"),
            "{error}"
        );
    }

    #[test]
    fn an_entry_names_an_address_rather_than_an_action_record() {
        // A candidate can name something that does not exist yet, so nothing
        // here looks the address up or splits it.
        let disabled = parse("[[actions]]\nid = 'core.zsh-plugins.p10k'\n").expect("parse");
        assert_eq!(disabled.actions[0].id, "core.zsh-plugins.p10k");
    }

    #[test]
    fn an_unknown_entry_field_is_rejected() {
        let error = parse("[[actions]]\nid = 'p10k'\ngroup = 'gui'\n").expect_err("closed records");
        assert!(
            error.to_string().contains("unknown field `group`"),
            "{error}"
        );
    }
}
