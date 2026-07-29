# Batfiles - Dotfiles Manager

A simple, composable dotfile manager. Users have a leaf repo containing their bootstrap config and an `install.sh` entry point. That repo can declare other dotfile repos to include, and the tool merges everything together and installs it into `$HOME`.

Design priorities: just files, composability across multiple sources, extensibility, no required interactivity, and minimal hidden state.

## Development

[go-task](https://taskfile.dev) drives everything; CI runs the same entry point.
The toolchain is pinned in `rust-toolchain.toml`.

```sh
task ci      # fmt + lint + test + deny + build + build:release (what CI runs)
task test    # project tests
task fmt     # formatting check
task lint    # clippy with warnings denied
task build   # debug build
```

Without `task` installed, the underlying commands are `cargo fmt --all`,
`cargo clippy --all-targets`, and `cargo test`.

## License

[MIT](./LICENSE)
