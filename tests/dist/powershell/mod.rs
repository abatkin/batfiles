//! The PowerShell installer and stub: their shared text on every platform, and on Windows what
//! they do, run by the machine's own `pwsh`.

mod text;

#[cfg(windows)]
mod install;
#[cfg(windows)]
mod stub;
#[cfg(windows)]
mod support;
