//! Validate manifest values without filesystem access.
//! [`Manifest::validate`](super::Manifest::validate) applies cross-record rules. See [manifest
//! validation](../../docs/repoformat.md#reading-the-manifest).

use std::collections::BTreeMap;
use std::path::{Component, Path};

use thiserror::Error;

use super::DeclarationName;
use super::ReadAs;
use super::actions::{IncludeRemoteAction, RunAction};
use super::remote::Remote;
use super::trigger::{AfterTarget, Trigger};
use crate::manifest::condition::{Condition, SyntaxError};
use crate::manifest::entry_filter::GlobFilter;
use crate::manifest::item::ItemId;
use crate::manifest::repo_path::{REMOTE_PREFIX, RepoPath};
use crate::manifest::template::TemplateString;
use crate::manifest::vars::{DynamicSource, DynamicVarSpec, VarSpec};
use crate::source::fetch::{FileUrlError, file_url_path};

/// How a message names the tree a path is read from.
fn tree_of(written: &RepoPath) -> String {
    match written.remote() {
        None => "the repository".to_owned(),
        Some(id) => format!("remote `{id}`"),
    }
}

/// How a message names that tree's root.
fn root_of(written: &RepoPath) -> String {
    match written.remote() {
        None => "the repository root".to_owned(),
        Some(id) => format!("the root of remote `{id}`"),
    }
}

/// Describe the entire repository or remote materialization for a source-path diagnostic.
fn whole_of(written: &RepoPath) -> String {
    match written.remote() {
        None => "the whole repository".to_owned(),
        Some(id) => format!("the whole of remote `{id}`"),
    }
}

/// The bare ID of the remote a path names, for the message that spells a
/// declaration. Empty for a path naming no remote.
fn named_remote_of(written: &RepoPath) -> String {
    written.remote().map(ItemId::to_string).unwrap_or_default()
}

/// Manifest validation failures, including value and cross-record constraints.
#[derive(Debug, Error)]
pub(crate) enum ManifestError {
    #[error("action {second} repeats the id `{id}`, which action {first} already uses")]
    DuplicateActionId {
        id: ItemId,
        first: usize,
        second: usize,
    },

