# Batfiles Documentation

**`docs/` describes behavior that runs. [`docs/future/`](future/) describes
everything else, and binds nothing.**

That split is the whole organizing rule here. A section moves from
`docs/future/` into `docs/` in the same commit as the code that implements it,
and it is re-read against what was actually built on the way — not copy-pasted.
Where the build disagreed with the specification, the build wins.

So a rule you find in this directory is settled and should be honored. A rule
you find in `docs/future/` is a decision made without implementation feedback,
kept because it is careful and worth having, and free to change the moment
something real contradicts it.

## What is here

- [Product goals](goals.md) defines the product model, guiding principles, and
  intended scope. It describes the whole product rather than the built subset,
  and says so where the two differ.
- [Command-line surface](cmdline.md) defines the commands, the global options,
  where output goes, and what an exit status means.
- [Environment variables](environment.md) defines the environment inputs
  batfiles reads.
- [Repository format](repoformat.md) defines where the leaf manifest lives, how
  it is read, and what it may declare.
- [Local state files](state.md) defines `disabled.toml`, and how every document
  batfiles owns is rewritten.

## What is not here yet

`docs/future/` holds the safety model, the two state files that do not exist
yet, and the unbuilt parts of the manifest schema, the command-line surface, and
the environment inputs. Each is promoted here in pieces, at the step that builds
the piece.

There is no `architecture.md` for the duration of the rewrite.
[`rewrite/guidance.md`](../rewrite/guidance.md) owns implementation shape, and
covers the same ground with the failure mode that caused the rewrite written
into it. It becomes `architecture.md` at slice 8.

Rules should be specified in their owning document and linked from the others.
This keeps safety-sensitive behavior such as precedence, dry-run execution, and
cache mutation from drifting between separate descriptions.

While the rewrite is in progress, [`rewrite/`](../rewrite/) outranks this
directory. See [`rewrite/docs.md`](../rewrite/docs.md) for what gets promoted
where.
