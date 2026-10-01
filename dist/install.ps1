# The batfiles hosted installer for Windows. A placeholder until the installer
# is built: it names where the release's binaries are, and installs nothing.
#
# `dist/assemble.sh` stamps the release base into the line below; this unstamped
# source refuses to run.
$BatfilesStampedBase = 'unstamped'

function Install-Batfiles {
    if ($BatfilesStampedBase -eq 'unstamped') {
        throw 'install.ps1: this copy was never stamped with a release base; use the one published with a release'
    }
    $base = if ($env:BATFILES_BASE) { $env:BATFILES_BASE } else { $BatfilesStampedBase }
    throw "install.ps1: this release has no installer yet; download batfiles-x86_64-pc-windows-msvc.exe from $base/latest/download/"
}

Install-Batfiles @args