    /// Two remote IDs differ only in case.
    #[error(
        "remotes `{one}` and `{other}` differ only in case; where the filesystem \
         ignores case they are one directory under `remotes/`, and only one of \
         the two repositories would ever be cloned"
    )]
    RemotesShareOneDirectory { one: ItemId, other: ItemId },

    /// A record declares both `when` and `unless`.
    #[error("{record}: writes both `when` and `unless`; a record has one condition or none")]
    BothConditions { record: DeclarationName },

    // Repository source paths.
    #[error("{record}: source is empty; a source names a path within the repository")]
    SourceEmpty { record: DeclarationName },

    /// A source path has a root or platform prefix instead of being tree-relative.
    #[error("{record}: source `{written}` is not relative to {}", root_of(.written))]
    SourceNotRelative {
        record: DeclarationName,
        written: RepoPath,
    },

    #[error("{record}: source `{written}` resolves outside {}", tree_of(.written))]
    SourceOutsideTree {
        record: DeclarationName,
        written: RepoPath,
    },

    /// A source that stays inside its tree but names all of it.
    #[error(
        "{record}: source `{written}` names {}; a source names a path within it",
        whole_of(.written)
    )]
    SourceIsWholeTree {
        record: DeclarationName,
        written: RepoPath,
    },

    /// A path within a remote reference that itself begins with `@`, which
    /// always introduces a remote.
    #[error(
        "{record}: source `{written}` starts with `@`, which introduces a remote \
         reference and cannot start a path within one"
    )]
    SourceStartsWithRemotePrefix {
        record: DeclarationName,
        written: RepoPath,
    },

    /// A source references an undeclared remote.
    #[error(
        "{record}: source `{written}` names {}, which this manifest does not \
         declare; add a `[remotes.{}]` record",
        tree_of(.written),
        named_remote_of(.written)
    )]
    SourceRemoteUndeclared {
        record: DeclarationName,
        written: RepoPath,
    },

    /// An included action references a remote in its source.
    #[error(
        "{record}: source `{written}` names a remote, which an included action may not do; \
         remote references belong to the leaf repository, and an included action installs \
         from the repository that declared it"
    )]
    IncludedSourceNamesRemote {
        record: DeclarationName,
        written: RepoPath,
    },

    /// A source names a path inside a file remote.
    #[error(
        "{record}: source `{written}` names a path within {}, which is a single file; \
         write `@{}` to name the file itself",
        tree_of(.written),
        named_remote_of(.written)
    )]
    SourceInsideFileRemote {
        record: DeclarationName,
        written: RepoPath,
    },

    /// A `source-dir` naming a file remote, which is never a directory.
    #[error(
        "{record}: source `{written}` names {}, which is a single file; \
         this action installs from a directory",
        tree_of(.written)
    )]
    SourceDirIsFileRemote {
        record: DeclarationName,
        written: RepoPath,
    },

    /// An `include-remote` naming a remote that holds no manifest to read.
    #[error(
        "{record}: remote `{remote}` is of type `{kind}`; \
         an include-remote reads the manifest of a `git` remote"
    )]
    InclusionRemoteNotGit {
        record: DeclarationName,
        remote: ItemId,
        kind: &'static str,
    },

    /// An `include-remote` naming a remote no `[remotes]` entry declares.
    #[error(
        "{record}: remote `{remote}` is not declared by this manifest; \
         add a `[remotes.{remote}]` record"
    )]
    InclusionRemoteUndeclared {
        record: DeclarationName,
        remote: ItemId,
    },

    /// An inclusion declares incompatible selection filters.
    #[error(
        "{record}: writes both `{one}` and `{other}`; an inclusion names at most one of \
         `install-actions`, `install-groups`, and `exclude-groups`, and `exclude-actions` \
         goes with either group filter or alone"
    )]
    InclusionFiltersConflict {
        record: DeclarationName,
        one: &'static str,
        other: &'static str,
    },

    /// A condition or template field's value does not parse.
    #[error("{record}: {field} {source}")]
    Syntax {
        record: DeclarationName,
        field: &'static str,
        source: Box<SyntaxError>,
    },

    /// A template field's literal value breaks its field's rule.
    #[error("{record}: {field}{} {source}", written(.value))]
    Value {
        record: DeclarationName,
        field: &'static str,
        value: String,
        source: ValueError,
    },

    /// An archive root is not a valid path inside the archive.
    #[error(
        "{record}: archive-root `{value}` is not a path inside the archive; \
         write a prefix such as `tool-1.0`, or `*` for the archive's single top-level directory"
    )]
    ArchiveRootNotInside {
        record: DeclarationName,
        value: String,
    },

    /// A `run` action whose trigger keeps state, without the `id` that state is kept under.
    #[error(
        "{record}: a `{trigger}` trigger records when the command last succeeded under the \
         action's address, so the action needs an `id`"
    )]
    StatefulTriggerWithoutId {
        record: DeclarationName,
        trigger: &'static str,
    },

    /// An `after` target naming no action in the manifest.
    #[error(
        "{record}: `after` names `{target}`, which no action in this manifest declares; write \
         `@{target}` to follow a remote"
    )]
    AfterTargetUnknown {
        record: DeclarationName,
        target: ItemId,
    },

    /// An `after` target naming the action that declares it.
    #[error("{record}: `after` names `{target}`, the action itself")]
    AfterTargetSelf {
        record: DeclarationName,
        target: ItemId,
    },

    /// An `after` target declared later in the manifest.
    #[error(
        "{record}: `after` names `{target}`, which is action {at}; an action follows only \
         actions declared before it"
    )]
    AfterTargetLater {
        record: DeclarationName,
        target: ItemId,
        at: usize,
    },

    /// An `after` remote the manifest does not declare.
    #[error(
        "{record}: `after` names `@{remote}`, which this manifest does not declare; add a \
         `[remotes.{remote}]` record"
    )]
    AfterRemoteUndeclared {
        record: DeclarationName,
        remote: ItemId,
    },

    /// An included `run` action following a remote.
    #[error(
        "{record}: `after` names `@{remote}`, which an included action may not do; remote \
         references belong to the leaf repository"
    )]
    IncludedAfterNamesRemote {
        record: DeclarationName,
        remote: ItemId,
    },

    /// A `github-url` reading something other than the host namespaces.
    #[error(
        "{record}: `github-url` reads `{root}`; `github-url` may read `env` and `facts`, not \
         variables"
    )]
    GithubUrlReadsVariable {
        record: DeclarationName,
        root: String,
    },

    /// A filter over direct children whose pattern could only match below one.
    #[error(
        "{record}: {field} pattern `{pattern}` contains `/`; this action's filters match \
         the names of the direct children of source-dir"
    )]
    ChildFilterNested {
        record: DeclarationName,
        field: &'static str,
        pattern: String,
    },
}

