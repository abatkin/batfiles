//! `dynamic-vars.toml`: the captured output of dynamic-variable declarations.
//! See [`docs/state.md`](../../docs/state.md#dynamic-varstoml-dynamic-variable-cache).

use std::collections::BTreeMap;
use std::path::Path;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::tomlfile;

/// The parsed cache, keyed by declaration [identity](super::VarIdentity) rather
/// than by variable name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct DynamicVarCache {
    pub entries: BTreeMap<String, CachedVar>,
}

/// One captured value. Both fields are required, and an unknown one is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct CachedVar {
    /// The captured value; a status capture is `"true"` or `"false"`.
    pub value: String,
    /// When the value was captured, as an RFC 3339 string.
    pub captured_at: Timestamp,
}

impl DynamicVarCache {
    /// The document's file name; [`StateRoots`](crate::location::StateRoots)
    /// decides its directory.
    pub const FILE_NAME: &'static str = "dynamic-vars.toml";

    /// Load the cache, treating a missing file as an empty one. A malformed one
    /// fails and is left alone.
    pub fn load(path: &Path) -> Result<Self, Error> {
        tomlfile::read_or_default(path)
    }

    /// Rewrite the cache.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        tomlfile::write(path, self)
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
        // What `save` writes.
        let document = toml::to_string(&cache).expect("serialize");
        assert!(document.contains("[\"remote:core.has_op\"]"), "{document}");
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
