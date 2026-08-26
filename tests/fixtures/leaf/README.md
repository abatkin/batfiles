# Fixture: a leaf repository

Nothing in the manifest names this file. It is here because a real dotfiles
repository has files that are not actions, and batfiles has to leave them alone.

The manifest declares every action type that exists, over a tree shaped like one
someone would keep: `shell/`, `git/`, `editor/`, and `bin/` are linked,
`files/` is linked a child at a time, and `templates/` and `zsh-local/` are
seeded, because what they hold is meant to be edited where it lands.
