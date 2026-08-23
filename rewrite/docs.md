# What to Do With `docs/`

Handoff for whoever cuts the specification down. Today `docs/` is 2,424 lines
describing a tool that does not exist, and `CLAUDE.md` instructs every
implementer to honor it as settled. That instruction is correct in principle and
was ruinous in practice: the decisions were all made without a single line of
implementation feedback, and then bound the implementation anyway.

The fix is not to delete the specification. Most of it is careful and worth
keeping. The fix is to stop `docs/` from being a promise.

## The rule

**`docs/` describes behavior that runs. `docs/future/` describes everything
else, and binds nothing.**

A section moves from `docs/future/` into `docs/` in the same commit as the code
that implements it. Nothing is promoted by copy-paste: re-read it against what
was actually built, and change it where the build disagreed. The build wins —
that is the entire point of the split.

`CLAUDE.md`'s "if a decision is settled, honor it" then applies only to things
that actually run, which is what makes it a safe instruction.

## Disposition

Each entry below describes where a document ends up **once slice 0 is finished**,
not what 0.1 does in one move. The initial cut is blunt on purpose: everything
that specifies unbuilt behavior goes to `docs/future/` wholesale, and the
individual sections named below come back at the steps that build them —
`cmdline.md`'s at 0.2, `environment.md`'s at 0.3, `repoformat.md`'s at 0.4 and
0.6, `state.md`'s at 3.3, `safety.md`'s at 0.10. Promoting them at 0.1 instead
would inherit them, which is the thing rule 9 exists to stop.

- **`goals.md`** (148) — **keep in `docs/`.** The product definition, and the one
  document written at the right altitude. Two edits: replace the enumeration of
  nine action types with what exists plus a pointer to `docs/future/`, and move
  the `--refresh-content` and backup paragraph out until 9.4.

- **`architecture.md`** (107) — **deleted at 0.1; recreated at slice 8 from
  `rewrite/guidance.md`.** The two covered the same ground, and guidance.md is
  the version with the failure mode in it, so keeping both meant maintaining a
  weaker duplicate of the authoritative document for eight slices. It is gone
  from the tree meanwhile; `guidance.md` owns implementation shape, and
  `docs/README.md` says so.

  When it comes back it is the one home for durable design rules — no
  `allow(dead_code)` and its CI check, error types only where a caller matches on
  them, rationale in commit messages, shell out to `git`, dry-run never
  simulates, tests never reach the network, dogfood before shipping, and the
  manager serves the configuration. Leave behind the scaffolding: slice ordering,
  "a later slice may reshape an earlier one", and the seams list are all spent by
  then, and carrying them forward would make the permanent guidance read like a
  plan again.

- **`AGENTS.md`**, and `CLAUDE.md` which symlinks to it — **rewrite at slice 8**,
  in the same change that deletes `rewrite/`. It is the file agents auto-load, so
  leaving it stale is worse than leaving any document in `docs/` stale. Five
  edits: drop the "Status: rewrite in progress" block; replace "Where the
  documentation lives" with the `docs/` versus `docs/future/` rule above; cut
  "Source organization" down to a pointer at the recreated `architecture.md`;
  add `task test:docker` to the canonical commands if 8.4 added it; and carry
  `guidance.md`'s "How a slice lands" over next to the canonical commands —
  its three workflow paragraphs only. That last one is a change of mind about
  which list it belongs on: branch, commit as you go, squash to `main` is how
  the repository is worked rather than how the rewrite was, so it outlives the
  rewrite the way the toolchain pin does. Its closing sentence does not: nothing
  merging back from the salvage tag is a rule about the rewrite, and it points
  at `keep.md`, which this same slice deletes. Leave it behind with the
  directory rather than copying a link to a file that is going away.

  That third edit is the one to get right, and it is a change from how the file
  reads today. `AGENTS.md` currently states the source-organization rules *and*
  links to `architecture.md` for "the durable design guidance", so the same rules
  live in two files and drift independently. One of them has to be the pointer.
  `architecture.md` is the one that holds the rules, because it can hold them at
  full length; `AGENTS.md` stays short, which is what makes it worth
  auto-loading.

- **`repoformat.md`** (740) — **split.** The real specification and the most
  valuable document in the set. Keep in `docs/` only the sections whose fields
  parse today: the layout, the top-level schema, names and IDs, and the action
  variants that exist. Move the rest verbatim to `docs/future/repoformat.md`. At
  slice 0 that leaves a very short document, and it should. The layout came back
  at 0.4 with the reader, alongside the reading rules from `state.md`; the
  schema follows at 0.6 with the parser.

