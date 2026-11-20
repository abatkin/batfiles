//! `dynamic-vars.toml`: the captured output of dynamic-variable declarations.
//! See [`docs/state.md`](../../../docs/state.md#dynamic-varstoml-dynamic-variable-cache).

use std::collections::BTreeMap;
use std::path::Path;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::filesystem::tomlfile;
use crate::manifest::vars::{CaptureMode, CommandSource, CommandSpec, GithubRepository};

/// The parsed cache, keyed by declaration [key](super::DynamicVarKey) rather than
/// by variable name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub(crate) struct DynamicVarCache {
    pub entries: BTreeMap<String, CachedVar>,
}

/// One captured value. `value` and `captured-at` are required, and an unknown field is
/// invalid.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CachedVar {
    /// The captured value; a status capture is `"true"` or `"false"`.
    pub value: String,
    /// When the value was captured, as an RFC 3339 string.
    pub captured_at: Timestamp,
    /// The declaration that captured it. Every capture writes one; an entry without one
    /// serves any declaration.
    #[serde(default)]
    pub from: Option<Provenance>,
}

/// The fields of an entry written as its table's own lines.
#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
struct EntryLines<'a> {
    value: &'a str,
    captured_at: Timestamp,
}

/// A declaration's value-determining fields, as an entry records them: `command` and
/// `capture`, or `github-release` and the resolved `github-url`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct Provenance {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    command: Option<CommandSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    capture: Option<CaptureMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    github_release: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    github_url: Option<String>,
}

impl Provenance {
    /// A command declaration's, with its capture mode spelled out.
    pub fn command(source: &CommandSource) -> Self {
        Self {
            command: Some(source.command.clone()),
            capture: Some(source.capture),
            github_release: None,
            github_url: None,
        }
    }

    /// A `github-release` declaration's, asking `host`.
    pub fn release(repository: &GithubRepository, host: &str) -> Self {
        Self {
            command: None,
            capture: None,
            github_release: Some(repository.as_str().to_owned()),
            github_url: Some(host.to_owned()),
        }
    }
}

impl DynamicVarCache {
    /// The document's file name; [`MachineRoots`](crate::location::MachineRoots)
    /// decides its directory.
    pub const FILE_NAME: &'static str = "dynamic-vars.toml";

    /// Load the cache, treating a missing file as an empty one. A malformed one
    /// fails and is left alone.
    pub fn load(path: &Path) -> Result<Self, Error> {
        tomlfile::read_or_default(path)
    }

    /// Rewrite the cache.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        let text = self.render().map_err(|source| Error::Serialize {
            path: path.to_path_buf(),
            source,
        })?;
        tomlfile::write_text(path, &text)
    }

    /// The document: one table per entry, with its `from` as an inline table.
    fn render(&self) -> Result<String, toml::ser::Error> {
        let mut document = String::new();
        for (key, entry) in &self.entries {
            if !document.is_empty() {
                document.push('\n');
            }
            let lines = EntryLines {
                value: &entry.value,
                captured_at: entry.captured_at,
            };
            document.push_str(&toml::to_string(&BTreeMap::from([(key, lines)]))?);
            if let Some(from) = &entry.from {
                document.push_str("from = ");
                from.serialize(toml::ser::ValueSerializer::new(&mut document))?;
                document.push('\n');
            }
        }
        Ok(document)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
[fzf_tag]
value = "v0.75.0"
captured-at = "2026-11-20T12:00:00Z"
from = { github-release = "junegunn/fzf", github-url = "https://github.com" }

[work_email]
value = "me@work.com"
captured-at = "2026-06-19T12:00:00Z"
from = { command = ["git", "config", "user.email"], capture = "stdout" }

["remote:core.has_op"]
value = "true"
captured-at = "2026-06-19T12:00:00Z"
from = { command = "command -v op >/dev/null", capture = "status" }

[legacy]
value = "old"
captured-at = "2026-06-19T12:00:00Z"
"#;

    fn parse(document: &str) -> Result<DynamicVarCache, toml::de::Error> {
        toml::from_str(document)
    }

    #[test]
    fn leaf_and_remote_entries_share_the_document() {
        let cache = parse(EXAMPLE).expect("parse");
        assert_eq!(cache.entries["work_email"].value, "me@work.com");
        assert_eq!(
            cache.entries["work_email"].captured_at,
            "2026-06-19T12:00:00Z".parse::<Timestamp>().expect("valid")
        );
        assert_eq!(cache.entries["remote:core.has_op"].value, "true");
    }

    #[test]
    fn an_entry_records_the_declaration_that_captured_it_or_nothing() {
        let cache = parse(EXAMPLE).expect("parse");
        let repository = GithubRepository::try_from("junegunn/fzf".to_owned()).expect("valid");
        assert_eq!(
            cache.entries["fzf_tag"].from,
            Some(Provenance::release(&repository, "https://github.com"))
        );
        assert_eq!(
            cache.entries["remote:core.has_op"].from,
            Some(Provenance::command(&CommandSource {
                command: CommandSpec::Shell("command -v op >/dev/null".to_owned()),
                capture: CaptureMode::Status,
                command_timeout: None,
            }))
        );
        assert_eq!(cache.entries["legacy"].from, None);
        let error = parse(
            "[a]\nvalue = 'x'\ncaptured-at = '2026-06-19T12:00:00Z'\nfrom = { cache = '1h' }\n",
        )
        .expect_err("closed record");
        assert!(
            error.to_string().contains("unknown field `cache`"),
            "{error}"
        );
    }

    #[test]
    fn both_entry_fields_are_required_and_no_others_are_allowed() {
        let error = parse("[a]\nvalue = 'x'\n").expect_err("no timestamp");
        assert!(
            error.to_string().contains("missing field `captured-at`"),
            "{error}"
        );
        let error = parse("[a]\ncaptured-at = '2026-06-19T12:00:00Z'\n").expect_err("no value");
        assert!(
            error.to_string().contains("missing field `value`"),
            "{error}"
        );
        let error = parse("[a]\nvalue = 'x'\ncaptured-at = '2026-06-19T12:00:00Z'\nttl = '1h'\n")
            .expect_err("closed record");
        assert!(error.to_string().contains("unknown field `ttl`"), "{error}");
    }

    #[test]
    fn a_timestamp_is_an_rfc_3339_string() {
        let error =
            parse("[a]\nvalue = 'x'\ncaptured-at = 'yesterday'\n").expect_err("not a timestamp");
        assert!(error.to_string().contains("line 3"), "{error}");
        // TOML's native datetime is not the format's spelling.
        assert!(parse("[a]\nvalue = 'x'\ncaptured-at = 2026-06-19T12:00:00Z\n").is_err());
    }

    #[test]
    fn a_remote_key_stays_one_quoted_key_and_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(DynamicVarCache::FILE_NAME);
        let cache = parse(EXAMPLE).expect("parse");
        let document = cache.render().expect("serialize");
        assert!(document.contains("[\"remote:core.has_op\"]"), "{document}");
        assert!(
            document.contains(
                "from = { command = [\"git\", \"config\", \"user.email\"], capture = \"stdout\" }"
            ),
            "{document}"
        );
        assert!(
            document.contains("captured-at = \"2026-06-19T12:00:00Z\""),
            "{document}"
        );
        cache.save(&path).expect("save");
        assert_eq!(DynamicVarCache::load(&path).expect("load"), cache);
    }

    #[test]
    fn a_missing_cache_is_an_empty_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            DynamicVarCache::load(&dir.path().join(DynamicVarCache::FILE_NAME))
                .expect("absent is empty"),
            DynamicVarCache::default()
        );
    }
}