/// What a [template field](crate::manifest::template::TemplateString)'s value must be. A
/// literal value is checked when its manifest loads; a templated one after it is evaluated.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ValueRule {
    /// A destination: never empty, and `~` only for the selected home.
    Destination,
    /// An `http://`, `https://`, or `file://` URL to fetch.
    Url,
    /// A repository for git to clone: never blank.
    GitSource,
    /// A branch, tag, or commit: never blank.
    GitRef,
    /// A SHA-256 digest.
    Digest,
    /// The `http://` or `https://` base a release path is appended to: it names a host, and
    /// has no credentials, query, or fragment.
    ReleaseHost,
    /// Any text, such as part of a command.
    Text,
}

impl ValueRule {
    /// Whether an error may quote `value`: not a release host that could hold credentials.
    pub fn may_show(self, value: &str) -> bool {
        !matches!(self, Self::ReleaseHost) || !value.contains('@')
    }

    /// Check `value`.
    pub fn check(self, value: &str) -> Result<(), ValueError> {
        match self {
            Self::Destination => check_dest(value),
            Self::Url => check_url(value),
            Self::ReleaseHost => check_release_host(value),
            Self::GitSource if value.trim().is_empty() => Err(ValueError::GitSourceEmpty),
            Self::GitRef if value.trim().is_empty() => Err(ValueError::GitRefEmpty),
            Self::Digest
                if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) =>
            {
                Err(ValueError::DigestNotSha256)
            }
            Self::GitSource | Self::GitRef | Self::Digest | Self::Text => Ok(()),
        }
    }
}

/// Why a value breaks its field's [`ValueRule`]. Messages follow the value and name neither
/// it nor its field, so a resolved template value need not be shown.
#[derive(Debug, Error)]
pub(crate) enum ValueError {
    #[error("is empty; write `~` for the home directory itself")]
    DestinationEmpty,

    #[error("names another user's home; `~` expands only to the selected home")]
    DestinationOtherHome,

    #[error("is not an http://, https://, or file:// URL")]
    NotAUrl,

    #[error("is not an http:// or https:// URL")]
    NotAnHttpUrl,

    #[error("names no host")]
    NoHost,

    #[error("contains credentials before `@`; a release lookup sends none")]
    Credentials,

    #[error("has a query or fragment, so the release path cannot be appended to it")]
    QueryOrFragment,

    #[error("is not a valid URL")]
    MalformedUrl,

    #[error(transparent)]
    FileUrl(FileUrlError),

    #[error("is not 64 hexadecimal digits")]
    DigestNotSha256,

    #[error("is empty; it names a repository for git to clone")]
    GitSourceEmpty,

    #[error("is empty; a ref names a branch, tag, or commit to follow")]
    GitRefEmpty,
}

/// A literal value as a diagnostic shows it after its field name: nothing if it is blank.
fn written(value: &str) -> String {
    if value.trim().is_empty() {
        String::new()
    } else {
        format!(" `{value}`")
    }
}

/// Check that a template field's value parses and, if it is literal, that it satisfies
/// `rule`. A templated value is checked when it is evaluated.
pub(super) fn check_template(
    value: &TemplateString,
    field: &'static str,
    rule: ValueRule,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    value
        .syntax()
        .map_err(|error| syntax_error(record, field, error))?;
    match value.literal() {
        Some(literal) => rule.check(literal).map_err(|source| ManifestError::Value {
            record: record.clone(),
            field,
            value: if rule.may_show(literal) {
                value.source().to_owned()
            } else {
                String::new()
            },
            source,
        }),
        None => Ok(()),
    }
}

