# Writing documentation

Batfiles documentation serves two reading paths: guides for completing a task,
and references for looking up the full behavior. Both are ordinary Markdown
kept with the code. [AGENTS.md](../../AGENTS.md#documentation) assigns each rule
to one owning reference.

## Guides

Write for someone who knows what they want to accomplish but does not yet know
batfiles' vocabulary. Start with the outcome, then provide:

1. Prerequisites, including platform limitations.
2. A small, complete example.
3. Commands to run and the result to expect.
4. Caveats that affect the user's decision at this step.
5. Links to the exact reference sections for further detail.

Use one task per page. Introduce terms such as *seed*, *remote*, and *inclusion*
where an example needs them, and define new terms in the [glossary](../glossary.md). Prefer a useful default over presenting every
possible option. Guides summarize and demonstrate; the linked reference owns
the complete rules, including exceptions and precedence.

Keep consequential behavior beside the relevant instruction: replacement and
backup behavior, Windows symlink limitations, dry-run's dynamic commands and
bookkeeping, and the distinction between disabling and uninstalling. Do not
hide these in expandable sections. Longer explanations belong in the reference.

## References

Lead command and action pages with their purpose and a minimal example. Follow
with complete syntax or field tables, behavior, and related rules. Keep shared
policies in their owner and link to them from individual entries.

Detailed guarantees are useful even when most readers rarely need them. Move
them to the appropriate reference rather than deleting them to shorten a guide.
Implementation design belongs in [architecture](architecture.md); release
operator procedures belong in [release management](../../dist/README.md).

## Coverage

Use this inventory when reorganizing material. Account for every section of a
moved document, including examples and edge cases, and update its owner in
[AGENTS.md](../../AGENTS.md#documentation) in the same change.

| Material | Reader's question | Reference |
| --- | --- | --- |
| Commands, selection, output, dry-run, failure handling | What does this invocation do? | [Shared command rules](../cmdline.md), [command pages](../commands/sync.md) |
| Manifest schema, actions, remotes, conditions, clone lists | How do I declare it? | [Repository format](../repoformat.md), [action pages](../actions/copy.md) |
| Locations, environment, precedence, dynamic execution | Where does this value come from? | [Environment](../environment.md) |
| Conflicts, seeds, permissions, archives, Git updates | What happens to my existing content? | [Safety](../safety.md) |
| Persistent choices, caches, locks, atomic writes | What is stored on this machine? | [State](../state.md) |
| Installers, checkout stubs, release trees, hosting | How is batfiles installed and delivered? | [Installation](../installer.md), [distribution](distribution.md) |

## Links and examples

Use relative Markdown links between documentation pages and link to a specific
heading when it answers the question. After a move, update every inbound link
instead of leaving a pointer heading behind, and do not maintain a second copy
of the moved rules.

Use fenced code blocks with a language, such as `sh`, `powershell`, `toml`, or
`text`. Shell examples intended for copying omit prompt characters. Show
expected output separately, and explain quiet success where appropriate.
Use reserved example domains for illustrative network sources. Executable
examples and tests use isolated homes, state, and local repositories or servers;
they must not touch the reader's real dotfiles or require external services.

Documentation on `main` describes that revision. Label behavior that is not
yet in a stable release with a paragraph of its own, directly after the heading
or the text it qualifies:

```md
**Unreleased:** `--ref` requires a build newer than 0.1.0.
```

When the release ships, replace each label with the shipped version:

```md
**Since 0.2.0:** `--ref` requires 0.2.0 or newer.
```

A stable release cannot be tagged while any published page still carries the
Unreleased label; see [release management](../../dist/README.md#a-stable-release).
Do not describe ideas from [enhancements](enhancements.md) as supported behavior.

## Building and previewing

The documentation tasks need Linux or macOS. Each first runs
`task docs:install`, which downloads the mdBook and lychee releases pinned in
`dist/docs-tools`, verifies their SHA-256 digests, and installs them under
`target/docs-tools`. Tools already installed at the pinned versions are kept,
so only the first run, or one after a pin changes, needs the network. Neither
tool is a dependency of the batfiles binary.

- `task docs:build` writes HTML to `target/docs` without fetching a release.
- `task docs:serve` serves a local preview with live reload.
- `task docs:check` builds and checks local HTML links and repository Markdown
  links, including heading fragments. Links to this repository's files on
  GitHub's `main` are checked against the working tree. External sites are not
  contacted.

To change a pin, update every line for that tool in `dist/docs-tools` with the
new version and each release archive's digest, as the GitHub release lists it.

[book.toml](../../book.toml) configures rendering; [SUMMARY.md](../SUMMARY.md) orders
published pages. Add a new user page to that navigation. The tutorial,
`getting-started.md`, sits beside the index rather than in `guides/` so the
landing page and deployment verification have a short, stable URL for it. Contributor documents
live in `docs/contributing/` and stay outside the book; links to them from
published pages use their GitHub URLs. This also applies to source files and
fixtures outside `docs/`. Ordinary relative
`.md` links between published pages work both on GitHub and in the HTML output.
Put sample links that are not meant to resolve in code blocks or inline code;
the link check skips those.

`SITE_URL` selects the deployment prefix, including its trailing slash, for
example `task docs:check SITE_URL=/batfiles/docs/`. Previewing uses `/`.
[theme/docs.css](../../theme/docs.css) holds the book's style adjustments.
The full check is part of `task ci`.
[Distribution](distribution.md#the-site) owns complete-site assembly and publishing.
Generated HTML is never committed. The generator's configuration refuses
missing chapter files instead of creating placeholders.
