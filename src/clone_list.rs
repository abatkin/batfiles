//! Parse and prepare line-oriented `git-clone-list` manifests.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use thiserror::Error;

use crate::condition::{Bindings, Condition, ConditionError, Exclusion, Gate};
use crate::error::Error;
use crate::item::{ItemId, ItemIdError};
use crate::manifest::action::GitCloneListAction;

/// One repository the list names.
#[derive(Debug)]
pub(crate) struct CloneListEntry {
    /// The repository, passed to git as written; not necessarily a URL.
    pub repository: String,
    /// The one directory component the clone lands in.
    pub dest_name: String,
    /// Identifies the entry in diagnostics. Individual entry selection is not
    /// implemented; see the clone-list address enhancement in the roadmap.
    pub id: Option<ItemId>,
    /// The branch, tag, or commit the entry follows.
    pub git_ref: Option<String>,
    /// The condition admitting the entry, if it is written with one.
    pub when: Option<Condition>,
    /// The condition excluding it. An entry writes at most one of the two.
    pub unless: Option<Condition>,
    /// The list line declaring it, for failures after the list was read.
    pub line: usize,
}

impl CloneListEntry {
    /// Return the entry's condition gate, if declared.
    pub fn gate(&self) -> Option<Gate<'_>> {
        Gate::from_fields(self.when.as_ref(), self.unless.as_ref())
    }

    /// Format the entry's list, line, and optional ID for diagnostics. `list` is the action's
    /// source as declared.
    pub fn written_at(&self, list: &str) -> String {
        match &self.id {
            Some(id) => format!("id={id}, {list} line {}", self.line),
            None => format!("{list} line {}", self.line),
        }
    }
}

/// A validated clone list with evaluated entry conditions. An empty list declares no
/// repositories.
pub(crate) struct PreparedList<'a> {
    /// The action that declares the list and its destination directory.
    action: &'a GitCloneListAction,
    entries: Vec<PreparedEntry>,
}

/// A parsed clone-list entry with its evaluated condition.
pub(crate) struct PreparedEntry {
    /// The parsed entry.
    pub declared: CloneListEntry,
    /// The condition excluding this entry, or `None` if it may be cloned.
    pub exclusion: Option<Exclusion>,
}

impl<'a> PreparedList<'a> {
    /// Read and validate the clone list at `path` for `action`, evaluating entry conditions
    /// against `bindings`. Missing or malformed lists return an error.
    pub fn prepare(
        action: &'a GitCloneListAction,
        path: &Path,
        bindings: &Bindings<'_>,
    ) -> Result<Self, Error> {
        let entries = read(path)?
            .into_iter()
            .map(|declared| PreparedEntry {
                exclusion: exclusion(&declared, bindings),
                declared,
            })
            .collect();
        Ok(Self { action, entries })
    }

    /// The directory the clones are made in, as the record wrote it.
    pub fn dest_dir(&self) -> &str {
        &self.action.dest_dir
    }

    /// The list source as declared, including any remote reference.
    pub fn name(&self) -> String {
        self.action.source.to_string()
    }

    /// Whether the list declares no repositories at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry, in list order.
    pub fn entries(&self) -> &[PreparedEntry] {
        &self.entries
    }
}

/// Evaluate an entry's condition, returning `None` when it may be cloned.
/// The caller supplies the `not cloning` prefix when reporting an exclusion.
fn exclusion(entry: &CloneListEntry, bindings: &Bindings<'_>) -> Option<Exclusion> {
    entry.gate()?.exclusion(bindings, None)
}

