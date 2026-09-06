# Documentation ownership

`docs/` specifies implemented behavior. `docs/future/` contains unbuilt proposals
and binds no implementation. Product goals may describe intended scope when
clearly distinguished from supported behavior.

Promote a section in the change implementing it. Check it against the code,
rewrite discrepancies, and remove the duplicated proposal. Leave a link where
future work depends on the implemented behavior.

## Owners

| Subject | Owner |
| --- | --- |
| Product overview, quick start, supported features | Project `README.md` |
| Product goals and intended scope | `docs/goals.md` |
| Command syntax, output, selection, dry-run, exit statuses | `docs/cmdline.md` |
| Environment parsing and location/color precedence | `docs/environment.md` |
| Manifest schema, action fields, clone-list syntax | `docs/repoformat.md` |
| Destination safety, seed installation, archive safety, Git update policy | `docs/safety.md` |
| State schemas, lifecycle, and atomic document replacement | `docs/state.md` |
| Implementation design | `rewrite/guidance.md` |
| Filesystem-owner inventory | `tests/hygiene.rs` |
| Work order and outstanding acceptance | `rewrite/steps.md` |
| Branch workflow and canonical commands | `AGENTS.md`, linking to rewrite workflow while active |

Specify each rule once and link to its owner. Field tables may summarize a
shared policy with a link. API comments describe caller contracts, not the
history or justification of an implementation. Future documents contain only
remaining proposals and their dependencies.

## Retirement at slice 8

1. Move durable design guidance into `docs/architecture.md`: source organization,
   caller requirements, concrete errors, comment conventions, Git invocation,
   dry-run structure, path and installation safety, testing, and small validated
   types. Preserve links to the documents owning user-visible policy.
2. Rewrite `AGENTS.md` to link to that design guidance. Keep workflow, the
   documentation ownership rule, canonical commands, and the toolchain pin
   there. Add `task test:docker` when 8.4 implements it. Remove the rewrite
   status and precedence override.
3. Move remaining numbered steps 9 and 10 and named enhancements into
   `docs/future/roadmap.md`. Preserve identifiers used by unsupported options or
   other live references. Remove completed scaffolding.
4. Repoint project and documentation links. Update hygiene checks to read the
   remaining roadmap for live step references; retire only checks with no
   remaining purpose. Unsupported options still require step validation.
5. Delete `rewrite/` after confirming that links and step references resolve.
