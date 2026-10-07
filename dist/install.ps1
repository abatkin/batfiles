# The batfiles hosted installer for Windows, run by PowerShell 7 or later.
#
#   irm <base>/latest/download/install.ps1 | iex
#   & ([scriptblock]::Create((irm <base>/latest/download/install.ps1))) clone <url>
#
# Uses a batfiles already on this machine, or downloads and verifies one, then
# runs it with any arguments given. Inputs, all optional: BATFILES_BASE (the
# release base, by default the one stamped below), BATFILES_VERSION (a release
# to fetch, and the oldest one accepted), and BATFILES_BIN (the one batfiles to
# use or install, instead of searching PATH and then
# $env:LOCALAPPDATA\Programs\batfiles). docs/contributing/distribution.md specifies the rest.
#
# Failures are terminating errors rather than `exit`, which would close the
# session the one-liner runs in. Batfiles' own status is left in $LASTEXITCODE.
#
# `dist/assemble.sh` stamps the release base into the line below. An unstamped
# copy runs only with BATFILES_BASE set.
$BatfilesStampedBase = 'unstamped'

function Invoke-BatfilesInstaller {
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

    # SemVer without build metadata, as docs/contributing/distribution.md#versions specifies.
    # dist/version.sh holds the same pattern; tests/dist checks they agree.
    $VersionPattern = '(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*)?'

    # Whether $text is a version.
    function Test-Version([string] $text) {
        return $text -cmatch "^(?:$VersionPattern)\z"
    }

    # -1, 0, or 1 as version $a precedes, equals, or follows version $b, both
    # valid, by SemVer precedence: a pre-release ranks below its release, and
    # its identifiers compare as numbers when both are digits and as ASCII
    # strings otherwise. Numbers compare exactly, by length and then digit by
    # digit, which needs no leading zeros.
    function Compare-Version([string] $a, [string] $b) {
        function Compare-Number([string] $x, [string] $y) {
            if ($x.Length -ne $y.Length) { return [Math]::Sign($x.Length - $y.Length) }
            return [Math]::Sign([string]::CompareOrdinal($x, $y))
        }
        $coreA, $preA = $a.Split('-', 2)
        $coreB, $preB = $b.Split('-', 2)
        $numbersA = $coreA.Split('.')
        $numbersB = $coreB.Split('.')
        for ($k = 0; $k -lt 3; $k++) {
            $order = Compare-Number $numbersA[$k] $numbersB[$k]
            if ($order -ne 0) { return $order }
        }
        if (-not $preA -and -not $preB) { return 0 }
        if (-not $preA) { return 1 }
        if (-not $preB) { return -1 }
        $x = $preA.Split('.')
        $y = $preB.Split('.')
        for ($k = 0; $k -lt $x.Count -and $k -lt $y.Count; $k++) {
            $numericX = $x[$k] -cmatch '^[0-9]+\z'
            $numericY = $y[$k] -cmatch '^[0-9]+\z'
            if ($numericX -and $numericY) { $order = Compare-Number $x[$k] $y[$k] }
            elseif ($numericX) { $order = -1 }
            elseif ($numericY) { $order = 1 }
            else { $order = [Math]::Sign([string]::CompareOrdinal($x[$k], $y[$k])) }
            if ($order -ne 0) { return $order }
        }
        return [Math]::Sign($x.Count - $y.Count)
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

    # The release target for this machine. A 32-bit PowerShell on a 64-bit
    # Windows reports the machine's architecture in PROCESSOR_ARCHITEW6432, and
    # Windows on ARM runs the x86_64 binary under emulation.
    function Get-Target {
        if (-not $IsWindows) {
            Fail 'this installer is for Windows; on Linux or macOS, use install.sh'
        }
        $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
        switch ($arch) {
            'AMD64' { return 'x86_64-pc-windows-msvc' }
            'ARM64' { return 'x86_64-pc-windows-msvc' }
            default { Fail "there is no batfiles release for Windows on $arch" }
        }
    }

    # Download $url to $path, or return $false.
    function Receive-File([string] $url, [string] $path) {
        try {
            Invoke-WebRequest -Uri $url -OutFile $path
            return $true
        } catch {
            return $false
        }
    }

    # Create an empty file in $dir, named for $prefix and $suffix, and return its
    # path, failing if it cannot.
    function Add-ScratchFile([string] $dir, [string] $prefix, [string] $suffix) {
        $path = Join-Path $dir ($prefix + [guid]::NewGuid().ToString('N') + $suffix)
        try {
            [System.IO.File]::Open($path, 'CreateNew').Dispose()
        } catch {
            Fail "cannot write in $dir"
        }
        return $path
    }

    # Download, verify, and install the requested release as $binPath, and
    # return its version. Refuses to replace anything there that is not
    # batfiles.
    function Install-Release([string] $base, [string] $want, [string] $binPath) {
        if (Test-Path -LiteralPath $binPath -PathType Container) {
            Fail "$binPath is a directory; set BATFILES_BIN to the path of the batfiles to install"
        }
        if ((Test-Path -LiteralPath $binPath) -and -not (Get-BatfilesVersion $binPath)) {
            Fail "$binPath is not a batfiles, so it is left as it is; move it aside, or set BATFILES_BIN to another path"
        }
        $target = Get-Target
        $asset = "batfiles-$target.exe"
        $binDir = Split-Path -Parent $binPath
        try {
            New-Item -ItemType Directory -Force -Path $binDir | Out-Null
        } catch {
            Fail "cannot create $binDir"
        }
        $staged = Add-ScratchFile -dir $binDir -prefix '.batfiles.' -suffix '.exe'
        $sums = Add-ScratchFile -dir $binDir -prefix '.SHA256SUMS.' -suffix ''
        try {
            # The latest release is read once, from its VERSION, and everything
            # else comes from that release's own directory, so a release
            # published midway cannot mix two.
            $release = $want
            if (-not $release) {
                if (-not (Receive-File "$base/latest/download/VERSION" $sums)) {
                    Say "cannot download $base/latest/download/VERSION"
                    Fail 'if this release base has no stable release yet, set BATFILES_VERSION to a pre-release'
                }
                $release = [string] (Get-Content -LiteralPath $sums -TotalCount 1)
                if (-not (Test-Version $release)) {
                    Fail "$base/latest/download/VERSION holds '$release', not a version"
                }
            }
            $from = "$base/download/v$release"

            Say "downloading $asset $release from $from"
            if (-not (Receive-File "$from/$asset" $staged)) { Fail "cannot download $from/$asset" }
            if (-not (Receive-File "$from/SHA256SUMS" $sums)) { Fail "cannot download $from/SHA256SUMS" }
            $expected = $null
            foreach ($line in Get-Content -LiteralPath $sums) {
                $fields = $line -split '\s+'
                if ($fields.Count -ge 2 -and $fields[1] -ceq $asset) {
                    $expected = $fields[0]
                    break
                }
            }
            if (-not $expected) { Fail "$from/SHA256SUMS lists no $asset" }
            $actual = (Get-FileHash -LiteralPath $staged -Algorithm SHA256).Hash
            if ($actual -ne $expected) {
                Fail "$asset does not match $from/SHA256SUMS; nothing was installed"
            }

            $installed = Get-BatfilesVersion $staged
            if (-not $installed) {
                Fail "the downloaded $asset does not run on this machine; nothing was installed"
            }
            if ($installed -cne $release) {
                Fail "the download reports version $installed, not $release; nothing was installed"
            }
            try {
                Move-Item -LiteralPath $staged -Destination $binPath -Force
            } catch {
                Fail "cannot install $binPath"
            }
        } finally {
            Remove-Item -LiteralPath $staged, $sums -Force -ErrorAction SilentlyContinue
        }
        Say "installed batfiles $installed as $binPath"
        $onPath = @($env:PATH -split [System.IO.Path]::PathSeparator) |
            Where-Object { $_ -and $_.TrimEnd('\', '/') -eq $binDir.TrimEnd('\', '/') }
        if (-not $onPath) {
            $quoted = $binDir.Replace("'", "''")
            Say "$binDir is not on PATH; to add it for this user, run:"
            Say "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path', 'User') + ';$quoted', 'User')"
        }
        return $installed
    }

    if ($PSVersionTable.PSVersion.Major -lt 7) {
        Fail "this is PowerShell $($PSVersionTable.PSVersion); batfiles installs with PowerShell 7 or later, which 'winget install Microsoft.PowerShell' provides. Run this again in pwsh"
    }

    $base = if ($env:BATFILES_BASE) { $env:BATFILES_BASE } else { $BatfilesStampedBase }
    if ($base -ceq 'unstamped') {
        Fail 'this copy was never stamped with a release base; set BATFILES_BASE, or use the one published with a release'
    }
    if ($base.EndsWith('/')) { $base = $base.Substring(0, $base.Length - 1) }

    $want = [string] $env:BATFILES_VERSION
    if ($want -ceq 'latest') {
        $want = ''
    } elseif ($want.StartsWith('v', [StringComparison]::Ordinal)) {
        $want = $want.Substring(1)
    }
    if ($want -and -not (Test-Version $want)) {
        Fail "BATFILES_VERSION is '$env:BATFILES_VERSION', not a version such as 1.2.3 or 1.2.3-rc.1"
    }

    # The first candidate that runs and is at least the requested version wins.
    $candidates = Get-CandidateList
    $bin = $null
    foreach ($candidate in @($candidates.OnPath, $candidates.BinPath)) {
        if (-not $candidate -or -not (Test-Path -LiteralPath $candidate -PathType Leaf)) { continue }
        $have = Get-BatfilesVersion $candidate
        if (-not $have) {
            # What is at the install location is reported if a download would replace it.
            if ($candidate -eq $candidates.OnPath) {
                Say "passing over $candidate, which does not report a batfiles version"
            }
            continue
        }
        if (-not $want -or (Compare-Version $have $want) -ge 0) {
            $bin = $candidate
            Say "using batfiles $have at $bin"
            break
        }
        if ($candidate -eq $candidates.OnPath) {
            Say "warning: passing over $candidate, batfiles $have, which is older than $want; it is left as it is"
        }
    }

    if (-not $bin) {
        Install-Release -base $base -want $want -binPath $candidates.BinPath | Out-Null
        $bin = $candidates.BinPath
    } elseif ($arguments.Count -eq 0 -and -not $want) {
        Say "to upgrade it, run 'batfiles update'"
    }

    if ($arguments.Count -eq 0) { return }
    & $bin @arguments
}

Invoke-BatfilesInstaller @args
