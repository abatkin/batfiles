#Requires -Version 7.0
# batfiles-stub 1
$BatfilesBase = if ($env:BATFILES_BASE) { $env:BATFILES_BASE } else { '@BATFILES_BASE@' }
$BatfilesVersion = if ($env:BATFILES_VERSION) { $env:BATFILES_VERSION } else { '' }

# Installs this checkout with batfiles, which `batfiles init` wrote this file
# for: it uses a batfiles already on this machine, or fetches one with the
# hosted installer from $BatfilesBase, then runs
# `batfiles sync --bootstrap --batfiles-dir <this directory>` with any arguments
# given, such as --dry-run. $BatfilesVersion, when set, is the oldest release
# this repository accepts. BATFILES_BIN names the one batfiles to use or
# install. To set up a machine with no checkout, use the hosted installer's
# one-liner with `clone` instead.

function Install-Checkout {
    $arguments = @($args)
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    $PSNativeCommandUseErrorActionPreference = $false

    function Say([string] $message) {
        [Console]::Error.WriteLine("install.ps1: $message")
    }

    function Fail([string] $message) {
        throw "install.ps1: $message"
    }

    # The release version grammar, as the hosted installer has it.
    $VersionPattern = '(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*)?'

    # Whether $text is a version.
    function Test-Version([string] $text) {
        return $text -cmatch "^(?:$VersionPattern)\z"
    }

    # The version the batfiles at $path reports, or $null if it does not run or
    # reports something else.
    function Get-BatfilesVersion([string] $path) {
        try {
            $reported = @(& $path version 2>$null)
        } catch {
            return $null
        }
        if ($LASTEXITCODE -ne 0 -or $reported.Count -ne 1) { return $null }
        $line = [string] $reported[0]
        if (-not $line.StartsWith('batfiles ', [StringComparison]::Ordinal)) { return $null }
        $version = $line.Substring('batfiles '.Length)
        if (-not (Test-Version $version)) { return $null }
        return $version
    }

    # The batfiles to consider, in order: BATFILES_BIN alone when it is set;
    # otherwise the batfiles on PATH and then the default install location,
    # which is where a download goes.
    function Get-CandidateList {
        if ($env:BATFILES_BIN) {
            $binPath = $env:BATFILES_BIN
            if (-not [System.IO.Path]::IsPathRooted($binPath)) {
                $binPath = Join-Path (Get-Location -PSProvider FileSystem).ProviderPath $binPath
            }
            return @{ OnPath = $null; BinPath = $binPath }
        }
        if (-not $env:LOCALAPPDATA) { Fail 'LOCALAPPDATA is not set; set BATFILES_BIN instead' }
        $binPath = Join-Path -Path $env:LOCALAPPDATA -ChildPath 'Programs' -AdditionalChildPath 'batfiles', 'batfiles.exe'
        $found = Get-Command batfiles -CommandType Application -ErrorAction SilentlyContinue |
            Select-Object -First 1
        $onPath = if ($found) { $found.Source } else { $null }
        if ($onPath -eq $binPath) { $onPath = $null }
        return @{ OnPath = $onPath; BinPath = $binPath }
    }

    # This file's directory, which must be a checkout: run any other way than
    # from a file, there is no such directory.
    $dir = $PSScriptRoot
    if (-not $dir -or -not (Test-Path -LiteralPath (Join-Path $dir 'batfiles.toml') -PathType Leaf)) {
        Say 'this runs from a checkout of a batfiles repository; to set up a machine without one, run:'
        Fail "& ([scriptblock]::Create((irm $BatfilesBase/latest/download/install.ps1))) clone <repository-url>"
    }

    $version = $BatfilesVersion
    if ($version.StartsWith('v', [StringComparison]::Ordinal)) { $version = $version.Substring(1) }
    if ($version -ceq 'latest') { $version = '' }
    $sync = @('sync', '--bootstrap', '--batfiles-dir', $dir) + $arguments

    $candidates = Get-CandidateList
    $found = $null
    foreach ($candidate in @($candidates.OnPath, $candidates.BinPath)) {
        if (-not $candidate -or -not (Test-Path -LiteralPath $candidate -PathType Leaf)) { continue }
        if (Get-BatfilesVersion $candidate) {
            $found = $candidate
            break
        }
    }

    # With no version requested, any batfiles will do, and nothing is fetched.
    if ($found -and -not $version) {
        & $found @sync
        exit $LASTEXITCODE
    }

    # Otherwise the hosted installer decides, matching the requested release.
    $from = if ($version) { "$BatfilesBase/download/v$version" } else { "$BatfilesBase/latest/download" }
    $installer = $null
    try {
        $installer = Invoke-RestMethod -Uri "$from/install.ps1"
    } catch {
        $installer = $null
    }
    if ($installer) {
        $env:BATFILES_BASE = $BatfilesBase
        $env:BATFILES_VERSION = $BatfilesVersion
        & ([scriptblock]::Create($installer)) @sync
        exit $LASTEXITCODE
    }
    if ($found) {
        Say "warning: cannot fetch $from/install.ps1 to check $found against BATFILES_VERSION=$BatfilesVersion; using it unchecked"
        & $found @sync
        exit $LASTEXITCODE
    }
    Fail "cannot fetch $from/install.ps1, and this machine has no batfiles to use instead"
}

Install-Checkout @args
