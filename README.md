# Batfiles - Dotfiles Manager

A simple, composable dotfile manager. Users have a leaf repo containing their bootstrap config and an `install.sh` entry point. That repo can declare other dotfile repos to include, and the tool merges everything together and installs it into `$HOME`.

Design priorities: just files, composability across multiple sources, extensibility, no required interactivity, and minimal hidden state.

## Development

[go-task](https://taskfile.dev) drives everything; CI runs the same entry point.
The toolchain is pinned in `rust-toolchain.toml`.

```sh
task ci      # fmt + lint + test + deny (what CI runs)
task test    # workspace tests
task fmt     # formatting check
task lint    # clippy with warnings denied
```

Without `task` installed, the underlying commands are `cargo fmt --all`,
`cargo clippy --workspace --all-targets`, and `cargo test --workspace`.

## License

[MIT](./LICENSE)