/// Read and check one list.
fn read(path: &Path) -> Result<Vec<CloneListEntry>, Error> {
    let text = fs::read_to_string(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse(&text).map_err(|(line, source)| Error::CloneList {
        path: path.to_path_buf(),
        line,
        source,
    })
}

/// Every entry the text declares, or the first fault and the line it is on.
fn parse(text: &str) -> Result<Vec<CloneListEntry>, (usize, CloneListError)> {
    let mut entries: Vec<CloneListEntry> = Vec::new();
    let mut names: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut ids: BTreeMap<String, usize> = BTreeMap::new();

    for (index, text) in text.lines().enumerate() {
        let line = index + 1;
        let Some(entry) = entry(text, line).map_err(|invalid| (line, invalid))? else {
            continue;
        };

        if let Some((first, written)) =
            names.insert(fold(&entry.dest_name), (line, entry.dest_name.clone()))
        {
            return Err((
                line,
                if written == entry.dest_name {
                    CloneListError::RepeatedName {
                        name: entry.dest_name,
                        first,
                    }
                } else {
                    CloneListError::AliasedName {
                        name: entry.dest_name,
                        written,
                        first,
                    }
                },
            ));
        }
        if let Some(id) = &entry.id
            && let Some(first) = ids.insert(id.to_string(), line)
        {
            return Err((
                line,
                CloneListError::RepeatedEntryId {
                    id: id.clone(),
                    first,
                },
            ));
        }
        entries.push(entry);
    }
    Ok(entries)
}

/// Parse a line; return `None` for blank or comment-only lines.
fn entry(text: &str, line: usize) -> Result<Option<CloneListEntry>, CloneListError> {
    let mut fields = fields(text)?.into_iter();
    let Some(repository) = fields.next() else {
        return Ok(None);
    };
    if repository.is_empty() {
        return Err(CloneListError::RepositoryEmpty);
    }

    let mut metadata: BTreeMap<String, String> = BTreeMap::new();
    for field in fields {
        let Some((key, value)) = field.split_once('=') else {
            return Err(CloneListError::NotMetadata { field });
        };
        match key {
            "id" | "ref" | "dest-name" | "when" | "unless" => {}
            _ => {
                return Err(CloneListError::UnknownKey {
                    key: key.to_owned(),
                });
            }
        }
        if value.is_empty() {
            return Err(CloneListError::EmptyValue {
                key: key.to_owned(),
            });
        }
        if metadata.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(CloneListError::RepeatedKey {
                key: key.to_owned(),
            });
        }
    }

    if metadata.contains_key("when") && metadata.contains_key("unless") {
        return Err(CloneListError::BothConditions);
    }

    let name = match metadata.get("dest-name") {
        Some(written) => {
            if !is_one_component(written) {
                return Err(CloneListError::DestNameUnusable {
                    name: written.clone(),
                });
            }
            written.clone()
        }
        None => {
            let derived = derive_name(&repository);
            if !is_one_component(&derived) {
                return Err(CloneListError::DerivedNameUnusable {
                    repository,
                    name: derived,
                });
            }
            derived
        }
    };

    Ok(Some(CloneListEntry {
        repository,
        dest_name: name,
        id: metadata
            .get("id")
            .map(|id| ItemId::try_from(id.clone()))
            .transpose()?,
        git_ref: metadata.get("ref").cloned(),
        when: condition(&metadata, "when")?,
        unless: condition(&metadata, "unless")?,
        line,
    }))
}

/// Parse an optional condition from entry metadata.
fn condition(
    metadata: &BTreeMap<String, String>,
    key: &str,
) -> Result<Option<Condition>, CloneListError> {
    metadata
        .get(key)
        .map(|source| Condition::new(source))
        .transpose()
        .map_err(CloneListError::from)
}

/// The directory an entry clones into when it does not name one.
fn derive_name(repository: &str) -> String {
    let trimmed = repository.trim_end_matches('/');
    let tail = match trimmed.rfind(['/', ':']) {
        Some(at) => &trimmed[at + 1..],
        None => trimmed,
    };
    // Keep a bare `.git` name for destination-name validation to reject.
    match tail.strip_suffix(".git") {
        Some(stripped) if !stripped.is_empty() => stripped.to_owned(),
        _ => tail.to_owned(),
    }
}

/// Lowercase a destination name for portable collision detection.
fn fold(name: &str) -> String {
    name.to_lowercase()
}

/// Accept a nonempty destination name without path separators or a drive colon.
/// Reject `.`, `..`, and `.git`.
fn is_one_component(name: &str) -> bool {
    !name.is_empty() && !matches!(name, "." | ".." | ".git") && !name.contains(['/', '\\', ':'])
}

