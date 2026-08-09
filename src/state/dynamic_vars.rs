//! `dynamic-vars.toml`: captured output of dynamic variable declarations.
//!
//! Disposable cache data, kept in the cache directory and named differently from
//! `vars.toml` so regenerable captures are never mistaken for the machine-local
//! configuration a user wrote.
//!
//! The top-level key is a *declaration* identity, not a variable name: leaf
//! declarations use their bare name and remote ones use
//! `remote:<remote-id>.<name>`, so the key type is a `String` rather than a
//! [`VarName`]. The remote ID is the key in the leaf's `[remotes]` map, not the
//! `id` of an `include-remote`, which is what makes every inclusion of one
//! remote share a single entry per declaration.

use std::collections::BTreeMap;
use std::path::Path;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::item::ItemId;
use crate::tomlfile;
use crate::var::VarName;

/// The parsed `dynamic-vars.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct DynamicVarCache {
    pub entries: BTreeMap<String, CachedVar>,
}

/// One captured value. A closed record: both fields are required, and an
/// unknown one is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CachedVar {
    /// The captured value. A `status` capture is stored as `"true"` or
    /// `"false"`, like every other batfiles variable value.
    pub value: String,
    /// When the capture happened, as an RFC 3339 timestamp. Freshness compares
    /// this instant against the declaration's cache duration, so it is parsed
    /// here rather than kept as text.
    pub captured_at: Timestamp,
}

impl DynamicVarCache {
    /// The document's name. It is deliberately unlike `vars.toml`, so cache is
    /// never mistaken for configuration; which directory it sits in — the cache
    /// directory, not the config one — is [`Roots`](crate::config::Roots)'
    /// answer, not this type's.
    pub const FILE_NAME: &'static str = "dynamic-vars.toml";

    /// Load the cache, treating a missing file as an empty one. Deleting the
    /// file is safe, so this is the common case rather than an error.
    pub fn load(path: &Path) -> Result<Self, tomlfile::Error> {
        tomlfile::read_or_default(path)
    }

    /// Rewrite the cache.
    pub fn save(&self, path: &Path) -> Result<(), tomlfile::Error> {
        tomlfile::write(path, self)
    }

    /// The cache key of a leaf repository's declaration: its bare name.
    pub fn leaf_key(name: &VarName) -> String {
        name.to_string()
    }

    /// The cache key of a remote's declaration.
    ///
    /// `remote_id` is the remote's key in the leaf's `[remotes]` map, hence an
    /// [`ItemId`]. The `remote:` prefix is what keeps leaf and remote
    /// declarations in separate namespaces even when they share a variable name.
    pub fn remote_key(remote_id: &ItemId, name: &VarName) -> String {
        format!("remote:{remote_id}.{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
[work_email]
value = "me@work.com"
captured-at = "2026-06-19T12:00:00Z"

["remote:core.has_op"]
value = "true"
captured-at = "2026-06-19T12:00:00Z"
"#;

    fn parse(document: &str) -> Result<DynamicVarCache, toml::de::Error> {
        toml::from_str(document)
    }

    fn remote_id(id: &str) -> ItemId {
        ItemId::new(id).expect("valid id")
    }

    fn timestamp(text: &str) -> Timestamp {
        text.parse().expect("valid timestamp")
    }

    #[test]
    fn leaf_and_remote_entries_share_the_document() {
        let cache = parse(EXAMPLE).expect("parse");
        assert_eq!(
            cache.entries["work_email"],
            CachedVar {
                value: "me@work.com".to_owned(),
                captured_at: timestamp("2026-06-19T12:00:00Z"),
            }
        );
        assert_eq!(cache.entries["remote:core.has_op"].value, "true");
    }

    #[test]
    fn an_empty_cache_is_valid() {
        assert_eq!(parse("").expect("empty"), DynamicVarCache::default());
    }

    #[test]
    fn both_entry_fields_are_required() {
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
    }

    #[test]
    fn an_unknown_entry_field_is_rejected() {
        let error = parse("[a]\nvalue = 'x'\ncaptured-at = '2026-06-19T12:00:00Z'\nttl = '1h'\n")
            .expect_err("closed record");
        assert!(error.to_string().contains("unknown field `ttl`"), "{error}");
    }

    #[test]
    fn a_timestamp_is_validated_while_the_document_is_read() {
        let error =
            parse("[a]\nvalue = 'x'\ncaptured-at = 'yesterday'\n").expect_err("not a timestamp");
        assert!(error.to_string().contains("line 3"), "{error}");
    }

    #[test]
    fn the_timestamp_is_a_string_rather_than_a_toml_datetime() {
        // TOML has a native datetime, but the format specifies a string, so an
        // unquoted datetime is a mistake rather than a second accepted spelling.
        let error =
            parse("[a]\nvalue = 'x'\ncaptured-at = 2026-06-19T12:00:00Z\n").expect_err("native");
        assert!(error.to_string().contains("invalid type"), "{error}");
    }

    #[test]
    fn keys_distinguish_leaf_and_remote_declarations() {
        let name = VarName::new("has_op").expect("valid name");
        assert_eq!(DynamicVarCache::leaf_key(&name), "has_op");
        assert_eq!(
            DynamicVarCache::remote_key(&remote_id("core"), &name),
            "remote:core.has_op"
        );
    }

    /// A path in a fresh directory, named the way the cache directory would name
    /// it.
    fn path(dir: &tempfile::TempDir) -> std::path::PathBuf {
        dir.path().join(DynamicVarCache::FILE_NAME)
    }

    #[test]
    fn a_remote_key_stays_one_key_when_written() {
        // The dot has to be quoted, or TOML reads `remote:core.has_op` as a
        // nested table.
        let dir = tempfile::tempdir().expect("temp dir");
        let name = VarName::new("has_op").expect("valid name");
        let cache = DynamicVarCache {
            entries: BTreeMap::from([(
                DynamicVarCache::remote_key(&remote_id("core"), &name),
                CachedVar {
                    value: "true".to_owned(),
                    captured_at: timestamp("2026-06-19T12:00:00Z"),
                },
            )]),
        };

        cache.save(&path(&dir)).expect("save");
        let document = std::fs::read_to_string(path(&dir)).expect("read");
        assert!(document.contains("[\"remote:core.has_op\"]"), "{document}");
        assert_eq!(DynamicVarCache::load(&path(&dir)).expect("load"), cache);
    }

    #[test]
    fn a_missing_cache_is_an_empty_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        assert_eq!(
            DynamicVarCache::load(&path(&dir)).expect("absent is empty"),
            DynamicVarCache::default()
        );
    }

    #[test]
    fn saving_and_loading_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache = parse(EXAMPLE).expect("parse");

        cache.save(&path(&dir)).expect("save");
        assert_eq!(DynamicVarCache::load(&path(&dir)).expect("load"), cache);
    }

    #[test]
    fn a_timestamp_is_written_back_as_rfc_3339() {
        let dir = tempfile::tempdir().expect("temp dir");
        parse(EXAMPLE)
            .expect("parse")
            .save(&path(&dir))
            .expect("save");

        let document = std::fs::read_to_string(path(&dir)).expect("read");
        assert!(
            document.contains("captured-at = \"2026-06-19T12:00:00Z\""),
            "{document}"
        );
    }
}
