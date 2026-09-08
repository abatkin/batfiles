# Reference code for remaining steps

Read reference implementations from tag `before-rewrite-20260819` as needed.
Paths below refer to that tag. Do not merge from it. Adapt code to current
callers, error handling, and [guidance.md](guidance.md); do not import unused
layers or tests for unbuilt behavior.

| Step | Reference | Useful contract |
| --- | --- | --- |
| 5.1 | `src/var.rs` | Small `VarName` validator and reserved names |
| 5.2 | `src/state/vars.rs`, variable command tests in `tests/cli.rs` | State editing; requested data stays on stdout, even with quiet output |
| 5.3 | `src/config/env.rs`, `src/cli/options.rs` | One-shot variables and `NAME=VALUE` validation |
| 5.6 | `src/repo/default_disabled.rs` | Condition fields on bootstrap entries |
| 6.1 | `src/repo/remote.rs` | Git remote record |
| 6.3 | `src/repo/value.rs` | Parsed repository paths |
| 7.3 | `src/repo/value.rs` | `GlobFilter` and `ItemIdList` |
| 8.1 | `src/init.rs` | Initialization and worktree detection; use the current Git launcher |
| 9.1 | `src/repo/duration.rs`, `src/repo/var_decl.rs`, `src/state/dynamic_vars.rs` | Duration and dynamic-variable records |
| 9.3 | `src/repo/remote.rs` | File and archive remote records |

Preserve hand-written serde visitors for short/long value forms: choose the
form by TOML type and retain specific errors and locations.

Use the existing `Reporter`, TOML read/write helpers, Git launcher, selection,
and execution loop. Add data output when the variable commands need it. The
`install.sh` template belongs to slice 10, separate from `init` in slice 8.

Design remote loading and per-inclusion variable scopes around their current
callers. Do not import the reference `reach`, `scope`, `dynamic`, or repository
model/loading layers wholesale.
