//! The release scripts under `dist/`, run with `sh` against stand-in binaries and `file://` or
//! loopback release trees. `dist/binary.sh` and `dist/publish.sh` need real toolchains and
//! GitHub, so the release workflow is what exercises them.
#![cfg(unix)]

mod support;

mod assemble;
mod install;
mod mirror;
mod release;
mod smoke;
mod stub;
mod verify;
