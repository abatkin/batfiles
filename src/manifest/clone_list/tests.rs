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
