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
- [Architecture](architecture.md) defines the implementation shape and the
  standard for introducing modules and abstractions.

## What is not here yet

`docs/future/` holds the repository format, the command-line surface, the
environment inputs, the local state files, and the safety model. Each is
promoted here in pieces, at the step that builds the piece.

Rules should be specified in their owning document and linked from the others.
This keeps safety-sensitive behavior such as precedence, dry-run execution, and
cache mutation from drifting between separate descriptions.

While the rewrite is in progress, [`rewrite/`](../rewrite/) outranks this
directory. See [`rewrite/docs.md`](../rewrite/docs.md) for what gets promoted
where.
