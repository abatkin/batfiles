# Architecture

> **Superseded for the duration of the rewrite.**
> [`rewrite/guidance.md`](../rewrite/guidance.md) covers the same ground and is
> the version with the failure mode in it. Where the two disagree, follow
> guidance.md. This file is replaced by it at slice 8; until then parts of it
> describe a source tree that no longer exists — `trace`, below, is cut.

Batfiles is one Cargo package that produces one `batfiles` executable. Its
implementation should remain proportionate to a small command-line tool while
keeping its rules understandable and testable.

## Source shape

Use ordinary Rust modules to group cohesive behavior.

`main` should remain a small entry point. `app` may perform straightforward
command orchestration. Configuration, planning, execution, and presentation
code should live near the data and operations they use rather than being
separated solely to enforce dependency direction.

`cli` owns everything specific to the command line. That is the clap argument
definitions, and also the two things that exist only to serve them: `color`,
which scans the raw arguments and resolves `--color` into the `ColorChoice`
clap needs back before it can render help or a usage error, and `trace`, which
renders a parsed invocation for verbose output and therefore holds the one
exhaustive match over every command. Both name the argument types; the argument
definitions name neither, so they stay free of formatting.

`output` deliberately stays outside `cli`. The reporter is the diagnostic sink
every subsystem writes to and it depends on no argument parsing, so filing it
under `cli` would imply a dependency that does not exist.

### Visibility

Module-level items are `pub(crate)`; fields and inherent methods are `pub`.
The crate builds one binary, so the two spellings behave identically — the
convention exists only so the source reads consistently.

## Data and control flow

The main command flow is:

```text
parse inputs → validate values → build an ordered plan → execute the plan
```

Keep this flow visible through concrete types and functions. Validated values
such as variable names and concrete planning records are useful because they
make invariants explicit. They do not need a separate package or a conversion
layer to be valuable.

On-disk records and runtime data may use the same type when their fields and
invariants match. Use a separate representation only when parsing concerns,
validation, normalization, or source-aware diagnostics make the distinction
useful.

Planning remains a meaningful behavioral boundary: build the complete
knowable structural plan before action execution, as specified in the product
and safety documents. Express the plan with concrete data and functions.
Execution may call filesystem, Git, network, archive, clock, and subprocess
helpers directly.

## Abstractions

Prefer concrete types and direct function calls. A new trait, adapter, wrapper,
or duplicate input/output type should solve a current problem, not preserve an
abstract layering rule.

Good test seams include:

- passing explicit input data to a pure calculation;
- passing a small closure for a nondeterministic value such as the detected
  home directory or current time;
- using fixtures and temporary directories for filesystem behavior;
- invoking local commands or local test servers for process and network
  behavior; and
- using a small trait when meaningful polymorphism or an otherwise impractical
  test boundary makes it the clearest option.

Local implementation traits that make nearby code clearer are fine. The
standard is whether an abstraction reduces the total complexity of the current
code.

## Dependencies and additional crates

Declare a dependency when implemented code uses it. Do not retain dependencies
for planned features.

An additional crate requires a concrete reason, such as an independently
reusable or publishable component with a stable interface. Internal categories
such as configuration, policy, I/O, and presentation are modules by default.

## Source of truth

The other documents in this directory own product behavior: command-line
semantics, configuration formats, precedence, state, and safety. This document
owns implementation shape. When either changes, update the owning document in
the same change.

## Additional guiding principles

- Build one executable crate, organized with cohesive modules rather than
  independently layered packages.
- Prefer concrete data and direct function calls over infrastructure that moves
  information between artificial boundaries.
- Introduce an abstraction only when it solves a current implementation or
  testing problem clearly enough to justify its cost.
- Keep policy testable with typed values, pure calculations where useful,
  fixtures, and temporary directories.
- Add dependencies and source structure when implemented behavior needs them,
  not in anticipation of possible future work.