/// Check a `run` action: its command parses, a stateful trigger has an `id` to keep state under,
/// and each `after` target is an action declared earlier, at `positions`, or a remote in
/// `remotes` that a [`ReadAs::Leaf`] manifest declares.
pub(super) fn check_run(
    action: &RunAction,
    record: &DeclarationName,
    positions: &BTreeMap<&ItemId, usize>,
    remotes: &BTreeMap<ItemId, Remote>,
    read_as: ReadAs,
) -> Result<(), ManifestError> {
    for part in action.command.parts() {
        check_template(part, "command", ValueRule::Text, record)?;
    }
    check_optional_template(action.cwd.as_ref(), "cwd", ValueRule::Destination, record)?;
    for value in action.env.values() {
        check_template(value, "env", ValueRule::Text, record)?;
    }
    if action.trigger.keeps_state() && action.id.is_none() {
        return Err(ManifestError::StatefulTriggerWithoutId {
            record: record.clone(),
            trigger: action.trigger.kind(),
        });
    }
    let Trigger::After(targets) = &action.trigger else {
        return Ok(());
    };
    for target in targets {
        match target {
            AfterTarget::Action(target) if action.id.as_ref() == Some(target) => {
                return Err(ManifestError::AfterTargetSelf {
                    record: record.clone(),
                    target: target.clone(),
                });
            }
            AfterTarget::Action(target) => match (positions.get(target), record) {
                (None, _) => {
                    return Err(ManifestError::AfterTargetUnknown {
                        record: record.clone(),
                        target: target.clone(),
                    });
                }
                (Some(&at), DeclarationName::Action(number)) if at > *number => {
                    return Err(ManifestError::AfterTargetLater {
                        record: record.clone(),
                        target: target.clone(),
                        at,
                    });
                }
                (Some(_), _) => {}
            },
            AfterTarget::Remote(remote) => match read_as {
                ReadAs::Included => {
                    return Err(ManifestError::IncludedAfterNamesRemote {
                        record: record.clone(),
                        remote: remote.clone(),
                    });
                }
                ReadAs::Leaf if !remotes.contains_key(remote) => {
                    return Err(ManifestError::AfterRemoteUndeclared {
                        record: record.clone(),
                        remote: remote.clone(),
                    });
                }
                ReadAs::Leaf => {}
            },
        }
    }
    Ok(())
}

/// Check a record's `when` and `unless`: at most one is written, and it parses.
pub(super) fn check_conditions(
    when: Option<&Condition>,
    unless: Option<&Condition>,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    if when.is_some() && unless.is_some() {
        return Err(ManifestError::BothConditions {
            record: record.clone(),
        });
    }
    for (spelling, condition) in [("when", when), ("unless", unless)] {
        if let Some(condition) = condition {
            condition
                .syntax()
                .map_err(|error| syntax_error(record, spelling, error))?;
        }
    }
    Ok(())
}

fn syntax_error(
    record: &DeclarationName,
    field: &'static str,
    error: &SyntaxError,
) -> ManifestError {
    ManifestError::Syntax {
        record: record.clone(),
        field,
        source: Box::new(error.clone()),
    }
}

/// [`check_template`] for a field that may be absent.
pub(super) fn check_optional_template(
    value: Option<&TemplateString>,
    field: &'static str,
    rule: ValueRule,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    value.map_or(Ok(()), |value| check_template(value, field, rule, record))
}

/// Required source kind: any file or directory, or a directory only.
#[derive(Debug, Clone, Copy)]
pub(super) enum SourceShape {
    Any,
    Directory,
}

