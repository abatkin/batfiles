# The corporate repository

A dotfiles repository a leaf manifest declares as a remote, standing in for the
work half of the acceptance in `rewrite/README.md`: settings an employer
publishes once, that a personal repository composes over.

Unlike the other fixtures here, this tree is not copied into a leaf repository.
`BareRepo::from_fixture("corporate")` commits it into a local bare repository,
which a manifest then names by `url` -- so what a test drives is a real clone of
a real repository rather than a directory pretending to be one.

What is here is what an action installs from: a file to link, a directory of
seeds to copy. A clone list is not, because a list names repositories by path
and the ones a test creates live in a temporary directory no committed file can
know; those tests publish their own list.

This repository declares nothing of its own yet. A `batfiles.toml` arrives with
`include-remote` at step 7.1, when something reads one.