/// Split a line into whitespace-separated fields, honoring quotes and escapes.
/// An unquoted `#` starts a comment. Empty quoted fields are retained.
fn fields(line: &str) -> Result<Vec<String>, CloneListError> {
    let mut fields = Vec::new();
    let mut current = String::new();
    // Track empty quoted fields separately from absent fields.
    let mut started = false;
    let mut characters = line.chars();

    while let Some(character) = characters.next() {
        match character {
            '#' => break,
            _ if character.is_whitespace() => {
                if started {
                    fields.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            '\'' | '"' => {
                started = true;
                quoted(character, &mut characters, &mut current)?;
            }
            _ => {
                started = true;
                current.push(character);
            }
        }
    }
    if started {
        fields.push(current);
    }
    Ok(fields)
}

/// Read a quoted run into `value`, stopping at the quote that opened it.
fn quoted(
    opener: char,
    characters: &mut std::str::Chars<'_>,
    value: &mut String,
) -> Result<(), CloneListError> {
    while let Some(character) = characters.next() {
        match character {
            _ if character == opener => return Ok(()),
            '\\' => match characters.next() {
                Some(escaped @ ('\\' | '"' | '\'')) => value.push(escaped),
                Some(escaped) => return Err(CloneListError::UnsupportedEscape { escape: escaped }),
                None => return Err(CloneListError::UnterminatedQuote),
            },
            _ => value.push(character),
        }
    }
    Err(CloneListError::UnterminatedQuote)
}

/// Invalid clone-list syntax or conflicting entries.
#[derive(Debug, Error)]
pub(crate) enum CloneListError {
    /// An empty repository field.
    #[error("names no repository")]
    RepositoryEmpty,

    /// A field after the repository that is not `key=value`.
    #[error("has `{field}` after the repository, which is not `key=value` metadata")]
    NotMetadata { field: String },

    #[error("uses the unknown key `{key}`; the keys are id, ref, dest-name, when, and unless")]
    UnknownKey { key: String },

    /// An invalid `when` or `unless` expression, with the parser's diagnostic.
    #[error(transparent)]
    Condition(#[from] ConditionError),

    /// An entry declaring both `when` and `unless`.
    #[error("writes both `when` and `unless`; a line has one condition or none")]
    BothConditions,

    #[error("writes `{key}` twice")]
    RepeatedKey { key: String },

    #[error("writes `{key}=` with no value")]
    EmptyValue { key: String },

    /// A quoted value with no closing quote.
    #[error("opens a quote that never closes")]
    UnterminatedQuote,

    /// An unsupported backslash escape in a quoted value.
    #[error("writes the escape `\\{escape}`; only `\\\\`, `\\\"`, and `\\'` are escapes")]
    UnsupportedEscape { escape: char },

    /// An invalid entry ID.
    #[error(transparent)]
    EntryId(#[from] ItemIdError),

    /// A repository whose final component cannot serve as a destination directory name.
    #[error(
        "names `{repository}`, which gives `{name}` as a directory name; \
         write `dest-name=` to say what to call the clone"
    )]
    DerivedNameUnusable { repository: String, name: String },

    /// A `dest-name` that is a path rather than a name.
    #[error("writes `dest-name={name}`, which is not one ordinary directory name")]
    DestNameUnusable { name: String },

    /// Two entries installing into one directory.
    #[error("clones into `{name}`, which line {first} already clones into")]
    RepeatedName { name: String, first: usize },

    /// Two entries whose names differ only in case.
    #[error(
        "clones into `{name}`, which differs only in case from the `{written}` on line {first}; \
         on a filesystem that ignores case they are one directory"
    )]
    AliasedName {
        name: String,
        written: String,
        first: usize,
    },

    /// Two entries declaring the same ID.
    #[error("repeats the id `{id}`, which line {first} already uses")]
    RepeatedEntryId { id: ItemId, first: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(text: &str) -> Vec<CloneListEntry> {
        parse(text).expect("the list should parse")
    }

    fn fault(text: &str) -> (usize, CloneListError) {
        parse(text).expect_err("the list should be refused")
    }

    /// Parse one repository entry and return its derived destination name.
    fn name_of(repository: &str) -> String {
        let parsed = entries(repository);
        parsed.into_iter().next().expect("one entry").dest_name
    }

    #[test]
    fn a_name_is_the_last_component_without_its_git_suffix() {
        assert_eq!(name_of("https://github.com/mileszs/ack.vim.git"), "ack.vim");
        assert_eq!(
            name_of("https://github.com/vim-airline/vim-airline"),
            "vim-airline"
        );
        assert_eq!(name_of("https://github.com/user/repo.git/"), "repo");
        assert_eq!(name_of("/srv/git/repo"), "repo");
    }

    #[test]
    fn a_name_ends_at_a_colon_where_that_is_the_last_separator() {
        // The `scp`-style form, which has no slash to end on.
        assert_eq!(name_of("git@github.com:repo.git"), "repo");
        assert_eq!(name_of("git@github.com:user/repo.git"), "repo");
        // A port, whose colon comes before the last slash and decides nothing.
        assert_eq!(name_of("ssh://git@host:2222/user/repo.git"), "repo");
    }

    #[test]
    fn a_repository_whose_name_is_not_a_directory_asks_for_dest_name() {
        for repository in ["git@host:", "C:\\src\\repo", "https://e.example/.git"] {
            assert!(
                matches!(
                    fault(repository),
                    (1, CloneListError::DerivedNameUnusable { .. })
                ),
                "`{repository}` should not name a directory"
            );
        }
    }

    #[test]
    fn a_dest_name_replaces_the_derived_one_and_is_still_only_a_name() {
        assert_eq!(
            name_of("https://github.com/romkatv/powerlevel10k.git dest-name=p10k"),
            "p10k"
        );
        for name in ["../elsewhere", "/etc", "a/b", "..", ".", ".git", "C:x"] {
            let written = format!("https://e.example/a.git dest-name='{name}'");
            assert!(
                matches!(
                    fault(&written),
                    (1, CloneListError::DestNameUnusable { .. })
                ),
                "`{name}` should not be a destination name"
            );
        }
    }

    #[test]
    fn blank_lines_and_comments_declare_nothing() {
        let parsed = entries(
            "# a whole-line comment, with an = and a \"quote\" in it\n\
             \n   \n\
             https://e.example/a.git # trailing, id=not-metadata\n\
             \t# indented\n",
        );
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].dest_name, "a");
    }

    #[test]
    fn a_fault_is_reported_against_the_line_it_is_written_on() {
        assert!(matches!(
            fault(
                "# a comment\n\
                 \n\
                 https://e.example/a.git\n\
                 https://e.example/b.git colour=blue\n"
            ),
            (4, CloneListError::UnknownKey { .. })
        ));
    }

    #[test]
    fn a_value_may_be_quoted_and_hold_what_a_bare_one_cannot() {
        let parsed = entries("https://e.example/a.git dest-name=\"my plugin\"\n");
        assert_eq!(parsed[0].dest_name, "my plugin");
        assert_eq!(name_of("https://e.example/a.git dest-name='a#b'"), "a#b");
    }

    #[test]
    fn a_ref_is_accepted_in_the_shapes_git_resolves() {
        assert_eq!(entries("https://e.example/a.git ref=main\n").len(), 1);
        assert_eq!(
            entries("https://e.example/a.git ref='refs/heads/main'\n").len(),
            1
        );
    }

    #[test]
    fn a_line_splits_into_fields_at_whitespace_a_quote_does_not_cover() {
        assert_eq!(
            fields(r#"url dest-name="a\"b\\c" ref='x y' # "not a field""#).expect("fields"),
            ["url", r#"dest-name=a"b\c"#, "ref=x y"]
        );
        // Adjacent quoted and bare runs are one field, which is what makes
        // `key="value"` a field rather than two.
        assert_eq!(fields(r#"a'b'"c"d"#).expect("fields"), ["abcd"]);
        assert_eq!(fields("   ").expect("fields"), Vec::<String>::new());
    }

    #[test]
    fn the_only_escapes_are_the_quotes_and_the_backslash() {
        assert!(matches!(
            fault("https://e.example/a.git ref=\"a\\nb\"\n"),
            (1, CloneListError::UnsupportedEscape { escape: 'n' })
        ));
        assert!(matches!(
            fault("https://e.example/a.git ref=\"unfinished\n"),
            (1, CloneListError::UnterminatedQuote)
        ));
    }

    #[test]
    fn a_field_after_the_repository_has_to_be_metadata() {
        assert!(matches!(
            fault("https://e.example/a.git https://e.example/b.git\n"),
            (1, CloneListError::NotMetadata { .. })
        ));
        assert!(matches!(
            fault("https://e.example/a.git colour=blue\n"),
            (1, CloneListError::UnknownKey { .. })
        ));
        assert!(matches!(
            fault("https://e.example/a.git ref=\n"),
            (1, CloneListError::EmptyValue { .. })
        ));
        assert!(matches!(
            fault("https://e.example/a.git ref=main ref=next\n"),
            (1, CloneListError::RepeatedKey { .. })
        ));
        assert!(matches!(
            fault("'' id=a\n"),
            (1, CloneListError::RepositoryEmpty)
        ));
    }

    #[test]
    fn an_entry_takes_one_condition_in_either_spelling() {
        let parsed = entries("https://e.example/a.git when=\"work\"\n");
        assert_eq!(
            parsed[0].gate().expect("a gate").exclusion_reason(),
            "when \"work\" is false"
        );
        let parsed = entries("https://e.example/a.git unless=\"facts.os == 'windows'\"\n");
        assert!(parsed[0].when.is_none());
        assert!(parsed[0].unless.is_some());
    }

    #[test]
    fn an_entry_writes_one_condition_or_none() {
        assert!(matches!(
            fault("https://e.example/a.git when=\"work\" unless=\"school\"\n"),
            (1, CloneListError::BothConditions)
        ));
    }

    #[test]
    fn a_malformed_condition_is_refused_where_the_line_is_read() {
        let (line, fault) = fault(
            "https://e.example/a.git\n\
             https://e.example/b.git when=\"work &&\"\n",
        );
        assert_eq!(line, 2);
        assert!(
            matches!(fault, CloneListError::Condition(_)),
            "{fault:?}: a condition that does not parse should be refused"
        );
        assert!(
            fault.to_string().contains("is not a valid condition"),
            "{fault}"
        );
    }

    #[test]
    fn an_entry_id_follows_the_id_rule_and_not_the_directory_one() {
        assert_eq!(
            entries("https://e.example/a.git id=p10k\n")[0]
                .id
                .as_ref()
                .expect("an id")
                .as_str(),
            "p10k"
        );
        // Dots are valid in directory names but separate address segments in IDs.
        assert!(matches!(
            fault("https://github.com/mileszs/ack.vim.git id=ack.vim\n"),
            (1, CloneListError::EntryId(_))
        ));
    }

    #[test]
    fn two_entries_may_not_claim_one_directory_or_one_id() {
        assert!(matches!(
            fault(
                "https://github.com/user/repo.git\n\
                 https://gitlab.example/user/repo\n"
            ),
            (2, CloneListError::RepeatedName { first: 1, .. })
        ));
        assert!(matches!(
            fault(
                "https://e.example/a.git id=plugin\n\
                 https://e.example/b.git id=plugin\n"
            ),
            (2, CloneListError::RepeatedEntryId { first: 1, .. })
        ));
        assert_eq!(
            entries(
                "https://e.example/a.git\n\
                 https://e.example/a.git dest-name=second\n"
            )
            .len(),
            2
        );
    }

    #[test]
    fn two_names_that_differ_only_in_case_are_refused_on_every_platform() {
        let (line, fault) = fault(
            "https://e.example/Plugin.git\n\
             https://elsewhere.example/plugin.git\n",
        );
        assert_eq!(line, 2);
        assert!(
            matches!(&fault, CloneListError::AliasedName { written, first: 1, .. } if written == "Plugin"),
            "{fault:?}"
        );
        let said = fault.to_string();
        assert!(
            said.contains("`plugin`") && said.contains("`Plugin`"),
            "{said}"
        );

        assert_eq!(
            entries(
                "https://e.example/YouCompleteMe.git\n\
                 https://e.example/youcompleteme-config.git\n"
            )
            .len(),
            2
        );
    }

    #[test]
    fn a_real_list_reads_as_the_shell_script_read_it() {
        let parsed = entries(
            "https://github.com/zsh-users/zsh-syntax-highlighting.git\n\
             https://github.com/vim-airline/vim-airline\n",
        );
        assert_eq!(
            parsed
                .iter()
                .map(|entry| &entry.dest_name)
                .collect::<Vec<_>>(),
            ["zsh-syntax-highlighting", "vim-airline"]
        );
    }
}
