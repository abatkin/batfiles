//! The release scripts under `dist/` and the stubs `init` writes: the POSIX ones run with `sh`
//! against stand-in binaries and `file://` or loopback release trees, and the PowerShell ones
//! natively on Windows, against compiled stand-ins over loopback HTTP. `dist/binary.sh` and
//! `dist/publish.sh` need real toolchains and GitHub, so the release workflow is what exercises
//! them.

mod powershell;

#[cfg(unix)]
mod support;

#[cfg(unix)]
mod assemble;
#[cfg(unix)]
mod install;
#[cfg(unix)]
mod mirror;
#[cfg(unix)]
mod pages;
#[cfg(unix)]
mod release;
#[cfg(unix)]
mod smoke;
#[cfg(unix)]
mod stub;
#[cfg(unix)]
mod verify;