/// Validate source syntax, declared remote references, and required source shape. File remotes
/// must be referenced as whole files. Do not check filesystem existence.
pub(super) fn check_source(
    written: &RepoPath,
    shape: SourceShape,
    record: &DeclarationName,
    remotes: &BTreeMap<ItemId, Remote>,
) -> Result<(), ManifestError> {
    let path = written.path();
    // An empty path with a remote ID can name a whole file remote; a plain empty source cannot.
    if written.remote().is_none() && path.is_empty() {
        return Err(ManifestError::SourceEmpty {
            record: record.clone(),
        });
    }
    if let Some(Remote::File(_)) = written.remote().and_then(|id| remotes.get(id)) {
        return match (path.is_empty(), shape) {
            (false, _) => Err(ManifestError::SourceInsideFileRemote {
                record: record.clone(),
                written: written.clone(),
            }),
            (true, SourceShape::Directory) => Err(ManifestError::SourceDirIsFileRemote {
                record: record.clone(),
                written: written.clone(),
            }),
            (true, SourceShape::Any) => Ok(()),
        };
    }
    if path.starts_with(REMOTE_PREFIX) {
        return Err(ManifestError::SourceStartsWithRemotePrefix {
            record: record.clone(),
            written: written.clone(),
        });
    }
    if is_anchored(Path::new(path)) {
        return Err(ManifestError::SourceNotRelative {
            record: record.clone(),
            written: written.clone(),
        });
    }
    match depth_within_tree(path) {
        None => {
            return Err(ManifestError::SourceOutsideTree {
                record: record.clone(),
                written: written.clone(),
            });
        }
        // `.` and paths such as `shell/..` resolve to the tree root.
        Some(0) => {
            return Err(ManifestError::SourceIsWholeTree {
                record: record.clone(),
                written: written.clone(),
            });
        }
        Some(_) => {}
    }
    if let Some(id) = written.remote()
        && !remotes.contains_key(id)
    {
        return Err(ManifestError::SourceRemoteUndeclared {
            record: record.clone(),
            written: written.clone(),
        });
    }
    Ok(())
}

/// Require the inclusion to reference a Git remote declared in the same manifest.
pub(super) fn check_inclusion_remote(
    remote: &ItemId,
    record: &DeclarationName,
    remotes: &BTreeMap<ItemId, Remote>,
) -> Result<(), ManifestError> {
    match remotes.get(remote) {
        Some(Remote::Git(_)) => Ok(()),
        Some(other) => Err(ManifestError::InclusionRemoteNotGit {
            record: record.clone(),
            remote: remote.clone(),
            kind: other.kind(),
        }),
        None => Err(ManifestError::InclusionRemoteUndeclared {
            record: record.clone(),
            remote: remote.clone(),
        }),
    }
}

/// Reject incompatible inclusion filters. At most one of `install-actions`, `install-groups`,
/// and `exclude-groups` may be present; `exclude-actions` cannot accompany `install-actions`.
pub(super) fn check_inclusion_filters(
    action: &IncludeRemoteAction,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    // A diagnostic names the conflicting pair in this order.
    let selectors = [
        ("install-actions", action.install_actions.is_some()),
        ("install-groups", action.install_groups.is_some()),
        ("exclude-groups", action.exclude_groups.is_some()),
    ];
    let mut written = selectors.iter().filter(|(_, present)| *present);
    let conflict = match (written.next(), written.next()) {
        (Some((one, _)), Some((other, _))) => Some((*one, *other)),
        (Some((one, _)), None) if *one == "install-actions" && action.exclude_actions.is_some() => {
            Some((*one, "exclude-actions"))
        }
        _ => None,
    };
    match conflict {
        None => Ok(()),
        Some((one, other)) => Err(ManifestError::InclusionFiltersConflict {
            record: record.clone(),
            one,
            other,
        }),
    }
}

/// An action read from an included manifest must not name a remote in its
/// source. Applied by [`Manifest::validate`](super::Manifest::validate) only
/// when reading an included manifest.
pub(super) fn check_included_source(
    written: &RepoPath,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    if written.remote().is_none() {
        Ok(())
    } else {
        Err(ManifestError::IncludedSourceNamesRemote {
            record: record.clone(),
            written: written.clone(),
        })
    }
}

/// Return whether the path starts with a root or platform prefix.
fn is_anchored(source: &Path) -> bool {
    matches!(
        source.components().next(),
        Some(Component::RootDir | Component::Prefix(_))
    )
}

/// How many components deep a relative path lands, or `None` if it does not stay
/// within the tree it is relative to.
fn depth_within_tree(source: &str) -> Option<usize> {
    let mut depth: usize = 0;
    for component in Path::new(source).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => depth = depth.checked_sub(1)?,
            Component::Normal(_) => depth += 1,
            // Unreachable behind `is_anchored`, and neither can appear later.
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(depth)
}

/// Reject empty destinations and unsupported `~user` prefixes.
fn check_dest(dest: &str) -> Result<(), ValueError> {
    if dest.is_empty() {
        return Err(ValueError::DestinationEmpty);
    }
    // Allow `~` and `~/...`, but do not expand other users' homes.
    match dest.strip_prefix('~') {
        Some(rest) if !rest.is_empty() && !rest.starts_with('/') => {
            Err(ValueError::DestinationOtherHome)
        }
        _ => Ok(()),
    }
}

