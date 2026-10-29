<#
.SYNOPSIS
Stage the C# baseline's pinned x64 LibRaw and TurboJPEG DLLs (no downloads).
.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev/stage-native.ps1   # default: target\debug, target\debug\deps and native\x64
.EXAMPLE
powershell -NoProfile -ExecutionPolicy Bypass -File tools/dev/stage-native.ps1 -Check
#>
[CmdletBinding()]
param(
    [string]$Destination,
    [switch]$Check
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
# $PSScriptRoot is not reliable inside a param() default on Windows PowerShell 5.1, so the default is computed here.
# Without -Destination the DLLs go where the Rust tests look for them: the exe folder (target\debug), the test exe folder (target\debug\deps, decision P-50)
# and natived (the pr-turbojpeg native tests load it by absolute path; without it they print 'skipping' and still report green).
$destinations = if ($Destination) { @($Destination) } else { @((Join-Path $PSScriptRoot '..\..\target\debug'), (Join-Path $PSScriptRoot '..\..\target\debug\deps'), (Join-Path $PSScriptRoot '..\..\native\x64')) }
try {
    # Like sync-from-csharp.ps1: resolve relative to the MAIN checkout, not this worktree.
    $commonOutput = @(& git -C $PSScriptRoot rev-parse --path-format=absolute --git-common-dir)
    if ($LASTEXITCODE -ne 0 -or $commonOutput.Count -ne 1) {
        throw 'Cannot resolve the main checkout git directory.'
    }
    $common = $commonOutput[0].Trim()
    $csharp = Join-Path (Split-Path (Split-Path $common)) 'PhotoReview'
    $verified = @()
    foreach ($name in @('libraw', 'turbojpeg')) {
        $pinOutput = @(& git -C $csharp show "7eaee2a2:native/$name.sha256")
        if ($LASTEXITCODE -ne 0 -or $pinOutput.Count -ne 1) {
            throw "Cannot read pinned SHA-256 for $name at C# baseline 7eaee2a2."
        }
        $pin = $pinOutput[0].Trim()
        if ($pin -notmatch '^[0-9a-fA-F]{64}$') {
            throw "Invalid pinned SHA-256 for $name at C# baseline 7eaee2a2."
        }
        $source = Join-Path $csharp "native\x64\$name.dll"
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
            throw "Missing source DLL: $source"
        }
        $actual = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash
        if ($actual -ne $pin) {
            throw "SHA-256 mismatch for $source : expected $pin, got $actual. No DLLs copied."
        }
        $verified += [pscustomobject]@{ Name = "$name.dll"; Source = $source; Hash = $pin }
    }
    if ($Check) {
        Write-Output 'PASS: both source DLLs match C# baseline 7eaee2a2; -Check copied nothing.'
        exit 0
    }
    foreach ($destination in $destinations) {
        $destinationPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($destination)
        [System.IO.Directory]::CreateDirectory($destinationPath) | Out-Null
        foreach ($dll in $verified) {
            $target = Join-Path $destinationPath $dll.Name
            Copy-Item -LiteralPath $dll.Source -Destination $target -Force
            if ((Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -ne $dll.Hash) {
                throw "SHA-256 mismatch after copying $target. Staging incomplete; do not use this destination."
            }
        }
        Write-Output "PASS: staged and verified libraw.dll and turbojpeg.dll in $destinationPath"
    }
    exit 0
} catch {
    [Console]::Error.WriteLine("stage-native: {0}", $_.Exception.Message)
    exit 1
}