- **`cmdline.md`** (376) — **split.** Keep the command overview, global options,
  output streams, and exit statuses; true from slice 0. Move each per-command
  section to `docs/future/` and promote it at that command's step. Keep "Dry-Run
  Behavior" in `docs/future/` but read it before starting slice 2 — the
  complete-versus-partial distinction is the design, not a detail.

- **`environment.md`** (387) — **mostly `docs/future/`.** Keep the location
  selection and precedence for the four roots, which 0.3 implements. Everything
  about runtime variable precedence goes to `docs/future/` and returns in two
  pieces: the flat four-source order at 5.4, and the layered stack at 7.5. Do not
  promote the layered version early; it is the document that most directly caused
  `scope.rs`.

- **`state.md`** (333) — **`docs/future/`, promoted per file.** `disabled.toml` at
  3.3, `vars.toml` at 5.2, `dynamic-vars.toml` at 9.1. Its shared rules are
  cross-cutting and split by which half of `tomlfile.rs` they describe: the
  reading rules went with the reader at 0.4, into `docs/repoformat.md` because
  the leaf manifest is the only document read there, and the atomic
  whole-document rewrite rule goes with the writer at 3.3.

- **`safety.md`** (310) — **split, and this one matters most.** Three separate
  promotions, earlier than the old plan assumed:
  - Destination resolution and symlink traversal at **0.10**. A tool that writes
    to `$HOME` needs those rules before it writes anything, so 0.7 landed them
    inside `docs/repoformat.md` under `symlink`, written for one action type;
    0.10 lifts them to general statements before `create-dir` and `copy` arrive.
  - Archive handling — absolute paths, `..` traversal, symlinks escaping the
    destination root — at **4.2**. This is no longer a late concern; slice 4 is
    the first code that unpacks untrusted content.
  - Conservative Git updates at **4.5**, reused at 6.2. Backup and replacement
    policy stays in `docs/future/` until 9.4.

- **`docs/README.md`** (23) — the docs index. Rewrite last, once the split has
  settled.

- **The project `README.md`** — **rewrite at 0.16, and keep honest every slice
  after.** It currently describes `install.sh` as the entry point and the tool as
  merging multiple sources, none of which will be true again until slices 10 and
  7 respectively. It is the first thing a human reads and the only document here
  with an audience outside the project, so it is the one place where describing
  unbuilt behavior is not merely untidy but misleading.

**Nothing is lost to the tag.** `docs/future/` is committed and searchable, so
the safety reasoning, the precedence tables, and the per-command specifications
all stay in the working tree — they simply stop binding the implementation. The
only thing that goes to the tag and does not come back is the old `src/`, and
`keep.md` says which parts of it to retrieve and when.

## What to write down before deleting anything

Four things the old implementation learned that are in no document. Put each in
its owning doc as a few sentences, not as a design essay. **All four are
recorded as of 0.1** — 2 and 3 turned out to be in `repoformat.md` already, and
1 and 4 were added there and to `cmdline.md`. They travel with their section
when it is promoted; the list stays here so a reviewer can check they survived.

1. **Variables exist only to feed conditions.** No interpolation anywhere in the
   format. `repoformat.md` implies this but never says it, and saying it plainly
   is what makes the ordering in `steps.md` obviously correct.
2. **An unevaluable condition closes the gate in both spellings.** A typo'd
   `unless` would otherwise install the thing it was written to suppress. This
   lives in `reach.rs`'s module comment today and nowhere else; it belongs in
   `repoformat.md`'s condition section.
3. **Reserving `facts`, `env`, `vars`, `true`, and `false` is what makes
   namespace dispatch unambiguous.** Without it the resolver needs a precedence
   rule. `repoformat.md` lists the reserved names but not the reason, and the
   reason is what stops someone from trimming the list.
4. **A dry run may write to `remotes/` but never to `$HOME`.** This is what keeps
   an `include-remote` plan complete rather than unknowable, and it belongs in
   `cmdline.md`'s dry-run section when that is promoted at slice 2.

## Size check

Done right, the 0.1 cut leaves `docs/` under 300 lines and `docs/future/`
holding the rest. If `docs/` is still over a thousand lines, something
unimplemented is still in there.

That number measures the cut, not a ceiling. `docs/` grows again from 0.2
onward, one promoted section per step, and the thing to check thereafter is not
its size but whether every line in it describes something the binary does.