/// Validate a fetch URL.
fn check_url(source: &str) -> Result<(), ValueError> {
    if is_http_url(source) {
        return Ok(());
    }
    match file_url_path(source) {
        Some(Ok(_)) => Ok(()),
        Some(Err(error)) => Err(ValueError::FileUrl(error)),
        None => Err(ValueError::NotAUrl),
    }
}

/// Whether `source` starts with `http://` or `https://`, in any case, and has more after it.
fn is_http_url(source: &str) -> bool {
    let scheme = |prefix: &str| {
        let (source, prefix) = (source.as_bytes(), prefix.as_bytes());
        source.len() > prefix.len() && source[..prefix.len()].eq_ignore_ascii_case(prefix)
    };
    scheme("http://") || scheme("https://")
}

/// Check a release lookup's base URL: `http://` or `https://`, then a host with no
/// credentials, then an optional path with no query or fragment, all of it a URI the HTTP
/// client can request.
fn check_release_host(value: &str) -> Result<(), ValueError> {
    if !is_http_url(value) {
        return Err(ValueError::NotAnHttpUrl);
    }
    let (_, rest) = value.split_once("://").unwrap_or_default();
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') {
        Err(ValueError::Credentials)
    } else if authority.is_empty() || authority.starts_with(':') {
        Err(ValueError::NoHost)
    } else if rest.contains(['?', '#']) {
        Err(ValueError::QueryOrFragment)
    } else if !requestable(value) {
        Err(ValueError::MalformedUrl)
    } else {
        Ok(())
    }
}

/// Whether the HTTP client parses `value` as a URI whose authority is a host and, if it has
/// one, a numeric port.
fn requestable(value: &str) -> bool {
    let Ok(uri) = value.parse::<ureq::http::Uri>() else {
        return false;
    };
    uri.authority().is_some_and(|authority| {
        authority.port_u16().is_some() || authority.as_str() == authority.host()
    })
}

/// Check a `[vars]` entry: a `github-url` parses, reads only `env` and `facts`, and, if it is
/// literal, is an http(s) URL.
pub(super) fn check_var(spec: &VarSpec, record: &DeclarationName) -> Result<(), ManifestError> {
    let VarSpec::Dynamic(DynamicVarSpec {
        source: DynamicSource::GithubRelease(release),
        ..
    }) = spec
    else {
        return Ok(());
    };
    let Some(url) = &release.github_url else {
        return Ok(());
    };
    check_template(url, "github-url", ValueRule::ReleaseHost, record)?;
    match url
        .roots()
        .into_iter()
        .find(|root| !HOST_NAMESPACES.contains(root))
    {
        Some(root) => Err(ManifestError::GithubUrlReadsVariable {
            record: record.clone(),
            root: root.to_owned(),
        }),
        None => Ok(()),
    }
}

/// The namespaces that exist before any variable resolves.
const HOST_NAMESPACES: [&str; 2] = ["env", "facts"];

/// Validate an optional archive-root path or the `*` automatic-root selector.
pub(super) fn check_archive_root(
    archive_root: Option<&str>,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    let Some(value) = archive_root else {
        return Ok(());
    };
    if value == "*" {
        return Ok(());
    }
    if names_a_path_inside_an_archive(value) {
        Ok(())
    } else {
        Err(ManifestError::ArchiveRootNotInside {
            record: record.clone(),
            value: value.to_owned(),
        })
    }
}

/// Refuse a pattern in a filter over direct child names that contains `/`.
pub(super) fn check_child_filter(
    field: &'static str,
    filter: Option<&GlobFilter>,
    record: &DeclarationName,
) -> Result<(), ManifestError> {
    match filter.and_then(GlobFilter::first_nested) {
        None => Ok(()),
        Some(pattern) => Err(ManifestError::ChildFilterNested {
            record: record.clone(),
            field,
            pattern: pattern.as_str().to_owned(),
        }),
    }
}

/// Whether a value is spelled the way a path inside an archive is: ordinary
/// components and `.`, with at least one of the former.
fn names_a_path_inside_an_archive(value: &str) -> bool {
    let mut named = 0;
    for component in Path::new(value).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(_) => named += 1,
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    named > 0
}

#[cfg(test)]
mod tests;
