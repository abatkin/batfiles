# Specification for behavior that does not exist yet

**Nothing in this directory binds the implementation.**

These documents were written before the tool was built, and describing a
finished design in 2,400 lines with no implementation feedback is what caused
the rewrite. They are kept because most of the thinking in them is careful and
expensive to redo — not because the decisions in them are settled.

Read them for context and for the reasoning behind a design. Do not treat a
statement here as a requirement, and do not implement something merely because
it is written here.

## The rule for getting out of here

A section moves up into [`docs/`](../) in the same commit as the code that
implements it. Promotion is a re-read, not a copy: check the text against what
was actually built, and change the text where the build disagreed. Whatever is
promoted becomes binding; whatever is left behind stays advisory.

## The documents

- [Repository format](repoformat.md) — `batfiles.toml`, manifest schemas,
  shared names, IDs, and value types.
- [Command-line surface](cmdline.md) — the per-command specifications, the
  shared action and selection options, what the unbuilt actions add to dry-run
  behavior, and the address forms nothing can resolve yet. The parts that are
  built live in [`docs/cmdline.md`](../cmdline.md).
- [Environment variables](environment.md) — environment inputs, location
  selection, runtime variable precedence, and bootstrap precedence. The parts
  that are built live in [`docs/environment.md`](../environment.md).
- [Local state and cache files](state.md) — the schemas and lifecycle of
  `vars.toml` and the dynamic-variable cache, and the parts of `disabled.toml`
  that need remotes or bootstrap to mean anything. The parts that are built live
  in [`docs/state.md`](../state.md).
- [Safety model](safety.md) — trust boundaries, destination resolution,
  replacement and backup policy, conservative Git updates, archive handling,
  and failure recovery.
