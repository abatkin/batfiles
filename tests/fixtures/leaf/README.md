# Fixture: a leaf repository

Nothing in the manifest names this file. It is here because a real dotfiles
repository has files that are not actions, and batfiles has to leave them alone.

The manifest declares every action type that exists, over a tree shaped like one
someone would keep: `shell/`, `git/`, `editor/`, and `bin/` are linked,
`files/` is linked a child at a time, all but its own README, and `templates/` and `zsh-local/` are
seeded, because what they hold is meant to be edited where it lands.

Two things about the manifest are load-bearing, and it says so at both. The
actions that need no symlink are declared first, so a platform that cannot make
one still runs them. And the two `profile.zsh` seeds name one destination, so
which of them lands is what declaration order decides — reorder them and a test
that reads this fixture fails, which is the point of their being here.
